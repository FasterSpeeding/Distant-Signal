'use client';

import { ActionIcon, Code, CopyButton, Group, Tooltip } from '@mantine/core';

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

/** The connector URL with a copy button, for /connect-claude.
 *
 * A client component because Mantine's `CopyButton` takes a render-prop
 * `children` function, and a function can't be passed from a Server
 * Component to a Client Component: rendering it inline in the (server)
 * /connect-claude page made the whole page a 500 in production whenever the
 * MCP URL was configured.
 *
 * Review §3.1.6: the URL is long enough (a full hostname plus path) that
 * selecting it precisely by hand is fiddly on a phone. The URL may break
 * anywhere: on one `nowrap` row with its label it ran off the side of a
 * phone-width screen. */
export function CopyConnectorUrl({ url }: { url: string }) {
  return (
    <Group gap="xs" wrap="nowrap" align="center">
      <Code style={{ wordBreak: 'break-all' }}>{url}</Code>
      <CopyButton value={url}>
        {({ copied, copy }) => (
          <Tooltip label={copied ? 'Copied' : 'Copy connector URL'} withArrow>
            <ActionIcon
              variant="subtle"
              color={copied ? 'teal' : 'gray'}
              onClick={copy}
              aria-label="Copy connector URL"
            >
              <CopyIcon />
            </ActionIcon>
          </Tooltip>
        )}
      </CopyButton>
    </Group>
  );
}
