import { Stack, Title, Group } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getTrackedTrainById, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { TrainJourney } from '@/components/TrainJourney';
import { TicketPanel } from '@/components/TicketPanel';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { DeleteTrainButton } from '@/components/DeleteTrainButton';
import { RenameTrainButton } from '@/components/RenameTrainButton';
import { trackedTrainDisplayName } from '@/lib/trackingName';

export default async function TrackedTrainByIdPage({
  params,
}: {
  params: Promise<{ trackingId: string }>;
}) {
  const { trackingId } = await params;

  // Validated before the fetch fires, per
  // docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md's
  // Error handling section -- a malformed segment 404s directly rather
  // than reaching the backend and relying on its error shape.
  if (!/^\d+$/.test(trackingId)) {
    notFound();
  }

  let state;
  try {
    state = await getTrackedTrainById(Number(trackingId));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    // Distinct from the custom-line detail page's 401-collapses-into-404
    // choice (see frontend/app/lines/[id]/page.tsx and its own comment) --
    // this page has no public sibling content to fall back to, so a
    // dedicated "log in, this might be yours" prompt is more honest than a
    // bare 404 for a real owner whose session lapsed.
    if (err instanceof ApiUnauthorizedError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Tracking Train {trackingId}</Title>
          <LoginLink underline="always">
            Log in to view this tracked train
          </LoginLink>
        </Stack>
      );
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Tracking Train {trackingId}</Title>
        <Group gap="xs">
          <RenameTrainButton
            trackingId={state.id}
            customName={state.customName}
            defaultName={trackedTrainDisplayName(state)}
          />
          <DeleteTrainButton trackingId={state.id} />
        </Group>
      </Group>
      <TrainJourney state={state} />
      <TicketPanel trackingId={state.id} />
      {/* A same-page nudge, not an automatic redirect -- Decision 2's
          explicit reasoning: a redirect would silently break "I
          bookmarked the URL right after tracking, before it resolved"
          for a user who didn't want to wait. */}
      {state.resolutionStatus === 'resolved' && state.trainUid && (
        <TextLink href={`/train/${state.trainUid}/${state.serviceDate}`} underline="always">
          View the canonical link for this train
        </TextLink>
      )}
    </Stack>
  );
}
