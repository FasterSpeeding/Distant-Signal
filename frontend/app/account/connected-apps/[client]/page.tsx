import { Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { ConnectAppConsent } from '@/components/ConnectAppConsent';
import { LoginControl } from '@/components/LoginControl';
import { getSessionOrLoggedOut } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';

// Session-dependent: never prerender (see app/page.tsx's own comment).
export const revalidate = 0;

export const metadata: Metadata = pageMetadata(
  'Connect an app',
  'Let an app work with your Distant Signal journeys, and choose what it may do.',
);

/** `/account/connected-apps/[client]`: the consent page an app links to
 * (delegated-access spec §5.1, §5.3). Logging in first creates the
 * Distant Signal account if this is the visitor's first visit. */
export default async function ConnectAppPage({
  params,
  searchParams,
}: {
  params: Promise<{ client: string }>;
  searchParams: Promise<{ scopes?: string | string[] }>;
}) {
  const [{ client }, query, session] = await Promise.all([params, searchParams, getSessionOrLoggedOut()]);
  const requestedScopes = Array.isArray(query.scopes) ? query.scopes.join(' ') : query.scopes;

  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="md" maw={640}>
        <Title order={1}>Connect an app</Title>
        <Text>Log in to Distant Signal first. You&apos;ll come back here to choose what the app may do.</Text>
        <LoginControl variant="button" title="Log in (needs a Distant Signal account)">
          Log in
        </LoginControl>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md" maw={640}>
      <Title order={1}>Connect an app</Title>
      <ConnectAppConsent client={client} requestedScopes={requestedScopes} />
    </Stack>
  );
}
