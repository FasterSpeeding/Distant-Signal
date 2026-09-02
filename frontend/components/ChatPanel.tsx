'use client';

import { useRef, useState, type FormEvent } from 'react';
import { Alert, Button, Card, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import Anthropic from '@anthropic-ai/sdk';
import Link from 'next/link';
import type { RenderedTrainLeg } from '@/lib/types';
import { getAnthropicApiKey } from '@/lib/anthropicKey';
import { BrowserMcpOAuthProvider } from '@/lib/mcpOAuthProvider';
import { AnthropicKeySettings } from './AnthropicKeySettings';
import { runChatTurn, type ChatEvent } from '@/lib/chatTurn';

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

// Same unresearched-starting-figure posture as orchestrator/'s own model
// choice (now deleted, see this plan's Task 5) -- carried forward
// unchanged, not re-benchmarked by this task.
const CHAT_MODEL = 'claude-opus-4-6';

type ChatError =
  | { kind: 'no-key' }
  | { kind: 'anthropic-rejected' }
  | { kind: 'mcp-reconnect' }
  | { kind: 'tool-error'; message: string };

/** The chat UI's own message list + input (embedded-chatbot-option-b-
 * client-side-tokens plan, Task 10). A Client Component -- it needs the
 * user's own localStorage-held Anthropic key and MCP tokens, and runs the
 * tool-calling loop (`runChatTurn`, Task 1's `orchestrator/src/chat.ts`
 * relocated) directly in the browser now, not through a server-side
 * proxy -- there is no longer a server-side orchestrator to talk to
 * (Decision 1/3 of the client-side-tokens design doc). */
export function ChatPanel() {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<ChatError | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const historyRef = useRef<Anthropic.Beta.Messages.BetaMessageParam[]>([]);

  function scrollToBottom() {
    // A convenience, never load-bearing: guarded so an environment without
    // a real `scrollTo` implementation (jsdom in this file's own tests)
    // can't turn a scroll nicety into a thrown exception mid-loop.
    if (typeof viewport.current?.scrollTo === 'function') {
      viewport.current.scrollTo({ top: viewport.current.scrollHeight });
    }
  }

  async function handleSubmit(event: FormEvent) {
    event.preventDefault();
    const trimmed = input.trim();
    if (!trimmed || sending) return;

    const apiKey = getAnthropicApiKey();
    if (!apiKey) {
      setError({ kind: 'no-key' });
      return;
    }

    const provider = new BrowserMcpOAuthProvider(`${window.location.origin}/chat/callback`);
    const tokens = provider.tokens();
    if (!tokens) {
      setError({ kind: 'mcp-reconnect' });
      return;
    }

    setError(null);
    setInput('');
    setSending(true);
    setMessages((prev) => [...prev, { role: 'user', content: trimmed, legs: [] }]);
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
      const anthropic = new Anthropic({ apiKey, dangerouslyAllowBrowser: true });
      let assistantText = '';

      for await (const event of runChatTurn({
        anthropic,
        model: CHAT_MODEL,
        mcpUrl: `${process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL}/mcp`,
        mcpAuthProvider: provider,
        conversationHistory: historyRef.current,
        userMessage: trimmed,
      })) {
        if (event.type === 'text-delta') assistantText += event.text;
        applyChatEvent(event, assistantIndex, setMessages);
        scrollToBottom();
      }

      historyRef.current = [
        ...historyRef.current,
        { role: 'user', content: trimmed },
        { role: 'assistant', content: assistantText },
      ];
    } catch (err) {
      // Drop only the empty pending assistant turn -- the user's own
      // message stays visible, with the error shown alongside it, rather
      // than silently disappearing too.
      setMessages((prev) => prev.slice(0, -1));
      setError(classifyChatError(err));
    } finally {
      setSending(false);
    }
  }

  return (
    <Stack gap="md" h="100%" style={{ flex: 1, minHeight: 0 }}>
      <AnthropicKeySettings />
      {error && <ChatErrorAlert error={error} />}
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
            placeholder="Ask about the next train, delays, or plan a journey…"
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

/** Anthropic's own `APIError` (and its `AuthenticationError` subclass, the
 * real shape a rejected/invalid key throws as) carries a numeric `status`.
 * Checked structurally as well as via `instanceof` so a test double or any
 * other Anthropic-error-shaped value (constructor named `APIError`, a
 * `status` of 401) is still recognized without depending on the real
 * class's prototype chain. */
function isAnthropicAuthError(err: unknown): boolean {
  if (err instanceof Anthropic.APIError) return err.status === 401;
  if (err && typeof err === 'object' && 'status' in err) {
    const status = (err as { status?: unknown }).status;
    const ctorName = (err as { constructor?: { name?: string } }).constructor?.name;
    return status === 401 && ctorName === 'APIError';
  }
  return false;
}

function classifyChatError(err: unknown): ChatError {
  if (isAnthropicAuthError(err)) {
    return { kind: 'anthropic-rejected' };
  }
  const message = err instanceof Error ? err.message : 'Something went wrong.';
  if (/401|403|unauthoriz/i.test(message)) {
    return { kind: 'mcp-reconnect' };
  }
  return { kind: 'tool-error', message };
}

function ChatErrorAlert({ error }: { error: ChatError }) {
  switch (error.kind) {
    case 'no-key':
      return (
        <Alert color="orange" variant="light">
          Set your Anthropic API key below to start chatting.
        </Alert>
      );
    case 'anthropic-rejected':
      return (
        <Alert color="red" variant="light">
          Your Anthropic API key was rejected. Check that it&apos;s correct and try again.
        </Alert>
      );
    case 'mcp-reconnect':
      return (
        <Alert color="red" variant="light">
          Your connection to the rail data service has expired or was not found -- reconnect from the Chat page to
          keep chatting.
        </Alert>
      );
    case 'tool-error':
      return (
        <Alert color="red" variant="light">
          Something went wrong answering that: {error.message}
        </Alert>
      );
  }
}

function applyChatEvent(
  event: ChatEvent,
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
