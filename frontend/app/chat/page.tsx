import { Stack, Text, Title } from '@mantine/core';
import { getChatbotAccess } from '@/lib/api';
import { AutoOpenLoginPrompt } from '@/app/track/mine/AutoOpenLoginPrompt';
import { ChatPanel } from '@/components/ChatPanel';

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
        <AutoOpenLoginPrompt>
          Sign in to ask about live departures, disruptions and journeys.
        </AutoOpenLoginPrompt>
      </Stack>
    );
  }

  if (access === 'forbidden') {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Chat</Title>
        <Text c="dimmed">Not available for your account yet.</Text>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md" h="100%">
      <Title order={1}>Chat</Title>
      <ChatPanel />
    </Stack>
  );
}
