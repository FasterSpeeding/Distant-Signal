import { Alert, Code, CopyButton, Group, List, ListItem, Stack, Text, Title, ActionIcon, Tooltip } from '@mantine/core';
import { getSession } from '@/lib/api';
import { LoginButton } from '@/components/LoginButton';
import { InfoIcon } from '@/components/InfoIcon';

// This route has no dynamic segment, so without this Next.js treats it as
// eligible for static generation and tries to prerender it during `next
// build` -- same reasoning as app/page.tsx's own `revalidate = 0` comment
// (getSession() needs the `api` service, which only exists on the runtime
// network, not at build time).
export const revalidate = 0;

/** The MCP server's own public URL -- baked in at container-start-read time
 * via NEXT_PUBLIC_RAILMCP_PUBLIC_URL (must match railMcp.publicUrl /
 * ingress.railMcp.host from the chart -- charts/distant-signal/templates/
 * frontend-deployment.yaml). Read fresh inside the component body (not
 * hoisted to a module-level constant) so it's picked up per-request, the
 * same way lib/api.ts's own baseUrl() reads API_BASE_URL at request time
 * rather than at module-load time -- the NEXT_PUBLIC_ prefix does not force
 * a build-time bake for a read that only ever happens server-side. Blank in
 * any deployment where railMcp isn't enabled; this page still renders in
 * that case, just with a placeholder, since hiding the whole route behind a
 * feature flag is more chart-wiring than this thin a page needs. */
function railMcpPublicUrl(): string {
  return process.env.NEXT_PUBLIC_RAILMCP_PUBLIC_URL ?? '(not configured on this deployment)';
}

/** Two overlapping rectangles -- the conventional "copy" glyph, in the
 * same inline-SVG house style as `components/InfoIcon.tsx` (`@tabler/
 * icons-react` isn't a project dependency). Decorative: the accessible
 * name for the button it sits in comes from that button's own
 * `aria-label`, not from this glyph. */
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

/** Option C's own thin instructional route (embedded-chatbot-shared-
 * foundation-and-option-c plan, Task 9) -- distinct from
 * app/connect-claude/authorize/route.ts's OAuth protocol bridge (Task 6),
 * which this page's own step-by-step instructions eventually send a user
 * through. Per the dual-mode design's Decision 6: the connector URL plus
 * static instructions mirroring the documented Claude.ai flow, gated behind
 * DS's own login the same way any other authenticated route is -- a
 * logged-out visitor has no DS identity to connect to in the first place. */
export default async function ConnectClaudePage() {
  const session = await getSession();

  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Connect Claude to Distant Signal</Title>
        <Text>
          Log in to Distant Signal first, then come back here to connect your own Claude.ai or Claude Desktop
          account.
        </Text>
        {/* Review §2.16: a filled `Button`, not the underlined text link
            this used to be -- the anonymous visitor's one action on this
            page had noticeably less visual weight than the authenticated
            branch's own step-by-step instructions below suggest a "real"
            page should have. */}
        <LoginButton title="Log in — needs a Distant Signal account">Log in</LoginButton>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Connect Claude to Distant Signal</Title>
      <Text>
        Distant Signal exposes an MCP server so you can ask Claude directly about UK train departures, arrivals,
        and delay-aware journey planning — inside Claude&apos;s own app, using your own Claude account. This does
        not use any of Distant Signal&apos;s own conversation features; Claude handles the whole conversation
        itself.
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
        Connecting requires a Pro, Max, Team, or Enterprise Claude plan for full support (a free Claude.ai account
        gets one custom connector).
      </Alert>
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
          <Group gap="xs" wrap="nowrap">
            <Text span>Enter this URL:</Text>
            <Code>{railMcpPublicUrl()}</Code>
            {/* Review §3.1.6: the connector URL is long enough (a full
                hostname plus path) that selecting it precisely by hand is
                fiddly on a phone. `CopyButton` is Mantine's own render-prop
                for this -- it owns the copied/not-copied toggle state, this
                just supplies the icon and the accessible name. */}
            <CopyButton value={railMcpPublicUrl()}>
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
        </ListItem>
        <ListItem>
          Approve access when prompted — you&apos;ll be sent to Distant Signal&apos;s own login if you
          aren&apos;t already signed in here, then asked to confirm the connection.
        </ListItem>
      </List>
      <Text size="sm" c="dimmed">
        Conversations happen entirely inside Claude&apos;s own interface, billed to your own Claude plan —
        Distant Signal never sees the conversation itself, only the specific train/line/journey lookups Claude
        asks it to run on your behalf.
      </Text>
    </Stack>
  );
}
