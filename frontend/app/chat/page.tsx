import { Stack, Text, Title } from '@mantine/core';
import { getSessionOrLoggedOut } from '@/lib/api';
import { AutoOpenLoginPrompt } from '@/app/track/mine/AutoOpenLoginPrompt';
import { LoginControl } from '@/components/LoginControl';
import { ChatPanel } from '@/components/ChatPanel';
import { AddMcpServerLinks } from '@/components/AddMcpServerLinks';
import { runtimeRailMcpPublicUrl } from '@/lib/csp';
import type { Metadata } from 'next';
import { pageMetadata } from '@/lib/pageMetadata';

// Same reasoning as app/page.tsx's own `revalidate = 0` (and
// track/mine/page.tsx's identical comment): no dynamic segment, so without
// this Next.js treats the route as eligible for static generation and
// tries to prerender it during `next build`, which fails since
// `getSessionOrLoggedOut()`'s backing `api` service only exists at runtime.
export const revalidate = 0;

/** `/chat` -- the embedded chat UI (embedded-chatbot-option-b plan, Task 5).
 * Open to every logged-in user (since 2026-10-14; it used to be limited to
 * an SSO group). Each user brings their own Anthropic key, held in their
 * own browser, so the chat costs Distant Signal no model spend. A logged-out
 * visitor gets `AutoOpenLoginPrompt` (the same modal-login-prompt convention
 * `/track/mine` uses). A failed session lookup renders as logged out, like
 * the nav (`getSessionOrLoggedOut()` logs it). */
/** Says plainly what answers here: an AI model, on the visitor's own key. */
const CHAT_SUBLINE = 'Uses Claude with your own API key';

export const metadata: Metadata = pageMetadata(
  'Ask about trains',
  'Ask about UK trains, departures and disruption in plain English. Uses Claude with your own API key.',
);

export default async function ChatPage() {
  const session = await getSessionOrLoggedOut();

  if (!session.authenticated) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Ask about trains</Title>
        {/* Server-rendered, same pattern as
            app/train/by-id/[trackingId]/page.tsx's own
            ApiUnauthorizedError branch: a link-unfurler bot or a
            pre-hydration visitor sees this sentence even though it can
            never run the client-only AutoOpenLoginPrompt modal below,
            which stays as progressive enhancement on top of it. */}
        <LoginControl underline="always">Log in to ask about live departures, disruptions and journeys</LoginControl>
        <AutoOpenLoginPrompt>Log in to ask about live departures, disruptions and journeys.</AutoOpenLoginPrompt>
      </Stack>
    );
  }

  // FE-2: read at request time on the server and passed down as a prop.
  // A `process.env.NEXT_PUBLIC_*` read inside the Client Component would be
  // inlined at `next build`, where the image has no value for it.
  const mcpServerUrl = runtimeRailMcpPublicUrl();

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
      <AddMcpServerLinks mcpPublicUrl={mcpServerUrl} />
    </Stack>
  );
}
