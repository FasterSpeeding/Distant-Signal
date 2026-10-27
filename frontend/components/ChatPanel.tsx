'use client';

import { useEffect, useRef, useState, useSyncExternalStore, type SubmitEvent } from 'react';
import { Alert, Button, Card, Code, Group, ScrollArea, Stack, Text, TextInput } from '@mantine/core';
import { LoadingText } from '@/components/LoadingText';
import type Anthropic from '@anthropic-ai/sdk';
import type { RenderedTrainLeg } from '@/lib/types';
import { getAnthropicApiKey } from '@/lib/anthropicKey';
import { chatOAuthProvider, startMcpSignIn } from '@/lib/mcpAuthorization';
import { mcpEndpointUrl } from '@/lib/mcpInstallLinks';
import { TextLink } from './TextLink';
import { AiGeneratedBadge, CHAT_AI_NOTE } from './AiGeneratedBadge';
import type { ChatEvent, ConfirmToolCall } from '@/lib/chatTurn';
import type { ChatError } from '@/lib/chatErrors';
import { loadChatRuntime, type ChatRuntime } from '@/lib/chatRuntime';
import { ButtonLink } from '@/components/ButtonLink';
import { PREFERENCES_DESTINATION } from '@/lib/navLinks';

const PREFERENCES_HREF = PREFERENCES_DESTINATION.href;

