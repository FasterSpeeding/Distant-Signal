import { Stack, Title, Group } from '@mantine/core';
import { notFound, redirect } from 'next/navigation';
import { getTrackedTrainById, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { TrainJourneyPanel } from '@/components/TrainJourneyPanel';
import { TicketPanel } from '@/components/TicketPanel';
import { LoginLink } from '@/components/LoginLink';
import { TrackedTrainOwnerControls } from '@/components/TrackedTrainOwnerControls';

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

  // Once resolved to a real (trainUid, serviceDate), that pair -- not this
  // page's own trackingId -- is this train's canonical identity: hand off
  // to `/train/[uid]/[date]`, which now overlays the same owner controls
  // rendered locally below for an owner who tracks it, rather than keep
  // rendering a second, parallel copy of them here. `serviceDate` needs no
  // separate null-check -- unlike `trainUid`, it's always populated on
  // `TrackedTrainState`. Unresolved trains (no `trainUid` yet) fall through
  // to the local render below unchanged: there is no canonical URL to send
  // them to yet.
  if (state.resolutionStatus === 'resolved' && state.trainUid) {
    // encodeURIComponent, matching `getPublicTrainByUidAndDate` (lib/api.ts),
    // which encodes this identical (uid, date) pair before building its own
    // request URL -- without it, a uid/date containing a URL-unsafe
    // character (e.g. a space) would build a broken redirect target.
    redirect(`/train/${encodeURIComponent(state.trainUid)}/${encodeURIComponent(state.serviceDate)}`);
  }

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Tracking Train {trackingId}</Title>
        <Group gap="xs">
          <TrackedTrainOwnerControls train={state} />
        </Group>
      </Group>
      <TrainJourneyPanel state={state} />
      <TicketPanel trackingId={state.id} />
    </Stack>
  );
}
