import { Stack, Text, Title } from '@mantine/core';
import { getChatbotAccess } from '@/lib/api';
import { AutoOpenLoginPrompt } from '@/app/track/mine/AutoOpenLoginPrompt';
import { LoginLink } from '@/components/LoginLink';
import { ChatPanel } from '@/components/ChatPanel';
import { AddMcpServerLinks } from '@/components/AddMcpServerLinks';
import { runtimeRailMcpPublicUrl } from '@/lib/csp';

// Same reasoning as app/page.tsx's own `revalidate = 0` (and
// track/mine/page.tsx's identical comment): no dynamic segment, so without
// this Next.js treats the route as eligible for static generation and
// tries to prerender it during `next build`, which fails since
// `getChatbotAccess()`'s backing `api` service only exists at runtime.
export const revalidate = 0;

/** `/chat` -- the embedded chat UI (embedded-chatbot-option-b plan, Task 5).
 * Gates on `getChatbotAccess()`'s three states before ever mounting
 * `ChatPanel`: `unauthenticated` reuses `AutoOpenLoginPrompt` (the same
 * modal-login-prompt convention `/track/mine` already established, not the
 * plain `LoginLink` this plan's own Task 5 sketch predates -- that page
 * confirmed obsolete this session); `forbidden` is a real, logged-in,
 * non-allowlisted user, per the dual-mode design's own Error handling
 * section ("a logged-in-but-not-allowlisted user... gets a plain 'not
 * available for your account' state, not a 404 -- the feature's existence
 * is not a secret"). */
export default async function ChatPage() {
  const access = await getChatbotAccess();

  if (access === 'unauthenticated') {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Chat</Title>
        {/* Server-rendered, same pattern as
            app/train/by-id/[trackingId]/page.tsx's own
            ApiUnauthorizedError branch: a link-unfurler bot or a
            pre-hydration visitor sees this sentence even though it can
            never run the client-only AutoOpenLoginPrompt modal below,
            which stays as progressive enhancement on top of it. Only this
            branch needs it -- the `forbidden` branch below already has
            real server-rendered content of its own, and the success
            branch is real content too. */}
        <LoginLink underline="always">Sign in to ask about live departures, disruptions and journeys</LoginLink>
        <AutoOpenLoginPrompt>Sign in to ask about live departures, disruptions and journeys.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  // FE-2: read at request time on the server and passed down as a prop.
  // A `process.env.NEXT_PUBLIC_*` read inside the Client Component would be
  // inlined at `next build`, where the image has no value for it.
  const mcpServerUrl = runtimeRailMcpPublicUrl();

  if (access === 'forbidden') {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Chat</Title>
        <Text c="dimmed">Not available for your account yet.</Text>
        {/* Review §3.1.2: this used to be a dead end for every logged-in,
            non-allowlisted visitor. The MCP server has its own access
            group, separate from this allowlist, so where it is configured
            its "add to your own assistant" section is a real next step.
            Where it isn't, there is nothing to connect to, so no link. */}
        {mcpServerUrl ? (
          <>
            <Text>
              This embedded chat is only available to a limited allowlist right now. If your account has access to the
              Distant Signal MCP server, you can ask your own assistant about live departures, disruptions and journeys
              instead — see below.
            </Text>
            <AddMcpServerLinks mcpPublicUrl={mcpServerUrl} />
          </>
        ) : (
          <Text>This embedded chat is only available to a limited allowlist right now.</Text>
        )}
      </Stack>
    );
  }

  if (!mcpServerUrl) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Chat</Title>
        <Text c="dimmed">Chat is not configured on this deployment.</Text>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md" h="100%">
      <Title order={1}>Chat</Title>
      <ChatPanel mcpServerUrl={mcpServerUrl} />
      <AddMcpServerLinks mcpPublicUrl={mcpServerUrl} />
    </Stack>
  );
}