interface ChatMessage {
  /** Stable per-panel id (FE-11): streamed events find their assistant turn
   * by id, not by an array index captured from React's scheduler. */
  id: number;
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

/** Warms `loadChatRuntime`; a failure here is retried, and reported, on Send. */
function preloadChatRuntime(): void {
  loadChatRuntime().catch(() => undefined);
}

function noSubscription(): () => void {
  return () => undefined;
}

/** The error states whose way forward is (re)running the MCP sign-in. */
function needsSignIn(error: ChatError): boolean {
  return (
    error.kind === 'mcp-connect' ||
    error.kind === 'mcp-reconnect' ||
    error.kind === 'mcp-incomplete' ||
    error.kind === 'sign-in-failed'
  );
}

interface ChatPanelProps {
  /** The MCP server's public base URL (`railMcp.publicUrl`), read at
   * request time by the `/chat` Server Component and passed down. FE-2: this
   * used to be `process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL`, which Next
   * inlines into the browser bundle at `next build` -- and the image is
   * built without it, so the shipped bundle said `"undefined/mcp"`. */
  mcpServerUrl: string;
}

/** The chat UI's own message list + input (embedded-chatbot-option-b-
 * client-side-tokens plan, Task 10). A Client Component -- it needs the
 * user's own localStorage-held Anthropic key and MCP tokens, and runs the
 * tool-calling loop (`runChatTurn`, Task 1's `orchestrator/src/chat.ts`
 * relocated) directly in the browser now, not through a server-side
 * proxy -- there is no longer a server-side orchestrator to talk to
 * (Decision 1/3 of the client-side-tokens design doc). */
export function ChatPanel({ mcpServerUrl }: ChatPanelProps) {
  const [messages, setMessages] = useState<ChatMessage[]>([]);
  const [input, setInput] = useState('');
  const [sending, setSending] = useState(false);
  const [error, setError] = useState<ChatError | null>(null);
  const viewport = useRef<HTMLDivElement>(null);
  const historyRef = useRef<Anthropic.Beta.Messages.BetaMessageParam[]>([]);
  const nextMessageId = useRef(0);
  // DQ12 (FE-6): a tool call waiting on the passenger's Allow/Don't allow.
  const [pendingTool, setPendingTool] = useState<{ toolName: string; args: Record<string, unknown> } | null>(null);
  const confirmResolver = useRef<((allowed: boolean) => void) | null>(null);
  const [signingIn, setSigningIn] = useState(false);

  // A sign-in that left for the authorization server and never came back
  // through /chat/callback -- most often because the server refused a
  // stale registration with a bare 400 page. Say so, and offer Reconnect
  // (which starts over with a fresh registration), instead of leaving the
  // visitor to discover it on their next message. Read via
  // `useSyncExternalStore` so the server render (no localStorage) and the
  // first client render agree.
  const abandonedAuthorization = useSyncExternalStore(
    noSubscription,
    () => chatOAuthProvider().hasAbandonedAuthorization(),
    () => false,
  );
  const shownError: ChatError | null = error ?? (abandonedAuthorization ? { kind: 'mcp-incomplete' } : null);

  async function signIn() {
    setSigningIn(true);
    try {
      await startMcpSignIn(mcpServerUrl);
      // 'REDIRECT': the browser is already navigating away; leave the
      // button busy until it does.
    } catch (err) {
      setSigningIn(false);
      setError({
        kind: 'sign-in-failed',
        message: err instanceof Error ? err.message : '',
      });
    }
  }

  const confirmToolCall: ConfirmToolCall = (request) =>
    new Promise<boolean>((resolve) => {
      confirmResolver.current?.(false);
      confirmResolver.current = resolve;
      setPendingTool(request);
    });

  function answerToolCall(allowed: boolean) {
    confirmResolver.current?.(allowed);
    confirmResolver.current = null;
    setPendingTool(null);
  }

  // Never leave the tool loop waiting on a panel that has gone away.
  useEffect(
    () => () => {
      confirmResolver.current?.(false);
      confirmResolver.current = null;
    },
    [],
  );

  function scrollToBottom() {
    // A convenience, never load-bearing: guarded so an environment without
    // a real `scrollTo` implementation (jsdom in this file's own tests)
    // can't turn a scroll nicety into a thrown exception mid-loop.
    if (typeof viewport.current?.scrollTo === 'function') {
      viewport.current.scrollTo({ top: viewport.current.scrollHeight });
    }
  }

  async function handleSubmit(event: SubmitEvent) {
    event.preventDefault();
    const trimmed = input.trim();
    if (!trimmed || sending) return;

    const apiKey = getAnthropicApiKey();
    if (!apiKey) {
      setError({ kind: 'no-key' });
      return;
    }

    const provider = chatOAuthProvider();
    const hadTokens = provider.tokens() !== undefined;
    // Drops a registration old enough that the server has expired it
    // (and the tokens issued to it) -- see `MCP_CLIENT_MAX_AGE_MS`.
    provider.clientInformation();
    if (!provider.tokens()) {
      setError({ kind: hadTokens ? 'mcp-reconnect' : 'mcp-connect' });
      return;
    }

    setError(null);
    setInput('');
    setSending(true);
    // FE-11: the assistant turn is addressed by an id allocated here, not
    // by an index read back out of a no-op state updater (which only worked
    // because React happened to flush it before the first await).
    const userId = nextMessageId.current++;
    const assistantId = nextMessageId.current++;
    setMessages((prev) => [
      ...prev,
      { id: userId, role: 'user', content: trimmed, legs: [] },
      { id: assistantId, role: 'assistant', content: '', legs: [] },
    ]);

    // Set once the SDK chunk has loaded; a failed load has nothing to
    // classify with, and reads as a plain "couldn't answer".
    let runtime: ChatRuntime | null = null;
    try {
      runtime = await loadChatRuntime();
      const anthropic = new runtime.Anthropic({ apiKey, dangerouslyAllowBrowser: true });
      let assistantText = '';

      for await (const event of runtime.runChatTurn({
        anthropic,
        model: CHAT_MODEL,
        mcpUrl: mcpEndpointUrl(mcpServerUrl),
        mcpAuthProvider: provider,
        conversationHistory: historyRef.current,
        userMessage: trimmed,
        confirmToolCall,
      })) {
        if (event.type === 'text-delta') assistantText += event.text;
        applyChatEvent(event, assistantId, setMessages);
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
      setMessages((prev) => prev.filter((m) => m.id !== assistantId));
      const chatError: ChatError = runtime
        ? runtime.classifyChatError(err)
        : { kind: 'tool-error', message: err instanceof Error ? err.message : '' };
      // The transport already tried the SDK's own reauth (refresh, or
      // re-registration on `invalid_client`) before this surfaced, so the
      // stored tokens are dead: drop them so the next Send offers Connect
      // rather than replaying them into the same failure.
      if (chatError.kind === 'mcp-reconnect') provider.invalidateCredentials('tokens');
      setError(chatError);
    } finally {
      answerToolCall(false);
      setSending(false);
    }
  }

  return (
    <Stack gap="md" h="100%" flex={1} mih={0}>
      {shownError && (
        <ChatErrorAlert
          error={shownError}
          onSignIn={needsSignIn(shownError) ? signIn : undefined}
          signingIn={signingIn}
        />
      )}
      <ScrollArea viewportRef={viewport} flex={1} offsetScrollbars>
        <Stack gap="md" p="xs">
          {messages.length === 0 && (
            <Text c="dimmed" ta="center" mt="xl">
              Ask about live departures, disruptions, or plan a journey.
            </Text>
          )}
          {messages.map((message) => (
            <ChatMessageRow key={message.id} message={message} />
          ))}
        </Stack>
      </ScrollArea>
      {pendingTool && (
        <ToolConfirmation toolName={pendingTool.toolName} args={pendingTool.args} onAnswer={answerToolCall} />
      )}
      {/* LEG-16: always visible, so it is read before the first answer. */}
      <Text size="xs" c="dimmed" data-ai-note>
        {CHAT_AI_NOTE}
      </Text>
      <form onSubmit={handleSubmit}>
        <Group gap="xs" align="flex-end">
          <TextInput
            flex={1}
            placeholder="Ask about the next train or delays, or plan a journey"
            value={input}
            onChange={(event) => setInput(event.currentTarget.value)}
            // Fetch the SDK chunk while the question is typed, not on Send.
            onFocus={preloadChatRuntime}
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

function ChatErrorAlert({
  error,
  onSignIn,
  signingIn,
}: {
  error: ChatError;
  onSignIn?: (() => void) | undefined;
  signingIn: boolean;
}) {
  const signInButton = (label: string) =>
    onSignIn && (
      <Group mt="xs">
        <Button size="xs" onClick={onSignIn} loading={signingIn}>
          {label}
        </Button>
      </Group>
    );
  switch (error.kind) {
    case 'no-key':
      return (
        <Alert color="grape" variant="light">
          Add your Claude API key in{' '}
          <TextLink href={PREFERENCES_HREF} underline="always" inline>
            Preferences
          </TextLink>{' '}
          to start asking.
        </Alert>
      );
    case 'anthropic-rejected':
      return (
        <Alert color="red" variant="light">
          Your Claude API key was rejected. Check it in Preferences and try again.
        </Alert>
      );
    case 'mcp-connect':
      return (
        <Alert color="orange" variant="light">
          Connect to the rail data service to start asking about trains. You&apos;ll be asked to log in and approve
          access, then brought back here.
          {signInButton('Connect')}
        </Alert>
      );
    case 'mcp-reconnect':
      return (
        <Alert color="red" variant="light">
          Your connection to the rail data service has expired. Reconnect to keep chatting. You&apos;ll be asked to log
          in again.
          {signInButton('Reconnect')}
        </Alert>
      );
    case 'mcp-incomplete':
      return (
        <Alert color="orange" variant="light">
          Your last login to the rail data service didn&apos;t finish. Reconnect to try again.
          {signInButton('Reconnect')}
        </Alert>
      );
    case 'sign-in-failed':
      return (
        <Alert color="red" variant="light">
          Couldn&apos;t start logging in to the rail data service. Try again.
          {signInButton('Try again')}
        </Alert>
      );
    case 'tool-error':
      return (
        <Alert color="red" variant="light">
          Couldn&apos;t answer that. Try again.
        </Alert>
      );
  }
}

function applyChatEvent(
  event: ChatEvent,
  assistantId: number,
  setMessages: React.Dispatch<React.SetStateAction<ChatMessage[]>>,
) {
  if (event.type === 'text-delta') {
    setMessages((prev) => prev.map((m) => (m.id === assistantId ? { ...m, content: m.content + event.text } : m)));
    return;
  }
  if (event.type === 'tool-result') {
    const leg = asRenderedTrainLeg(event.structuredContent);
    if (!leg) return;
    setMessages((prev) => prev.map((m) => (m.id === assistantId ? { ...m, legs: [...m.legs, leg] } : m)));
    return;
  }
  // 'done' needs no state change -- the stream ending IS the signal.
}

/** DQ12 (FE-6): shown when the assistant wants a tool that isn't known to
 * be read-only. Nothing runs until the passenger chooses. */
function ToolConfirmation({
  toolName,
  args,
  onAnswer,
}: {
  toolName: string;
  args: Record<string, unknown>;
  onAnswer: (allowed: boolean) => void;
}) {
  return (
    <Alert color="orange" variant="light" title="Allow this action?" role="alertdialog" aria-label="Allow this action?">
      <Stack gap="xs">
        <Text size="sm">
          The assistant wants to use <Code>{toolName}</Code>. It isn&apos;t marked as read-only, so it might change
          something for you. Only allow it if you asked for this.
        </Text>
        <Code block>{JSON.stringify(args, null, 2)}</Code>
        <Group gap="xs">
          <Button size="xs" onClick={() => onAnswer(true)}>
            Allow
          </Button>
          <Button size="xs" variant="default" onClick={() => onAnswer(false)}>
            Don&apos;t allow
          </Button>
        </Group>
      </Stack>
    </Alert>
  );
}

function ChatMessageRow({ message }: { message: ChatMessage }) {
  const isUser = message.role === 'user';
  return (
    <Stack gap={4} align={isUser ? 'flex-end' : 'flex-start'}>
      {/* `grape.0`, not Mantine's default `blue.0` -- review §3.1.6: the
          grape-theme spec reserves blue for `planned` severity
          (`lib/severity.ts`'s `GROUP_COLOR`), the same reason
          `app/connect-claude/page.tsx`'s own informational `Alert` moved
          off blue. */}
      <Card withBorder padding="sm" radius="md" maw="80%" {...(isUser && { bg: 'grape.0' })}>
        {!isUser && (
          <Group gap={4} mb={4}>
            <AiGeneratedBadge note={CHAT_AI_NOTE} />
          </Group>
        )}
        {message.content || isUser ? (
          <Text className="ds-pre-wrap">{message.content}</Text>
        ) : (
          <LoadingText>Writing a reply</LoadingText>
        )}
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
          {originName} to {destinationName}
        </Text>
        <Text size="xs" c="dimmed">
          {leg.departure}
          {leg.operator ? ` · ${leg.operator}` : ''}
        </Text>
        {originCrs && (
          <ButtonLink href={`/track?origin=${encodeURIComponent(originCrs)}`} size="xs" variant="light">
            Track this train
          </ButtonLink>
        )}
      </Stack>
    </Card>
  );
}
