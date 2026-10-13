'use client';

import { useId } from 'react';
import { Button, Card, Group, List, ListItem, Stack, Text, Title } from '@mantine/core';
import { CopyableLink } from './CopyableLink';
import { TextLink } from './TextLink';
import {
  claudeCodeCommand,
  codexCommand,
  cursorInstallLink,
  geminiCommand,
  mcpEndpointUrl,
  vscodeInstallLink,
} from '@/lib/mcpInstallLinks';
import { SectionTitle } from './SectionTitle';

/** "Use Distant Signal in your own assistant": the MCP server URL plus the
 * documented way to add it to each assistant. Research and sources are in
 * docs/superpowers/specs/2026-09-29-chat-add-mcp-server-links.md. The
 * caller renders this only when `railMcp.publicUrl` is configured.
 *
 * `accessRestricted` (default `true`, the conservative wording) shows the
 * "only accounts that have been given access" note. `/chat` passes `false`
 * when the api reports `CHATBOT_ACCESS=authenticated`, which is flipped at
 * the same time as the MCP server's own ungating. */
export function AddMcpServerLinks({
  mcpPublicUrl,
  accessRestricted = true,
}: {
  mcpPublicUrl: string;
  accessRestricted?: boolean;
}) {
  const endpoint = mcpEndpointUrl(mcpPublicUrl);
  const headingId = useId();

  return (
    <Card withBorder component="section" aria-labelledby={headingId}>
      <Stack gap="sm">
        <SectionTitle id={headingId}>Use Distant Signal in your own assistant</SectionTitle>
        <Text size="sm">
          Add Distant Signal to an AI assistant you already use, then ask it about live departures, disruptions and
          journeys there. When the assistant first connects, you&apos;ll log in with your Distant Signal account.
        </Text>
        {accessRestricted && (
          <Text size="sm" c="dimmed">
            Only accounts that have been given access to the Distant Signal MCP server can connect. If yours
            hasn&apos;t, logging in will be refused.
          </Text>
        )}

        <CopyableLink monospace label="MCP server URL" value={endpoint} />

        <Stack gap="xs">
          <Title order={3} size="sm">
            One-click install
          </Title>
          <Group gap="xs">
            <Button component="a" href={cursorInstallLink(endpoint)} variant="light" size="xs">
              Add to Cursor
            </Button>
            <Button component="a" href={vscodeInstallLink(endpoint)} variant="light" size="xs">
              Add to VS Code
            </Button>
          </Group>
          <Text size="xs" c="dimmed">
            Opens the app if it&apos;s installed. It asks you to confirm, then to log in when you first use it.
          </Text>
        </Stack>

        <Stack gap="xs">
          <Title order={3} size="sm">
            Command line
          </Title>
          <CopyableLink
            monospace
            label="Claude Code command"
            value={claudeCodeCommand(endpoint)}
            description="Then run /mcp in Claude Code to log in."
          />
          <CopyableLink
            monospace
            label="Codex CLI command"
            value={codexCommand(endpoint)}
            description="Then run codex mcp login distant-signal to log in."
          />
          <CopyableLink
            monospace
            label="Gemini CLI command"
            value={geminiCommand(endpoint)}
            description="Then run /mcp auth distant-signal in Gemini CLI to log in."
          />
        </Stack>

        <Stack gap="xs">
          <Title order={3} size="sm">
            Claude.ai and Claude Desktop
          </Title>
          <List type="ordered" size="sm" spacing={4}>
            <ListItem>
              Open{' '}
              <TextLink href="https://claude.ai/customize/connectors" underline="always" inline size="sm">
                Customize &gt; Connectors
              </TextLink>
              .
            </ListItem>
            <ListItem>Choose + then Add custom connector.</ListItem>
            <ListItem>Paste the MCP server URL above, choose Add, then Connect and log in.</ListItem>
          </List>
          <Text size="xs" c="dimmed">
            A free Claude plan allows one custom connector.{' '}
            <TextLink href="/connect-claude" underline="always" inline size="xs">
              More about connecting Claude
            </TextLink>
          </Text>
        </Stack>

        <Stack gap="xs">
          <Title order={3} size="sm">
            ChatGPT
          </Title>
          <List type="ordered" size="sm" spacing={4}>
            <ListItem>In Settings &gt; Security and login, turn on Developer mode.</ListItem>
            <ListItem>Open Plugins, choose + and create an app with the MCP server URL above, using OAuth.</ListItem>
          </List>
          <Text size="xs" c="dimmed">
            Needs a Plus, Pro, Business, Enterprise or Education plan, on the web.
          </Text>
        </Stack>

        {/* Seen with non-Claude clients: they register one loopback
            redirect (say localhost) then sign in via another (127.0.0.1),
            and the exact-match redirect check refuses it. Plain
            <details>, as in NetworkTrendsResults.tsx: small and closed by
            default, since most people never hit it. */}
        <details>
          <summary>
            <Text span size="xs" c="dimmed">
              Logging in fails with &quot;unregistered redirect_uri&quot;?
            </Text>
          </summary>
          <Text size="xs" c="dimmed" mt={4}>
            Your assistant logged in from a different local address than the one it registered with (localhost instead
            of 127.0.0.1, or the other way round). Remove Distant Signal from the assistant, add it again, then log in.
          </Text>
        </details>
      </Stack>
    </Card>
  );
}
