import { Group, Stack, Text } from '@mantine/core';
import type { PredictedStart } from '@/lib/types';
import { StateBadge } from './StateBadge';
import { TextLink } from './TextLink';

/** The headline for a predicted late start (DS-127): a number at medium or
 * high confidence, words only at low. */
export function predictedStartHeadline(prediction: PredictedStart): string {
  if (prediction.confidence !== 'low' && prediction.lateMinutes !== null) {
    return `Expected to start about ${prediction.lateMinutes} min late`;
  }
  return 'May start late';
}

/** The incoming train's own page. */
function incomingTrainHref(prediction: PredictedStart): string {
  const { trainUid, serviceDate } = prediction.previousWorking;
  return `/train/${encodeURIComponent(trainUid)}/${encodeURIComponent(serviceDate)}`;
}

/** Why: the incoming train's delay, or, two links back, that it is due to
 * start late itself. */
function provenanceTail(prediction: PredictedStart): string {
  const delay = prediction.previousWorking.delayMinutes;
  if (delay !== null && delay > 0) return `, running ${delay} min late.`;
  return ', which is also expected to start late.';
}

/** A predicted late start for a train Network Rail has not reported yet
 * (`predictedStart`, docs/superpowers/specs/2026-10-21-pre-departure-prediction-design.md
 * section 8 and decisions Q1, Q5).
 *
 * - Medium or high confidence: an orange "Expected to start about 7 min
 *   late", with "Based on the incoming train, running 12 min late."
 * - Low: a yellow "May start late" and no number.
 *
 * Always an estimate, and said so: the badge reads "Estimate". "The
 * incoming train" links to that train's own page (decision Q5) rather than
 * naming it. Colour is never the only signal: the headline says it in
 * words. Renders nothing without a prediction. */
export function PredictedStartNotice({ prediction }: { prediction: PredictedStart | null | undefined }) {
  if (!prediction) return null;
  const confident = prediction.confidence !== 'low' && prediction.lateMinutes !== null;
  return (
    <Stack gap={4} data-predicted-start={prediction.confidence}>
      <Group gap="xs">
        <Text fw={500}>{predictedStartHeadline(prediction)}</Text>
        <StateBadge tone={confident ? 'late' : 'caution'}>Estimate</StateBadge>
      </Group>
      <Text size="sm" c="dimmed">
        Based on the <TextLink href={incomingTrainHref(prediction)}>incoming train</TextLink>
        {provenanceTail(prediction)}
      </Text>
    </Stack>
  );
}
