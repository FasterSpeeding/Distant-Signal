import { Button, Card, Stack, Text, Title } from '@mantine/core';
import { SectionTitle } from '@/components/SectionTitle';
import type { Metadata } from 'next';
import { getSessionOrLoggedOut } from '@/lib/api';
import { LoginButton } from '@/components/LoginButton';
import { DeleteAccountButton } from '@/components/DeleteAccountButton';

// Session-dependent: never prerender (see app/page.tsx's own comment).
export const revalidate = 0;

const METADATA_TITLE = 'Account & data — Distant Signal';
const METADATA_DESCRIPTION =
  'Download a copy of the personal data Distant Signal holds about you, or delete your account and all of it.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

/** `/account` -- the visitor's data rights in one place (UK legal audit
 * LEG-4/LEG-5): download everything held about them as JSON
 * (`GET /api/account/export`, UK GDPR Arts. 15 and 20), and delete the
 * account (`DeleteAccountButton`, Art. 17). The retention sentence mirrors
 * the api's default `PAST_TRAVEL_RETENTION_DAYS` (548 days, 18 months)
 * and the backup retention (up to 14 days); see docs/personal-data-retention.md. */
export default async function AccountPage() {
  const session = await getSessionOrLoggedOut();

  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="md" maw={640}>
        <Title order={1}>Account &amp; data</Title>
        <Text>Log in to download your data or delete your account.</Text>
        <LoginButton title="Log in — needs a Distant Signal account">Log in</LoginButton>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="lg" maw={640}>
      <Title order={1}>Account &amp; data</Title>

      <Card withBorder component="section" aria-labelledby="download-heading">
        <Stack gap="sm" align="flex-start">
          <SectionTitle id="download-heading">Download my data</SectionTitle>
          <Text>
            A JSON file of everything Distant Signal holds about you: your account details, tracked trains, tickets,
            journeys, templates, pins, custom lines, groups, share links, notification subscriptions and sessions.
          </Text>
          <Button component="a" href="/api/account/export" download variant="light">
            Download my data
          </Button>
        </Stack>
      </Card>

      <Card withBorder component="section" aria-labelledby="retention-heading">
        <Stack gap="xs">
          <SectionTitle id="retention-heading">How long we keep it</SectionTitle>
          <Text size="sm">
            Tracked trains, tickets and journeys are deleted automatically 18 months after the day of travel. Journey
            templates, groups, pins and custom lines are kept until you delete them or your account.
          </Text>
          <Text size="sm">
            Database backups are encrypted, and anything deleted can stay in them for up to 14 days.
          </Text>
        </Stack>
      </Card>

      <Card withBorder component="section" aria-labelledby="delete-heading">
        <Stack gap="sm" align="flex-start">
          <SectionTitle id="delete-heading">Delete my account</SectionTitle>
          <Text>
            Deletes your account and all of the data above straight away. You&apos;ll be asked to confirm first.
          </Text>
          <DeleteAccountButton />
        </Stack>
      </Card>
    </Stack>
  );
}
