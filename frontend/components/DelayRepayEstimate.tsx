import { Alert, Stack, Text } from '@mantine/core';
import { TextLink } from './TextLink';
import { isTimetableOnly, serviceModeLabel } from '@/lib/serviceMode';
import type { DelayRepayEstimateResponse } from '@/lib/types';

/** Renders one ticket's Delay Repay estimate, per
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md
 * Decision 3. Pure presentational -- takes an already-fetched response, no
 * fetch of its own (the per-ticket fetch lives in `TicketPanel`).
 *
 * Deliberately does NOT render `response.disclaimer` (the TOP-LEVEL field,
 * always populated regardless of `estimate`) verbatim any more -- Task
 * 3.6.11 found the same ~120-word disclaimer repeated up to three times
 * across `/track/mine` (once per attached ticket, via this component,
 * PLUS the aggregate rollup) and one more time on the train detail page,
 * which reads as noise rather than caution. The one FULL disclaimer now
 * lives once, at the card level, in `ReliabilityDigest.tsx`'s
 * `DelayRepaySection` (`CARRIED_FORWARD_DISCLAIMER`); every per-ticket
 * instance -- this component -- keeps only the short anti-CTA reminder
 * ("this app never submits a claim on your behalf") next to its own claim
 * link, since that specific caveat is the one fact that varies per link
 * and must stay attached to it. `estimate.disclaimer` (present only when
 * `estimate` is non-null, a textually DIFFERENT string from the top-level
 * one) is still deliberately never rendered here -- two near-duplicate
 * caveats at once would read as inconsistent, not doubly cautious.
 * `claimUrl` is always rendered as a real outbound link, labelled to
 * describe leaving this app -- never phrasing that could read as this app
 * performing a claim itself. */
/** LEG-14: when the Delay Repay rules the api applies were last checked
 * against each operator's own page -- the "as of 2026-08-29" date in
 * `crates/api/src/data/delay_repay_rules.rs`. Update both together. */
export const DELAY_REPAY_RULES_CHECKED_ON = '29 August 2026';

/** Shown for a bus or ferry when the backend sends no `unmeasurableReason`
 * of its own. */
const TIMETABLE_ONLY_DELAY_REPAY_FALLBACK =
  "buses and ferries aren't tracked live, so we can't measure a delay on this leg. If it ran late, claim with the operator using the times you recorded.";

export function DelayRepayEstimate({ response }: { response: DelayRepayEstimateResponse }) {
  return (
    <Stack gap={4}>
      <EstimateSummary response={response} />
      {/* LEG-14: say how current the rules are and where the delay figure
          comes from. */}
      <Text size="xs" c="dimmed">
        Rules last checked: {DELAY_REPAY_RULES_CHECKED_ON}. Delays are measured against the public timetable arrival at
        your destination, from public running data, and may differ from the operator&apos;s own records.
      </Text>
      <Text size="sm">This app never submits a claim on your behalf.</Text>
      {/* The only place in this feature that opens a new tab -- every
          other action stays same-page. */}
      <TextLink href={response.claimUrl} underline="always" external>
        See how to claim from the operator
      </TextLink>
    </Stack>
  );
}

/** Before the train reaches the ticket's destination the delay (and any
 * band) is a projection from its current delay to the public timetable
 * arrival there, and says so; it becomes final once the train arrives
 * (design doc §9 decision 3). */
function ProvisionalNote({ response }: { response: DelayRepayEstimateResponse }) {
  if (!response.provisional || response.delayMinutes === null) return null;
  const where = response.measuredAtCrs ?? 'your destination';
  return (
    <Text size="sm" fw={500}>
      Provisional: the train hasn&apos;t reached {where} yet, so this uses its current delay projected to the timetabled
      arrival there. It will change, and becomes final once the train arrives.
    </Text>
  );
}

function EstimateSummary({ response }: { response: DelayRepayEstimateResponse }) {
  const { estimate, delayMinutes } = response;

  // A bus or ferry is never reported live, so there is no delay to measure
  // -- say that, rather than "no delay data recorded yet", which implies
  // some may arrive.
  if (isTimetableOnly(response)) {
    const what = serviceModeLabel(response.serviceMode) ?? 'This service';
    return (
      <Text size="sm">
        {what}: {response.unmeasurableReason ?? TIMETABLE_ONLY_DELAY_REPAY_FALLBACK}
      </Text>
    );
  }

  if (estimate) {
    const fare = estimate.fareBasis === 'return' ? 'your return fare' : 'your fare';
    const title = estimate.provisional ? 'Provisional Delay Repay estimate' : 'Estimated Delay Repay eligibility';
    return (
      <>
        <Alert color="grape" title={title} variant="light">
          Estimated compensation: {estimate.percentage}% of {fare} ({estimate.scheme}, {estimate.bandMinutes}+ minute
          delay). This is an estimate, not a guarantee.
        </Alert>
        <ProvisionalNote response={response} />
      </>
    );
  }

  if (delayMinutes !== null) {
    // Deliberate: the API gives no way to distinguish "you're genuinely
    // under threshold" from "we don't recognize this operator's scheme"
    // from "some other reason didn't clear a band" -- this copy must not
    // assert a specific one of the three the response doesn't support.
    return (
      <>
        <Text size="sm">
          Based on the recorded delay ({delayMinutes} minutes), this operator&apos;s Delay Repay rules may not give a
          payout at that length — but rules vary and this estimate can be wrong, so it&apos;s still worth checking
          directly.
        </Text>
        <ProvisionalNote response={response} />
      </>
    );
  }

  return (
    <Text size="sm">
      No delay data recorded yet for this journey — if you already know you were delayed, the link below still goes
      straight to the operator.
    </Text>
  );
}
