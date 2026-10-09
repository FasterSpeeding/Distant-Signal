import { Stack, Text, Title } from '@mantine/core';
import { getChatbotAccess } from '@/lib/api';
import { AutoOpenLoginPrompt } from '@/app/track/mine/AutoOpenLoginPrompt';
import { LoginLink } from '@/components/LoginLink';
import { ChatPanel } from '@/components/ChatPanel';
import { AddMcpServerLinks } from '@/components/AddMcpServerLinks';
import { runtimeRailMcpPublicUrl } from '@/lib/csp';
import type { Metadata } from 'next';
import { pageMetadata } from '@/lib/pageMetadata';

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
 * is not a secret"). `forbidden` only happens in the api's `group` mode;
 * with `CHATBOT_ACCESS=authenticated` every logged-in user is `allowed`. */
/** Says plainly what answers here: an AI model, on the visitor's own key. */
const CHAT_SUBLINE = 'Uses Claude with your own API key';

export const metadata: Metadata = pageMetadata(
  'Ask about trains',
  'Ask about UK trains, departures and disruption in plain English. Uses Claude with your own API key.',
);

export default async function ChatPage() {
  const access = await getChatbotAccess();

  if (access.status === 'unauthenticated') {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Ask about trains</Title>
        {/* Server-rendered, same pattern as
            app/train/by-id/[trackingId]/page.tsx's own
            ApiUnauthorizedError branch: a link-unfurler bot or a
            pre-hydration visitor sees this sentence even though it can
            never run the client-only AutoOpenLoginPrompt modal below,
            which stays as progressive enhancement on top of it. Only this
            branch needs it -- the `forbidden` branch below already has
            real server-rendered content of its own, and the success
            branch is real content too. */}
        <LoginLink underline="always">Log in to ask about live departures, disruptions and journeys</LoginLink>
        <AutoOpenLoginPrompt>Log in to ask about live departures, disruptions and journeys.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  // FE-2: read at request time on the server and passed down as a prop.
  // A `process.env.NEXT_PUBLIC_*` read inside the Client Component would be
  // inlined at `next build`, where the image has no value for it.
  const mcpServerUrl = runtimeRailMcpPublicUrl();

  if (access.status === 'forbidden') {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Ask about trains</Title>
        <Text c="dimmed">Not available for your account yet.</Text>
        {/* Review §3.1.2: this used to be a dead end for every logged-in,
            non-allowlisted visitor. The MCP server has its own access
            group, separate from this allowlist, so where it is configured
            its "add to your own assistant" section is a real next step.
            Where it isn't, there is nothing to connect to, so no link. */}
        {mcpServerUrl ? (
          <>
            <Text>
              Chat is open to a small group of accounts for now. If your account can use the Distant Signal MCP server,
              ask your own assistant instead, as below.
            </Text>
            <AddMcpServerLinks mcpPublicUrl={mcpServerUrl} />
          </>
        ) : (
          <Text>Chat is open to a small group of accounts for now.</Text>
        )}
      </Stack>
    );
  }

  if (!mcpServerUrl) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Ask about trains</Title>
        <Text c="dimmed">Chat isn&apos;t available on this site.</Text>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md" h="100%">
      <Stack gap={4}>
        <Title order={1}>Ask about trains</Title>
        <Text c="dimmed">{CHAT_SUBLINE}</Text>
      </Stack>
      <ChatPanel mcpServerUrl={mcpServerUrl} />
      {/* The MCP server's own access rule is flipped together with this
          api's (CHATBOT_ACCESS), so the "only accounts that have been
          given access" note follows the api's mode. */}
      <AddMcpServerLinks mcpPublicUrl={mcpServerUrl} accessRestricted={access.mode === 'group'} />
    </Stack>
  );
}
