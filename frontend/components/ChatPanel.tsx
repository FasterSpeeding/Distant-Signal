'use client';

import { useRef, useState, type FormEvent } from 'react';
import { Alert, Button, Card, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import Link from 'next/link';
import type { RenderedTrainLeg } from '@/lib/types';

/** One SSE frame's parsed `data:` payload -- mirrors `orchestrator/src/chat.ts`'s
 * own `ChatEvent` union (a different repository/deploy unit, no shared
 * package to import the type from) plus the error shape `orchestrator/src/app.ts`
 * writes on a mid-stream failure. */
type ChatStreamEvent =
  | { type: 'text-delta'; text: string }
  | { type: 'tool-result'; toolName: string; structuredContent?: unknown }
  | { type: 'done' }
  | { type: 'error'; error: string };

interface ChatMessage {
  role: 'user' | 'assistant';
  content: string;
  /** `plan_journey` tool-result events whose `structuredContent` looks
   * like a `RenderedTrainLeg` (`kind: 'train'`), attached to whichever
   * assistant turn produced them -- rendered as "track this leg" cards
   * below that turn's own text. */
  legs: RenderedTrainLeg[];
}

/** Narrow, structural check -- distant-signal-mcp is a separate
 * repository/deploy unit with no shared type to import, so this is the
 * boundary where an unknown `structuredContent` value either is or isn't
 * trusted as a `RenderedTrainLeg`. Deliberately loose (checks the
 * discriminant and the two fields this card actually reads, not every
 * field the real interface declares) -- a `plan_journey` result carrying
 * extra fields this app doesn't use should still render, not be rejected
 * for "not matching exactly". */
function asRenderedTrainLeg(value: unknown): RenderedTrainLeg | null {
  if (typeof value !== 'object' || value === null) return null;
  const v = value as Record<string, unknown>;
  if (v.kind !== 'train') return null;
  const from = v.from as Record<string, unknown> | undefined;
  if (typeof from?.crs !== 'string' && from?.crs !== null) return null;
  if (typeof v.uid !== 'string') return null;
  return v as unknown as RenderedTrainLeg;
}

function parseSseFrames(buffer: string): { events: ChatStreamEvent[]; rest: string } {
  const events: ChatStreamEvent[] = [];
  const parts = buffer.split('\n\n');
  // The last part is either an empty string (buffer ended exactly on a
  // frame boundary) or a partial frame still awaiting more bytes -- either
  // way, it's not a complete frame yet, so it's carried forward rather
  // than parsed.
  const rest = parts.pop() ?? '';
  for (const part of parts) {
    const line = part.split('\n').find((l) => l.startsWith('data: '));
    if (!line) continue;
    try {
      events.push(JSON.parse(line.slice('data: '.length)) as ChatStreamEvent);
    } catch {
      // A malformed frame is skipped, not fatal to the rest of the
      // stream -- one bad event shouldn't take down an otherwise-working
      // conversation.
    }
  }
  return { events, rest };
}

/** The chat UI's own message list + input (embedded-chatbot-option-b plan,
 * Task 5 Step 3). A Client Component -- it needs `fetch`+`ReadableStream`
 * reading and local message state, neither available to `app/chat/page.tsx`'s
 * Server Component.
 *
 * Submits through the same-origin `/api/chat` proxy (Task 4), reading
 * `response.body.getReader()` manually rather than `EventSource`: native
 * `EventSource` is GET-only and can't carry a POST body or this app's own
 * `Cookie`-forwarding proxy path -- a concrete, implementation-time choice
 * the dual-mode design's own Decision 4 left unresolved. */
export function ChatPanel() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const viewport = useRef<HTMLDivElement>(null);

  function scrollToBottom() {
    // A convenience, never load-bearing: guarded so an environment without
    // a real `scrollTo` implementation (jsdom in this file's own tests)
    // can't turn a scroll nicety into a thrown exception that aborts the
    // SSE read loop mid-stream -- confirmed as a real failure mode this
    // session (an unguarded call here silently dropped every event after
    // the first).
    if (typeof viewport.current?.scrollTo === 'function') {
      viewport.current.scrollTo({ top: viewport.current.scrollHeight });
    }
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const message = input.trim();
    if (!message || sending) return;

    setError(null);
    setInput('');
    setSending(true);
    setMessages((prev) => [...prev, { role: 'user', content: message, legs: [] }]);
    setMessages((prev) => [...prev, { role: 'assistant', content: '', legs: [] }]);
    // Index of the assistant turn just pushed above -- both pushes above
    // are synchronous state updates within this same handler, so `prev`
    // reflects the array as of the previous call each time; capturing the
    // resulting length up front avoids any ambiguity from relying on
    // "the last element" after further updates land.
    let assistantIndex = -1;
    setMessages((prev) => {
      assistantIndex = prev.length - 1;
      return prev;
    });

    try {
      const res = await fetch('/api/chat', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ message }),
      });

      if (res.status === 401 || res.status === 403) {
        setError(
          res.status === 401
            ? 'Your session has expired -- sign in again to keep chatting.'
            : 'Chat is not available for your account.',
        );
        setMessages((prev) => prev.slice(0, -1));
        return;
      }
      if (!res.ok || !res.body) {
        setError('Something went wrong reaching the chat service.');
        setMessages((prev) => prev.slice(0, -1));
        return;
      }

      const reader = res.body.getReader();
      const decoder = new TextDecoder();
      let buffer = '';
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buffer += decoder.decode(value, { stream: true });
        const { events, rest } = parseSseFrames(buffer);
        buffer = rest;
        for (const event of events) {
          applyEvent(event, assistantIndex, setMessages);
          scrollToBottom();
        }
      }
    } catch {
      setError('Lost connection to the chat service.');
    } finally {
      setSending(false);
    }
  }

  return (
    <Stack gap="md" h="100%" style={{ flex: 1, minHeight: 0 }}>
      {error && (
        <Alert color="red" variant="light">
          {error}
        </Alert>
      )}
      <ScrollArea viewportRef={viewport} style={{ flex: 1 }} offsetScrollbars>
        <Stack gap="md" p="xs">
          {messages.length === 0 && (
            <Text c="dimmed" ta="center" mt="xl">
              Ask about live departures, disruptions, or plan a journey.
            </Text>
          )}
          {messages.map((message, index) => (
            <ChatMessageRow key={index} message={message} />
          ))}
        </Stack>
      </ScrollArea>
      <form onSubmit={handleSubmit}>
        <Group gap="xs" align="flex-end">
          <TextInput
            style={{ flex: 1 }}
            placeholder="When's the next train from King's Cross?"
            value={input}
            onChange={(event) => setInput(event.currentTarget.value)}
            disabled={sending}
          />
          <Button type="submit" loading={sending} disabled={!input.trim()}>
            Send
          </Button>
        </Group>
      </form>
    </Stack>
  );
}

