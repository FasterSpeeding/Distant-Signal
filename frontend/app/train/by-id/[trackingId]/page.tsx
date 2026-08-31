import { Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getTrackedTrainById, ApiNotFoundError } from '@/lib/api';
import { TrainJourney } from '@/components/TrainJourney';
import { TicketPanel } from '@/components/TicketPanel';
import { TextLink } from '@/components/TextLink';

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
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Tracking Train {trackingId}</Title>
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
