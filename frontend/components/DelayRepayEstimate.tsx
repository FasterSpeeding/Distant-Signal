import { Alert, Stack, Text } from '@mantine/core';
import { TextLink } from './TextLink';
import { isTimetableOnly, serviceModeLabel } from '@/lib/serviceMode';
import type { DelayRepayEstimate as Estimate, DelayRepayEstimateResponse } from '@/lib/types';
import { formatDate } from '@/lib/dateFormat';

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
 * performing a claim itself.
 *
 * The states (2026-10-07 decisions, docs/superpowers/specs/2026-10-01-working-vs-public-times-design.md):
 * - a bus or ferry leg: never measured;
 * - `outcome: notReached`: the train didn't reach the destination; no
 *   percentage, eligibility depends on the replacement journey;
 * - `ownScheme`: the operator runs its own scheme; no percentage;
 * - a `borderline` provisional estimate: no percentage, the threshold named;
 * - an estimate: the band, worded by fare and ticket type;
 * - a delay under the lowest band, or no delay yet.
 * The summary sits in a polite live region, so a screen reader hears a
 * change of state (provisional to final, say) when the response refreshes. */

/** LEG-14: when the Delay Repay rules the api applies were last checked
 * against their sources -- `RULES_CHECKED_ON` in
 * `crates/api/src/data/delay_repay_rules.rs`, served as `rulesCheckedOn`.
 * The fallback for an older backend that doesn't send it. */
export const DELAY_REPAY_RULES_CHECKED_ON = '7 Oct 2026';

/** Shown for a bus or ferry when the backend sends no `unmeasurableReason`
 * of its own. */
const TIMETABLE_ONLY_DELAY_REPAY_FALLBACK =
  "buses and ferries aren't tracked live, so we can't measure a delay on this leg. If it ran late, claim with the operator using the times you recorded.";

/** `2026-10-07` as `7 Oct 2026` (lib/dateFormat.ts); anything else verbatim. */
function formatCheckedOn(value: string | undefined): string {
  if (!value || !/^\d{4}-\d{2}-\d{2}$/.test(value)) return value ?? DELAY_REPAY_RULES_CHECKED_ON;
  return formatDate(`${value}T12:00:00Z`);
}

export function DelayRepayEstimate({ response }: { response: DelayRepayEstimateResponse }) {
  return (
    <Stack gap={4}>
      <div aria-live="polite">
        <Stack gap={4}>
          <EstimateSummary response={response} />
        </Stack>
      </div>
      {/* LEG-14: say how current the rules are and where the delay figure
          comes from. */}
      <Text size="xs" c="dimmed">
        Rules last checked: {formatCheckedOn(response.rulesCheckedOn)}. Delays are measured against the public timetable
        arrival at your destination, from public running data, and may differ from the operator&apos;s own records.
      </Text>
      <Text size="sm">Distant Signal never claims on your behalf.</Text>
      {/* The only place in this feature that opens a new tab -- every
          other action stays same-page. */}
      <TextLink href={response.claimUrl} underline="always" external>
        See how to claim from the operator
      </TextLink>
    </Stack>
  );
}

/** The destination, by name when known. */
function destinationLabel(response: DelayRepayEstimateResponse): string {
  return response.measuredAtName ?? response.measuredAtCrs ?? 'your destination';
}

/** Before the train reaches the ticket's destination the delay (and any
 * band) is a projection from its current delay to the public timetable
 * arrival there, and says so; it becomes final once the train arrives
 * (design doc §9 decision 3). A departure-only station is final on the
 * train's departure from it, and says that instead. */
function ProvisionalNote({ response }: { response: DelayRepayEstimateResponse }) {
  if (response.delayMinutes === null) return null;
  if (response.outcome === 'departedOnly') {
    return (
      <Text size="sm">
        Final, based on its departure from {destinationLabel(response)}: that station reports departures only.
      </Text>
    );
  }
  if (!response.provisional) return null;
  return (
    <Text size="sm" fw={500}>
      Provisional: the train hasn&apos;t reached {destinationLabel(response)} yet, so this uses its current delay
      projected to the timetabled arrival there. It will change, and becomes final once the train arrives.
    </Text>
  );
}

/** What the band pays, in words (a non-borderline estimate). */
export function compensationText(estimate: Estimate): string {
  const pct = estimate.percentage ?? 0;
  let text: string;
  if (estimate.scheme === 'HX') {
    text = `${pct}% of your ticket price`;
  } else if (estimate.bandMinutes >= 120) {
    if (estimate.ticketKind === 'single' || (estimate.ticketKind === undefined && estimate.fareBasis === 'single')) {
      text = `${pct}% of your single fare: singles are already refunded in full from 60 minutes`;
    } else if (estimate.ticketKind === 'return') {
      text = `${pct}% of your return fare`;
    } else {
      text = `${pct}% of your return fare if you hold a return (a single is already refunded in full from 60 minutes)`;
    }
  } else {
    text = `${pct}% of the single fare (${pct / 2}% of a return)`;
  }
  if (estimate.roomSupplementPercentage) {
    text += `, and ${estimate.roomSupplementPercentage}% of your room supplement`;
  }
  return text;
}

/** `DR30, 30+ minute delay` / `Heathrow Express, more than 30 minutes late`. */
function bandLabel(estimate: Estimate): string {
  if (estimate.scheme === 'HX') return `Heathrow Express, more than ${estimate.bandMinutes} minutes late`;
  return `${estimate.scheme}, ${estimate.bandMinutes}+ minute delay`;
}

function EstimateSummary({ response }: { response: DelayRepayEstimateResponse }) {
  const { estimate, delayMinutes } = response;
  const operator = response.schemeOperator ?? 'the operator';

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

  if (response.outcome === 'notReached') {
    return (
      <Alert color="orange" variant="light" title={`This train didn't reach ${destinationLabel(response)}`}>
        You&apos;re likely eligible for compensation, depending on your replacement journey: claim with {operator}.
      </Alert>
    );
  }

  if (response.ownScheme) {
    return (
      <>
        <Text size="sm" fw={500}>
          This operator runs its own compensation scheme.
        </Text>
        <Text size="sm">
          {delayMinutes !== null
            ? `Recorded delay: ${delayMinutes} minutes. Check ${operator}'s own rules using the link below.`
            : `Check ${operator}'s own rules using the link below.`}
        </Text>
        <ProvisionalNote response={response} />
      </>
    );
  }

  if (estimate?.borderline) {
    const threshold = estimate.thresholdMinutes ?? estimate.bandMinutes;
    const over = estimate.scheme === 'HX' ? `more-than-${threshold}-minute` : `${threshold}-minute`;
    return (
      <>
        <Alert color="yellow" variant="light" title="Borderline: could go either way">
          The projected delay
          {delayMinutes !== null ? ` (${delayMinutes} minutes)` : ''} is just over the {over} threshold
          {` (${estimate.scheme})`}, so the final delay could land on either side of it. No estimate is shown until it
          is clearer.
        </Alert>
        <ProvisionalNote response={response} />
      </>
    );
  }

  if (estimate) {
    const title = estimate.provisional ? 'Provisional Delay Repay estimate' : 'Estimated Delay Repay eligibility';
    return (
      <>
        <Alert color="grape" title={title} variant="light">
          Estimated compensation: {compensationText(estimate)} ({bandLabel(estimate)}). This is an estimate, not a
          guarantee.
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
