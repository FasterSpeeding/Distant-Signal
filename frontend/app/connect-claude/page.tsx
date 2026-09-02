import { Alert, Code, List, Stack, Text, Title } from '@mantine/core';
import { getSession } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';

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
        <LoginLink underline="always">Log in</LoginLink>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Connect Claude to Distant Signal</Title>
      <Text>
        Distant Signal exposes an MCP server so you can ask Claude directly about UK train departures, arrivals,
        and delay-aware journey planning -- inside Claude&apos;s own app, using your own Claude account. This does
        not use any of Distant Signal&apos;s own conversation features; Claude handles the whole conversation
        itself.
      </Text>
      <Alert color="blue">
        Connecting requires a Pro, Max, Team, or Enterprise Claude plan for full support (a free Claude.ai account
        gets one custom connector).
      </Alert>
      <List type="ordered">
        <List.Item>
          In Claude.ai or Claude Desktop, open <strong>Customize &gt; Connectors</strong>.
        </List.Item>
        <List.Item>
          Click <strong>+</strong>, then <strong>Add custom connector</strong>.
        </List.Item>
        <List.Item>
          Enter this URL: <Code>{railMcpPublicUrl()}</Code>
        </List.Item>
        <List.Item>
          Approve access when prompted -- you&apos;ll be sent to Distant Signal&apos;s own login if you
          aren&apos;t already signed in here, then asked to confirm the connection.
        </List.Item>
      </List>
      <Text size="sm" c="dimmed">
        Conversations happen entirely inside Claude&apos;s own interface, billed to your own Claude plan --
        Distant Signal never sees the conversation itself, only the specific train/line/journey lookups Claude
        asks it to run on your behalf.
      </Text>
    </Stack>
  );
}
