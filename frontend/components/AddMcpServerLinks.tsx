'use client';

import { useEffect, useId, useRef, useState } from 'react';
import { ActionIcon, Button, Card, Group, List, ListItem, Stack, Text, TextInput, Title, Tooltip } from '@mantine/core';
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

const COPIED_RESET_MS = 2000;

/** Two overlapping rectangles, the conventional "copy" glyph. Inline SVG in
 * the house style of `InfoIcon.tsx`; decorative, since the button carries
 * its own `aria-label`. */
function CopyIcon() {
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
      <rect x="9" y="9" width="13" height="13" rx="2" />
      <path d="M5 15H4a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h9a2 2 0 0 1 2 2v1" />
    </svg>
  );
}

function CheckIcon() {
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
      <polyline points="20 6 9 17 4 12" />
    </svg>
  );
}

type CopyState = 'idle' | 'copied' | 'failed';

/** A read-only, labelled field with a copy button. The result is shown in
 * a `role="status"` line under the field, so it is both visible and
 * announced; the tooltip alone would only reach mouse users. */
function CopyField({ label, value, description }: { label: string; value: string; description?: React.ReactNode }) {
  const [state, setState] = useState<CopyState>('idle');
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(
    () => () => {
      if (timer.current) clearTimeout(timer.current);
    },
    [],
  );

  async function copy() {
    if (timer.current) clearTimeout(timer.current);
    try {
      // `navigator.clipboard` is missing outside a secure context; the
      // throw lands in the catch below like any other failure.
      await navigator.clipboard.writeText(value);
      setState('copied');
      timer.current = setTimeout(() => setState('idle'), COPIED_RESET_MS);
    } catch {
      setState('failed');
    }
  }

  const copied = state === 'copied';
  return (
    <Stack gap={4} data-copy-field>
      <TextInput
        label={label}
        description={description}
        value={value}
        readOnly
        onFocus={(event) => event.currentTarget.select()}
        styles={{ input: { fontFamily: 'var(--mantine-font-family-monospace)' } }}
        rightSectionPointerEvents="all"
        rightSection={
          <Tooltip label={copied ? 'Copied' : `Copy ${label}`} withArrow>
            <ActionIcon variant="subtle" color="gray" onClick={copy} aria-label={`Copy ${label}`}>
              {copied ? <CheckIcon /> : <CopyIcon />}
            </ActionIcon>
          </Tooltip>
        }
      />
      <Text role="status" aria-live="polite" size="xs" c={state === 'failed' ? 'var(--ds-color-error-text)' : 'dimmed'}>
        {state === 'copied' && 'Copied.'}
        {state === 'failed' && 'Couldn’t copy. Select the text and copy it yourself.'}
      </Text>
    </Stack>
  );
}

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
          journeys there. When the assistant first connects, you’ll sign in with your Distant Signal account.
        </Text>
        {accessRestricted && (
          <Text size="sm" c="dimmed">
            Only accounts that have been given access to the Distant Signal MCP server can connect. If yours hasn’t,
            logging in will be refused.
          </Text>
        )}

        <CopyField label="MCP server URL" value={endpoint} />

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
            Opens the app if it’s installed. It asks you to confirm, then to log in when you first use it.
          </Text>
        </Stack>

        <Stack gap="xs">
          <Title order={3} size="sm">
            Command line
          </Title>
          <CopyField
            label="Claude Code command"
            value={claudeCodeCommand(endpoint)}
            description="Then run /mcp in Claude Code to log in."
          />
          <CopyField
            label="Codex CLI command"
            value={codexCommand(endpoint)}
            description="Then run codex mcp login distant-signal to log in."
          />
          <CopyField
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
              Logging in fails with “unregistered redirect_uri”?
            </Text>
          </summary>
          <Text size="xs" c="dimmed" mt={4}>
            Your assistant logged in from a different local address than the one it registered with (localhost instead
            of 127.0.0.1, or the other way round). Remove Distant Signal from the assistant, add it again, then sign in.
          </Text>
        </details>
      </Stack>
    </Card>
  );
}
