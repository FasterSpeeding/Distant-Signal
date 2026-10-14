'use client';

import { useEffect, useState } from 'react';
import { Alert, Button, Checkbox, Group, Loader, Stack, Text } from '@mantine/core';
import { ButtonLink } from './ButtonLink';
import { preselectedScopes, scopeSentence } from '@/lib/connectedApps';
import { describeFailure } from '@/lib/failure';
import type { ConnectAppConsent as ConsentData } from '@/lib/types';

type State =
  | { kind: 'loading' }
  | { kind: 'missing' }
  | { kind: 'failed'; message: string }
  | { kind: 'ready'; consent: ConsentData }
  | { kind: 'connected'; displayName: string };

/** The JSON `{error, message}` the Connected apps routes answer with. */
async function errorCode(response: Response): Promise<string | null> {
  try {
    const body = (await response.json()) as { error?: unknown };
    return typeof body.error === 'string' ? body.error : null;
  } catch {
    return null;
  }
}

/** The consent page's body (docs/connected-apps.md; delegated-access spec
 * §5.3): who is logged in, which app is asking, and what it would be able
 * to do, as boxes to tick, with Allow and Cancel.
 *
 * Loading the page asks the api for a single-use confirmation token bound
 * to this session and this app, valid for 10 minutes; Allow sends it back.
 * So a grant always comes from this rendered page and a deliberate click,
 * never a single blind request. The page cannot be framed
 * (`frame-ancestors 'none'` in every page's CSP, lib/csp.ts). A link may
 * preselect boxes with `?scopes=`, but the grant takes only what is ticked
 * when Allow is pressed. */
export function ConnectAppConsent({
  client,
  requestedScopes,
}: {
  client: string;
  requestedScopes?: string | undefined;
}) {
  const [state, setState] = useState<State>({ kind: 'loading' });
  const [scopes, setScopes] = useState<string[]>([]);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const controller = new AbortController();
    void (async () => {
      try {
        const response = await fetch(`/api/account/connected-apps/${encodeURIComponent(client)}/consent`, {
          cache: 'no-store',
          signal: controller.signal,
        });
        if (controller.signal.aborted) return;
        if (response.status === 404) {
          setState({ kind: 'missing' });
          return;
        }
        if (!response.ok) {
          setState({ kind: 'failed', message: describeFailure('load', 'this page', response.status) });
          return;
        }
        // Aborted after this point, `json()` rejects into the catch below.
        const consent = (await response.json()) as ConsentData;
        setScopes(preselectedScopes(requestedScopes, consent.client.scopes));
        setState({ kind: 'ready', consent });
      } catch {
        if (!controller.signal.aborted) setState({ kind: 'failed', message: describeFailure('load', 'this page') });
      }
    })();
    return () => {
      controller.abort();
    };
  }, [client, requestedScopes]);

  if (state.kind === 'loading') {
    return <Loader size="sm" aria-label="Loading" />;
  }
  if (state.kind === 'missing') {
    return (
      <Alert color="yellow" title="That app isn't available">
        There is no app with that name, or Distant Signal has turned it off for now.
      </Alert>
    );
  }
  if (state.kind === 'failed') {
    return (
      <Alert color="red" title="Couldn't load this page">
        {state.message}
      </Alert>
    );
  }
  if (state.kind === 'connected') {
    return (
      <Stack gap="sm" align="flex-start">
        <Alert color="green" title={`${state.displayName} is connected`}>
          You can go back to your chat. To disconnect it later, open Connected apps.
        </Alert>
        <ButtonLink href="/account/connected-apps" variant="light">
          Open Connected apps
        </ButtonLink>
      </Stack>
    );
  }

  const { consent } = state;
  const app = consent.client;
  const who = consent.name || consent.username;

  async function allow() {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/account/connected-apps/${encodeURIComponent(app.name)}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ scopes, confirmationToken: consent.confirmationToken }),
      });
      if (response.ok) {
        setState({ kind: 'connected', displayName: app.displayName });
        return;
      }
      const code = await errorCode(response);
      setError(
        code === 'confirmation_required'
          ? 'This page has expired. Reload it and try again.'
          : response.status === 401
            ? 'Your session has ended. Log in again, then reload this page.'
            : describeFailure('connect', 'the app', response.status),
      );
    } catch {
      setError(describeFailure('connect', 'the app'));
    } finally {
      setBusy(false);
    }
  }

  return (
    <Stack gap="md">
      <Text fw={600} size="lg">
        {app.displayName} would like to work with your journeys.
      </Text>
      {who && <Text>You are logged in to Distant Signal as {who}.</Text>}
      {consent.existingGrant && (
        <Text size="sm">It&apos;s already connected. Allowing again replaces what it can do.</Text>
      )}
      <Checkbox.Group label={`${app.displayName} will be able to:`} value={scopes} onChange={setScopes}>
        <Stack gap="xs" mt="xs">
          {app.scopes.map((scope) => (
            <Checkbox key={scope} value={scope} label={scopeSentence(scope)} />
          ))}
        </Stack>
      </Checkbox.Group>
      <Text size="sm" c="dimmed">
        It can&apos;t share journeys, change your groups or settings, or delete journeys you saved yourself. You can
        disconnect it at any time in Connected apps.
      </Text>
      {error && (
        <Alert color="red" title="Couldn't connect the app">
          {error}
        </Alert>
      )}
      <Group gap="sm">
        <Button onClick={() => void allow()} loading={busy} disabled={scopes.length === 0}>
          Allow
        </Button>
        <ButtonLink href="/account/connected-apps" variant="default">
          Cancel
        </ButtonLink>
      </Group>
    </Stack>
  );
}
