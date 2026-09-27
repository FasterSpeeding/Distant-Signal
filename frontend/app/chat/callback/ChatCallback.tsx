'use client';

import { useEffect, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { Alert, Button, Loader, Group, Stack, Text, Title } from '@mantine/core';
import { auth, type AuthResult } from '@modelcontextprotocol/sdk/client/auth.js';
import { BrowserMcpOAuthProvider } from '@/lib/mcpOAuthProvider';

/** Plain exclamation-in-a-circle, in the same inline-SVG house style as
 * `components/InfoIcon.tsx` (`@tabler/icons-react` is not a project
 * dependency -- see that file's own note). This is the icon half of the
 * error `Alert` below: WCAG 1.4.1 requires the error not be conveyed by
 * colour (red) alone. Decorative -- the accessible name for "this is an
 * error" comes from the Alert's own `role="alert"` and its text content,
 * not from this glyph. */
function ErrorIcon() {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="8" x2="12" y2="12" />
      <line x1="12" y1="16" x2="12.01" y2="16" />
    </svg>
  );
}

/** How long to wait for `auth()`'s authorization-code exchange before
 * giving up and showing an error -- there is otherwise no bound on how
 * long this page can sit on "Connecting…" if the exchange (a round trip
 * to `distant-signal-mcp`'s own token endpoint) hangs rather than
 * rejecting outright. 20s: generous for a same-request OAuth token
 * exchange, but short enough that a genuinely stuck connection doesn't
 * strand a visitor on a spinner indefinitely. This only bounds how long
 * the PAGE waits, not the underlying request -- a late resolution after
 * the timeout has already fired is simply ignored (see the `timedOut`
 * guard below), since `auth()` offers no cancellation hook to actually
 * abort it. */
const AUTH_TIMEOUT_MS = 20_000;

type Exchange =
  | { kind: 'error'; message: string }
  | { kind: 'pending'; promise: Promise<AuthResult> };

/** Validates the callback URL and starts the authorization-code exchange.
 * Runs once per mount (see `exchangeRef`). */
function startExchange(serverUrl: string): Exchange {
  const params = new URLSearchParams(window.location.search);
  const code = params.get('code');
  if (!code) {
    return { kind: 'error', message: 'No authorization code was present in the callback URL.' };
  }

  const provider = new BrowserMcpOAuthProvider(`${window.location.origin}/chat/callback`);

  // Finding 3 of the deferred fapp Low-severity batch (2026-09-24
  // security review): before this, nothing about this callback verified
  // an OAuth `state` parameter -- defense against a planted/replayed
  // authorization code rested entirely on the PKCE verifier mismatching
  // over in `auth()`'s own token-exchange call below. `provider.state()`
  // (called by `auth()` itself when it built the authorization redirect
  // that sent the browser here -- see `BrowserMcpOAuthProvider`) stored a
  // random single-use value before that redirect; verifying it here,
  // BEFORE ever calling `auth()` with the code, rejects a callback that
  // didn't actually originate from a redirect this browser itself
  // started, independent of and prior to the PKCE check.
  if (!provider.consumeAndVerifyState(params.get('state'))) {
    return {
      kind: 'error',
      message: 'The authorization response could not be verified. Please try connecting again from the Chat page.',
    };
  }

  return { kind: 'pending', promise: auth(provider, { serverUrl, authorizationCode: code }) };
}

type CallbackState =
  | { kind: 'connecting' }
  | { kind: 'success' }
  | { kind: 'error'; message: string };

/** `/chat/callback` -- the redirect target `distant-signal-mcp`'s own
 * `/authorize` -> `/connect-claude/authorize` consent bridge sends the
 * browser back to once the user approves (client-side-tokens design doc,
 * Decisions 1/3, Architecture step 3). Exchanges the `code` query param
 * for a bearer token via the MCP SDK's own `auth()` orchestrator
 * (`@modelcontextprotocol/sdk/client/auth.js`) -- the SAME function
 * `StreamableHTTPClientTransport` calls internally on a 401, reused here
 * directly for the one-time authorization-code exchange, driven by
 * `BrowserMcpOAuthProvider` (Task 7) so the resulting tokens land in
 * `localStorage` the same way either caller would leave them.
 *
 * Review §3.1.1: this used to keep "Connecting…" as the heading in every
 * branch, including a failed one, and rendered the raw exchange error
 * (`err.message` -- anything the MCP SDK or the token endpoint felt like
 * throwing) as the page's only explanation, with no next step. The
 * heading now tracks the actual `CallbackState` (connecting -> success ->
 * error), and an error renders one plain, non-technical sentence plus a
 * primary way back into the app, with the raw message demoted to a
 * collapsed `<details>` for anyone who wants it (e.g. to paste into a bug
 * report) rather than presented as the primary explanation.
 *
 * FE-2: `serverUrl` (the MCP server's public URL) comes from the server
 * wrapper in `page.tsx`, read at request time. It used to be a
 * `process.env.NEXT_PUBLIC_*` read here, which Next inlines at build time. */