function applyEvent(
  event: ChatStreamEvent,
  assistantIndex: number,
  setMessages: React.Dispatch<React.SetStateAction<ChatMessage[]>>,
) {
  if (event.type === 'text-delta') {
    setMessages((prev) => {
      const next = [...prev];
      const target = next[assistantIndex];
      if (target) {
        next[assistantIndex] = { ...target, content: target.content + event.text };
      }
      return next;
    });
    return;
  }
  if (event.type === 'tool-result') {
    const leg = asRenderedTrainLeg(event.structuredContent);
    if (!leg) return;
    setMessages((prev) => {
      const next = [...prev];
      const target = next[assistantIndex];
      if (target) {
        next[assistantIndex] = { ...target, legs: [...target.legs, leg] };
      }
      return next;
    });
    return;
  }
  if (event.type === 'error') {
    setMessages((prev) => {
      const next = [...prev];
      const target = next[assistantIndex];
      if (target && !target.content) {
        next[assistantIndex] = { ...target, content: 'Sorry, something went wrong answering that.' };
      }
      return next;
    });
  }
  // 'done' needs no state change -- the stream ending IS the signal.
}

function ChatMessageRow({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  return (
    <Stack gap={4} align={isUser ? 'flex-end' : 'flex-start'}>
      <Card withBorder padding="sm" radius="md" maw="80%" bg={isUser ? 'blue.0' : undefined}>
        <Text style={{ whiteSpace: 'pre-wrap' }}>{message.content || (isUser ? '' : '…')}</Text>
      </Card>
      {message.legs.map((leg, index) => (
        <TrainLegCard key={index} leg={leg} />
      ))}
    </Stack>
  );
}

/** A `plan_journey` result's leg, rendered as a small card with a "Track
 * this train" deep-link into `TrackTrainForm` (Task 5's own scope note: not
 * a full pre-fill of every `TrackTrainForm` field, just `origin` --
 * `TrackTrainForm`'s existing `initialOrigin` prop, the same mechanism
 * `/stations/[crs]`'s own "Track a train from here" shortcut already
 * uses). A leg with no CRS (`from.crs === null` -- `RenderedTrainLeg`'s own
 * TIPLOC-only fallback, per distant-signal-mcp's `StationRef`) has nothing
 * `/track?origin=` can pre-fill, so no button renders for it; the card
 * itself still does, so the leg's own detail isn't silently dropped. */
function TrainLegCard({ leg }: { leg: RenderedTrainLeg }) {
  const originCrs = leg.from.crs;
  const originName = leg.from.name ?? leg.from.tiploc;
  const destinationName = leg.to.name ?? leg.to.tiploc;
  return (
    <Card withBorder padding="sm" radius="md" maw="80%">
      <Stack gap={4}>
        <Text size="sm" fw={500}>
          {originName} → {destinationName}
        </Text>
        <Text size="xs" c="dimmed">
          {leg.departure}
          {leg.operator ? ` · ${leg.operator}` : ''}
        </Text>
        {originCrs && (
          <Link href={`/track?origin=${encodeURIComponent(originCrs)}`} style={{ textDecoration: 'none' }}>
            <Button size="xs" variant="light">
              Track this train
            </Button>
          </Link>
        )}
      </Stack>
    </Card>
  );
}
