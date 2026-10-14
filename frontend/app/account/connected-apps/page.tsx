import { Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { ConnectedAppsSection } from '@/components/ConnectedAppsSection';
import { LoginControl } from '@/components/LoginControl';
import { TextLink } from '@/components/TextLink';
import { getSessionOrLoggedOut } from '@/lib/api';
import { pageMetadata } from '@/lib/pageMetadata';

// Session-dependent: never prerender (see app/page.tsx's own comment).
export const revalidate = 0;

export const metadata: Metadata = pageMetadata(
  'Connected apps',
  'See and disconnect the apps that can work with your Distant Signal journeys.',
);

/** `/account/connected-apps`: the apps the visitor connected (delegated
 * access, docs/connected-apps.md). The same `ConnectedAppsSection` belongs
 * on the Preferences page. */
export default async function ConnectedAppsPage() {
  const session = await getSessionOrLoggedOut();
  return (
    <Stack p="lg" gap="lg" maw={640}>
      <Title order={1}>Connected apps</Title>
      {session.authenticated ? (
        <>
          <ConnectedAppsSection showTitle={false} />
          <Text size="sm">
            <TextLink href="/account">Back to account and data</TextLink>
          </Text>
        </>
      ) : (
        <Stack gap="sm" align="flex-start">
          <Text>Log in to see the apps connected to your account.</Text>
          <LoginControl variant="button" title="Log in (needs a Distant Signal account)">
            Log in
          </LoginControl>
        </Stack>
      )}
    </Stack>
  );
}
