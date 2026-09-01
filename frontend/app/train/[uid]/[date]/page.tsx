import { Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getTrackedTrainByUidAndDate, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { TrainJourney } from '@/components/TrainJourney';
import { TicketPanel } from '@/components/TicketPanel';
import { TextLink } from '@/components/TextLink';

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export default async function TrackedTrainByUidPage({
  params,
}: {
  params: Promise<{ uid: string; date: string }>;
}) {
  const { uid, date } = await params;

  // Validated before the fetch fires, per the same "malformed URL segment
  // 404s directly" rule as the by-id page (Task 8) --
  // docs/superpowers/specs/2026-08-29-train-tracking-frontend-design.md's
  // Error handling section.
  if (!DATE_PATTERN.test(date)) {
    notFound();
  }

  let state;
  try {
    state = await getTrackedTrainByUidAndDate(uid, date);
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
          <Title order={1}>Train {uid}</Title>
          <TextLink href="/api/auth/login" underline="always">
            Log in to view this tracked train
          </TextLink>
        </Stack>
      );
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Train {uid}</Title>
      <TrainJourney state={state} />
      <TicketPanel trackingId={state.id} />
    </Stack>
  );
}
