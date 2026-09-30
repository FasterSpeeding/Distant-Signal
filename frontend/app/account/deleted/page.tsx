import { Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { TextLink } from '@/components/TextLink';

export const metadata: Metadata = {
  title: 'Account deleted — Distant Signal',
  robots: { index: false },
};

/** Where `DeleteAccountButton` lands after a successful deletion. Static:
 * the session is gone by now, so there is nothing to look up. */
export default function AccountDeletedPage() {
  return (
    <Stack p="lg" gap="md" maw={640}>
      <Title order={1}>Your account has been deleted</Title>
      <Text>
        Your Distant Signal account and all of its data have been deleted, and you have been logged out everywhere.
        Encrypted database backups can keep your data for up to 14 days, and then it is gone from them too.
      </Text>
      <Text>
        Your single sign-on account (or Discord login) is separate and still exists. Close it there if you want it gone
        too.
      </Text>
      <TextLink href="/">Back to the home page</TextLink>
    </Stack>
  );
}
