import { Alert, Stack, Text } from '@mantine/core';
import { TextLink } from './TextLink';
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
export function DelayRepayEstimate({ response }: { response: DelayRepayEstimateResponse }) {
  return (
    <Stack gap={4}>
      <EstimateSummary response={response} />
      <Text size="sm">This app never submits a claim on your behalf.</Text>
      {/* The only place in this feature that opens a new tab -- every
          other action stays same-page. */}
      <TextLink href={response.claimUrl} underline="always" target="_blank" rel="noopener noreferrer">
        See how to claim from the operator ↗
      </TextLink>
    </Stack>
  );
}

function EstimateSummary({ response }: { response: DelayRepayEstimateResponse }) {
  const { estimate, delayMinutes } = response;

  if (estimate) {
    return (
      <Alert color="blue" title="Estimated Delay Repay eligibility" variant="light">
        Estimated compensation: {estimate.percentage}% of your fare ({estimate.scheme}, {estimate.bandMinutes}+
        minute delay). This is an estimate, not a guarantee.
      </Alert>
    );
  }

  if (delayMinutes !== null) {
    // Deliberate: the API gives no way to distinguish "you're genuinely
    // under threshold" from "we don't recognize this operator's scheme"
    // from "some other reason didn't clear a band" -- this copy must not
    // assert a specific one of the three the response doesn't support.
    return (
      <Text size="sm">
        Based on the recorded delay ({delayMinutes}
        {' '}minutes), this operator&apos;s Delay Repay rules may not give
        a payout at that length — but rules vary and this estimate can be wrong, so it&apos;s still worth checking
        directly.
      </Text>
    );
  }

  return (
    <Text size="sm">
      No delay data recorded yet for this journey — if you already know you were delayed, the link below still
      goes straight to the operator.
    </Text>
  );
}