export function ChatCallback({ serverUrl }: { serverUrl: string | undefined }) {
  const router = useRouter();
  const [state, setState] = useState<CallbackState>(() =>
    serverUrl
      ? { kind: 'connecting' }
      : { kind: 'error', message: 'The rail data service is not configured on this deployment.' },
  );

  // FE-9: the one-time exchange for this mount. `consumeAndVerifyState` is
  // single-use, so under React StrictMode's dev-only double effect run the
  // second run used to fail verification and show "could not be verified"
  // while the first run's exchange still completed and navigated. The ref
  // survives that re-run, so the second run attaches to the same exchange.
  const exchangeRef = useRef<Exchange | null>(null);

  useEffect(() => {
    if (!serverUrl) return; // initial state is already the error
    if (exchangeRef.current === null) {
      exchangeRef.current = startExchange(serverUrl);
    }
    const exchange = exchangeRef.current;
    if (exchange.kind === 'error') {
      setState({ kind: 'error', message: exchange.message });
      return;
    }

    // Guards both the timeout firing after a real result already landed
    // and a real result landing after the timeout already gave up --
    // whichever settles first wins, and the other is a no-op. `settled` is
    // also set by this run's cleanup, so a torn-down run never updates.
    let settled = false;

    const timeoutId = setTimeout(() => {
      if (settled) return;
      settled = true;
      setState({
        kind: 'error',
        message: 'Connecting to the rail data service timed out. Please try again.',
      });
    }, AUTH_TIMEOUT_MS);

    exchange.promise
      .then((result) => {
        if (settled) return;
        settled = true;
        clearTimeout(timeoutId);
        if (result === 'AUTHORIZED') {
          setState({ kind: 'success' });
          router.replace('/chat');
        } else {
          setState({
            kind: 'error',
            message: 'Authorization did not complete. Please try connecting again from the Chat page.',
          });
        }
      })
      .catch((err: unknown) => {
        if (settled) return;
        settled = true;
        clearTimeout(timeoutId);
        setState({
          kind: 'error',
          message: err instanceof Error ? err.message : 'Connecting to the rail data service failed.',
        });
      });

    return () => {
      settled = true;
      clearTimeout(timeoutId);
    };
    // Empty deps, deliberately: this effect processes the one-time OAuth
    // callback `code` exactly once per mount, and must never re-run just
    // because `router` (used only for the terminal `router.replace` below)
    // happens to be a new reference on some render -- re-running it would
    // mean re-exchanging an already-consumed authorization code. Next's own
    // `useRouter()` is a stable reference in real usage, but is not
    // guaranteed to be by every caller (this file's own tests, notably),
    // and this effect has no reason to depend on router identity anyway.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  if (state.kind === 'error') {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Couldn&apos;t connect</Title>
        {/* `role="alert"` + a non-colour icon: WCAG 1.4.1 -- this used to
            be conveyed by red text colour alone. */}
        <Alert color="red" icon={<ErrorIcon />} role="alert">
          <Stack gap="sm">
            <Text>We couldn&apos;t finish connecting to the rail data service.</Text>
            {/* `<Link>` wrapping a plain `Button`, not Mantine's
                `component={Link}` polymorphic prop -- the same pattern
                `components/ChatPanel.tsx`'s own "Track this train" button
                uses. Empirically, `component={Link}` here hung the test
                environment indefinitely (a real, reproducible hang, not
                mere slowness -- isolated by bisecting this file's tests
                down to this one render). */}
            <Link href="/chat" style={{ textDecoration: 'none' }}>
              <Button>Back to Chat</Button>
            </Link>
            <details>
              <summary>Show error details</summary>
              <Text size="sm" c="dimmed" style={{ whiteSpace: 'pre-wrap' }}>
                {state.message}
              </Text>
            </details>
          </Stack>
        </Alert>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>
        {state.kind === 'success' ? 'Connected, taking you to Chat…' : 'Connecting…'}
      </Title>
      <Group gap="sm">
        {state.kind === 'connecting' && <Loader size="sm" />}
        <Text c="dimmed">Finishing sign-in to the rail data service.</Text>
      </Group>
    </Stack>
  );
}
