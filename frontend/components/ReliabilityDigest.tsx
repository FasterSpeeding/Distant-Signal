import { Alert, Card, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import {
  computeDelayRepayRollup,
  computePunctualitySummary,
  type DelayRepayRollup,
  type PunctualitySummary,
} from '@/lib/reliabilityDigest';
import { londonDayKey, formatDate } from '@/lib/dateFormat';
import type { TrackedTrainListItem, TicketListItem } from '@/lib/types';

/** New retrospective summary card for `/track/mine`, per
 * docs/superpowers/specs/2026-09-12-reliability-digest-design.md. Takes the
 * exact same two already-fetched arrays the rest of the page renders from
 * -- no fetch of its own, no new `Promise.all` member (Decision 5).
 * `today` is computed here, once per render, from `londonDayKey(new
 * Date())` -- this page already sets `revalidate = 0` for the same
 * "don't let this go stale mid-day" reason (see page.tsx's own comment on
 * that export). */
export function ReliabilityDigest({ trains, tickets }: { trains: TrackedTrainListItem[]; tickets: TicketListItem[] }) {
  const today = londonDayKey(new Date());
  const punctuality = computePunctualitySummary(trains, today);
  const rollup = computeDelayRepayRollup(tickets);

  return (
    <Card withBorder>
      <Stack gap="md">
        <Title order={2}>Your reliability</Title>
        <PunctualitySection summary={punctuality} />
        <DelayRepaySection rollup={rollup} />
      </Stack>
    </Card>
  );
}

function PunctualitySection({ summary }: { summary: PunctualitySummary }) {
  if (summary.eligibleCount === 0) {
    return (
      <Text size="sm" c="dimmed">
        Track a train and check back once it&apos;s finished running to see your punctuality here.
      </Text>
    );
  }

  return (
    <Stack gap={4}>
      <Text>
        Of your last {summary.eligibleCount} tracked journey{summary.eligibleCount === 1 ? '' : 's'} with a
        recorded outcome, {summary.onTimePct}% were on time
        {summary.avgDelayMinutes !== null && ` (average delay ${summary.avgDelayMinutes.toFixed(1)} minutes)`}.
        {summary.cancelledCount > 0 &&
          ` ${summary.cancelledCount} journey${summary.cancelledCount === 1 ? ' was' : 's were'} cancelled and ${summary.cancelledCount === 1 ? "isn't" : "aren't"} counted in that figure.`}
      </Text>
      {summary.worstJourneys.length > 0 && (
        <Stack gap={2}>
          <Text size="sm" fw={500}>
            Your most delayed tracked journeys:
          </Text>
          {summary.worstJourneys.map((journey) => {
            // Same "canonical link once resolved, by-id fallback
            // otherwise" href TrackedTrainListRow already computes on
            // this exact page (spec Decision 2/Architecture) -- ordinary
            // in-app navigation to a journey's own existing detail page,
            // not an outbound Delay Repay claim link (that guardrail is
            // specific to DelayRepaySection below).
            const href = journey.trainUid
              ? `/train/${journey.trainUid}/${journey.serviceDate}`
              : `/train/by-id/${journey.trainId}`;
            return (
              <Group key={journey.trainId} gap="xs">
                <Link href={href}>{formatDate(journey.serviceDate)}</Link>
                <Text size="sm" c="dimmed">
                  {journey.delayMinutes} minutes late
                </Text>
              </Group>
            );
          })}
        </Stack>
      )}
    </Stack>
  );
}

// Human-readable labels for the known bands this app's own
// `delay_repay_rules.rs` ever produces -- `dr15_band`/`dr30_band` only ever
// return 15/30/60 as `bandMinutes`, so this table is exhaustive against
// today's rules, not a guess. An unlisted key (a future new band) falls
// back to the raw key itself, so a rules change never disappears silently.
const BAND_LABELS: Record<string, string> = {
  'DR15-15': '15–29 minute delays (25% of fare)',
  'DR15-30': '30–59 minute delays (50% of fare)',
  'DR15-60': '60+ minute delays (100% of fare)',
  'DR30-30': '30–59 minute delays (50% of fare)',
  'DR30-60': '60+ minute delays (100% of fare)',
};

// The always-populated top-level disclaimer `delay_repay_rules::ROUTE_DISCLAIMER`
// produces, reproduced VERBATIM up to its hedge clause -- same
// "not a guarantee of compensation and not proof you travelled" wording
// `DelayRepayEstimate.tsx` renders (via `response.disclaimer`) for a
// single ticket. Unlike that component, this aggregate has no single
// `DelayRepayEstimateResponse` to read the string from (there is no
// aggregate API response at all -- Decision 5), so this is hardcoded
// rather than data-driven -- a known, explicitly flagged residual drift
// risk if `ROUTE_DISCLAIMER` is ever reworded (spec Open questions 1). The
// backend sentence's own tail ("...using the link above") is deliberately
// NOT reproduced here: this rollup renders no claim link at all (below),
// so that clause would describe a link that does not exist on screen.
const CARRIED_FORWARD_DISCLAIMER =
  "This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled.";

function DelayRepaySection({ rollup }: { rollup: DelayRepayRollup }) {
  if (rollup.attachedTicketsWithOperator === 0) {
    return (
      <Text size="sm" c="dimmed">
        Attach a ticket to one of your tracked trains to see whether any of your journeys may have qualified for
        Delay Repay.
      </Text>
    );
  }

  const bandEntries = Object.entries(rollup.bandCounts);

  return (
    <Stack gap={4} data-testid="delay-repay-rollup">
      <Alert color="blue" title="Possible Delay Repay eligibility, across your tracked journeys" variant="light">
        Of the {rollup.attachedTicketsWithOperator} tracked journey
        {rollup.attachedTicketsWithOperator === 1 ? '' : 's'} with a ticket attached, {rollup.eligibleCount} may
        have qualified for a partial or full refund of that journey&apos;s fare under the operator&apos;s Delay
        Repay scheme.
      </Alert>
      {bandEntries.length > 0 && (
        <Stack gap={2}>
          {bandEntries.map(([key, count]) => (
            <Text key={key} size="sm">
              {count} journey{count === 1 ? '' : 's'} — {BAND_LABELS[key] ?? key}
            </Text>
          ))}
        </Stack>
      )}
      <Text size="sm">
        {CARRIED_FORWARD_DISCLAIMER} This is a count of journeys, not a total amount: this app never stores ticket
        prices, so it has no fare figure to add up into a refund total, and never will. This app does not claim on
        your behalf for any of them — always verify eligibility and claim directly with each operator, using the
        link already shown against each ticket below.
      </Text>
    </Stack>
  );
}
