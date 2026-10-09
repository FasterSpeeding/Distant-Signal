import { Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { TextLink } from '@/components/TextLink';

export const metadata: Metadata = {
  title: 'Connect Claude',
  robots: { index: false },
};

/** Where Distant Signal's own Claude consent bridge used to live. Retired
 * on 2026-09-29: the MCP service is now its own Authentik OIDC client and
 * sends Claude's login straight to the single sign-on page, so nothing
 * links here any more. A stale bookmark or an old in-flight Claude login
 * lands on this short note instead of a bare 404. */
export default function ConnectClaudeAuthorizeRetiredPage() {
  return (
    <Stack p="lg" gap="md" maw={640}>
      <Title order={1}>This connection link has expired</Title>
      <Text>
        Claude connections no longer go through this page. Start again from Claude: add or reconnect the Distant Signal
        connector there, and Claude will send you straight to the sign-in page.
      </Text>
      <TextLink href="/connect-claude" underline="always">
        How to connect Claude
      </TextLink>
    </Stack>
  );
}
