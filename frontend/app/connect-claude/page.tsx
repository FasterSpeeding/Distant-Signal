import { Alert, Card, List, ListItem, Stack, Text, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { CopyConnectorUrl } from '@/components/CopyConnectorUrl';
import { InfoIcon } from '@/components/InfoIcon';
import { TextLink } from '@/components/TextLink';
import { runtimeRailMcpPublicUrl } from '@/lib/csp';
import { mcpEndpointUrl } from '@/lib/mcpInstallLinks';
import { pageMetadata } from '@/lib/pageMetadata';

// Read the environment per request, never at build time: the connector URL
// comes from the runtime env (see connectorUrl() below), and the image
// is built without it. Same as app/chat/callback/page.tsx.
export const dynamic = 'force-dynamic';

// Same metadata shape as app/account/page.tsx: the root layout's bare
// "Distant Signal" title otherwise names every tab this page is open in.
const METADATA_TITLE = 'Connect Claude';
const METADATA_DESCRIPTION =
  'Connect your Claude account to Distant Signal to ask about UK trains, departures and journeys.';
export const metadata: Metadata = pageMetadata(METADATA_TITLE, METADATA_DESCRIPTION);

/** The connector URL: the MCP endpoint, `{railMcp.publicUrl}/mcp`, built
 * by the same `mcpEndpointUrl` as /chat and ChatPanel. Not the bare
 * origin -- that has no route, and the server's OAuth resource check is an
 * exact match on the `/mcp` URL.
 * FE-2: read through `runtimeRailMcpPublicUrl()`, which looks the name up
 * via a variable. A literal `process.env.NEXT_PUBLIC_…` reference is
 * inlined by Next at `next build` -- in Server Components too -- and the
 * image is built without it. Blank in any deployment where railMcp isn't
 * enabled; this page still renders then, with a placeholder. */
function connectorUrl(): string {
  const publicUrl = runtimeRailMcpPublicUrl();
  return publicUrl ? mcpEndpointUrl(publicUrl) : '(not configured on this deployment)';
}

/** Two overlapping rectangles -- the conventional "copy" glyph, in the
 * same inline-SVG house style as `components/InfoIcon.tsx` (`@tabler/
 * icons-react` isn't a project dependency). Decorative: the accessible
 * name for the button it sits in comes from that button's own
 * `aria-label`, not from this glyph. */
/** Option C's instructional page (embedded-chatbot-shared-foundation-and-
 * option-c plan, Task 9; the dual-mode design's Decision 6): the connector
 * URL plus static instructions mirroring the documented Claude.ai flow.
 *
 * Since 2026-09-29 the MCP service is its own Authentik OIDC client, so
 * Claude's login goes straight from the MCP service to the single sign-on
 * page and back. Distant Signal's own consent bridge
 * (`/connect-claude/authorize`) is retired -- that path now only shows a
 * short "this moved" note (`authorize/page.tsx`). With no Distant Signal
 * session involved any more, this page no longer gates on one either: the
 * connector URL is public, and the MCP service itself enforces the login
 * and its access group. `maw={640}`: the same reading width as the other
 * single-column flow pages (/account, /account/deleted). */
export default function ConnectClaudePage() {
  const url = connectorUrl();
  return (
    <Stack p="lg" gap="md" maw={640}>
      <Title order={1}>Connect Claude to Distant Signal</Title>
      <Text>
        Distant Signal exposes an MCP server so you can ask Claude directly about UK train departures, arrivals, and
        delay-aware journey planning — inside Claude&apos;s own app, using your own Claude account. This does not use
        any of Distant Signal&apos;s own conversation features; Claude handles the whole conversation itself.
      </Text>
      {/* `grape` + `IconInfoCircle`-equivalent, not Mantine's default blue
          -- review §3.1.6: the grape-theme spec reserves blue for `planned`
          severity (`lib/severity.ts`'s `GROUP_COLOR`), and an unrelated
          informational Alert rendering in that same blue reads as if it
          were reporting a planned-closure-flavoured status. `InfoIcon` is
          this app's own house-style info glyph (`components/InfoIcon.tsx`
          -- `@tabler/icons-react` isn't a project dependency), the same
          one `ChatPanel.tsx`'s own blue-background fix below reaches for. */}
      <Alert color="grape" variant="light" icon={<InfoIcon />}>
        Connecting requires a Pro, Max, Team, or Enterprise Claude plan for full support (a free Claude.ai account gets
        one custom connector).
      </Alert>
      {/* A bordered, headed section, the same shape as /account's cards,
          so the steps read as the page's one task rather than as more
          body copy. */}
      <Card withBorder component="section" aria-labelledby="connect-steps-heading">
        <Stack gap="sm">
          <Title order={2} size="h3" id="connect-steps-heading">
            How to connect
          </Title>
          {/* Flat `ListItem` named export, not the `List.Item` dot-notation
              compound API -- this page is a Server Component and `List` carries
              a `"use client"` directive, so a dot-notation sub-component
              reached off its reference resolves to `undefined` once Next
              actually compiles the Server/Client boundary, 500ing the route
              with "Element type is invalid ... got: undefined". Same bug class
              already hit and fixed for Table (AllLinesTable.tsx) and Tabs
              (app/lines/[id]/history/page.tsx) -- confirmed live against a
              running dev server, not reproducible via
              jsdom/@testing-library/react (renderWithMantine renders
              everything as one ordinary client tree and never enforces this
              boundary). */}
          <List type="ordered">
            <ListItem>
              In Claude.ai or Claude Desktop, open <strong>Customize &gt; Connectors</strong>.
            </ListItem>
            <ListItem>
              Click the <strong>+</strong> button, then <strong>Add custom connector</strong>.
            </ListItem>
            <ListItem>
              {/* The URL gets its own line; see CopyConnectorUrl for why it's a
                  client component. */}
              <Text span>Enter this URL:</Text>
              <CopyConnectorUrl url={url} />
            </ListItem>
            <ListItem>
              Connect it when Claude asks. Claude sends you to the sign-in page, where you log in with your Distant
              Signal account — there is no separate confirmation step — and then finishes the connection itself.
            </ListItem>
          </List>
        </Stack>
      </Card>
      <Text size="sm" c="dimmed">
        Conversations happen entirely inside Claude&apos;s own interface, billed to your own Claude plan — Distant
        Signal never sees the conversation itself, only the specific train/line/journey lookups Claude asks it to run on
        your behalf.
      </Text>
      {/* The other assistants' steps live in one place, /chat's "Use
          Distant Signal in your own assistant" section, rather than being
          copied here. */}
      <Text size="sm">
        Using a different assistant, such as ChatGPT, Cursor, VS Code, Claude Code, Codex or Gemini CLI? See{' '}
        <TextLink href="/chat" underline="always" inline size="sm">
          the setup steps on the chat page
        </TextLink>{' '}
        (you’ll need to sign in).
      </Text>
    </Stack>
  );
}
