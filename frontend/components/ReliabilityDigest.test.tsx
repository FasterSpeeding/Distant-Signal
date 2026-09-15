import { describe, it, expect } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ReliabilityDigest } from './ReliabilityDigest';
import type { TrackedTrainListItem, TicketListItem } from '@/lib/types';

// Substring-equal to DelayRepayEstimate.test.tsx's own TOP_LEVEL_DISCLAIMER
// fixture -- this is the one mechanical guard against the two silently
// drifting apart if `delay_repay_rules::ROUTE_DISCLAIMER` is ever
// reworded. See docs/superpowers/specs/2026-09-12-reliability-digest-design.md
// Open questions 1.
const CARRIED_FORWARD_DISCLAIMER = 'not a guarantee of compensation and not proof you travelled';

function train(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-09-01',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinOriginName: 'London Kings Cross',
    pinDestinationName: 'York',
    pinScheduledDeparture: '2026-09-01T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'completed',
    delayMinutes: 3,
    trackedAt: '2026-08-30T12:00:00Z',
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

function ticket(overrides: Partial<TicketListItem> = {}): TicketListItem {
  return {
    id: 1,
    trackedTrainId: 1,
    operator: 'LNER',
    ticketType: null,
    originCrs: 'KGX',
    destinationCrs: 'YRK',
    originName: null,
    destinationName: null,
    source: 'manual',
    createdAt: '2026-08-30T12:00:00Z',
    serviceDate: '2026-09-01',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinScheduledDeparture: '2026-09-01T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'completed',
    delayMinutes: 35,
    estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'estimate disclaimer' },
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: 'route disclaimer',
    customName: null,
    ...overrides,
  };
}

describe('ReliabilityDigest', () => {
  it('zero eligible punctuality data: renders the check-back-later prose, never a 0%/0-minute figure', () => {
    renderWithMantine(<ReliabilityDigest trains={[]} tickets={[]} />);
    expect(screen.getByText(/check back once it's finished running/)).toBeInTheDocument();
    expect(screen.queryByText(/0%/)).not.toBeInTheDocument();
  });

  it('zero attached-tickets-with-operator: renders the attach-a-ticket prose, never a 0-of-0 figure', () => {
    renderWithMantine(<ReliabilityDigest trains={[]} tickets={[]} />);
    expect(screen.getByText(/Attach a ticket to one of your tracked trains/)).toBeInTheDocument();
    expect(screen.queryByText(/0 of 0/)).not.toBeInTheDocument();
  });

  it('with eligible data: shows an on-time percentage and eligible count', () => {
    renderWithMantine(<ReliabilityDigest trains={[train({ serviceDate: '2026-09-01', delayMinutes: 0 })]} tickets={[]} />);
    expect(screen.getByText(/100%/)).toBeInTheDocument();
  });

  it('every finished journey cancelled: reports the cancellation count, not the generic no-data prose', () => {
    // Fix for review finding A: eligibleCount excludes cancelled
    // journeys, so an all-cancelled population must not fall into the
    // "nothing tracked yet" branch, which would silently hide the
    // cancellations from the user.
    renderWithMantine(
      <ReliabilityDigest trains={[train({ serviceDate: '2026-09-01', status: 'cancelled' })]} tickets={[]} />,
    );
    expect(screen.getByText(/1 tracked journey was cancelled/)).toBeInTheDocument();
    expect(screen.queryByText(/check back once it's finished running/)).not.toBeInTheDocument();
    expect(screen.queryByText(/0%/)).not.toBeInTheDocument();
  });

  it('worst-journeys list excludes on-time/early entries: an all-on-time population shows no "most delayed" list', () => {
    // Fix for review finding B: without a delayMinutes > 0 filter this
    // would otherwise render a nonsensical "0 minutes late" row under
    // "Your most delayed tracked journeys".
    renderWithMantine(
      <ReliabilityDigest
        trains={[
          train({ id: 1, serviceDate: '2026-09-01', delayMinutes: 0 }),
          train({ id: 2, serviceDate: '2026-08-31', delayMinutes: -3 }),
        ]}
        tickets={[]}
      />,
    );
    expect(screen.getByText(/100%/)).toBeInTheDocument();
    expect(screen.queryByText(/Your most delayed tracked journeys/)).not.toBeInTheDocument();
    expect(screen.queryByText(/minutes late/)).not.toBeInTheDocument();
  });

  it('hedged-copy: carries the disclaimer forward verbatim, the aggregate-specific no-total sentence, no claim-performing language, and no outbound/claim link in the rollup', () => {
    renderWithMantine(<ReliabilityDigest trains={[train({ serviceDate: '2026-09-01' })]} tickets={[ticket()]} />);
    expect(screen.getByText(new RegExp(CARRIED_FORWARD_DISCLAIMER))).toBeInTheDocument();
    expect(screen.getByText(/never stores ticket prices/)).toBeInTheDocument();
    expect(screen.queryByText(/claim now/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/get your refund/i)).not.toBeInTheDocument();
    // Word-boundary, not a bare /submit/i: the component's own rendered
    // copy doesn't currently contain "submit" at all (it was worded to
    // avoid the word entirely -- see DelayRepaySection's closing
    // paragraph), but the *backend's* ROUTE_DISCLAIMER this copy is
    // carried forward from legitimately contains "submits" as part of an
    // anti-CTA hedge ("This app never submits a claim on your behalf") --
    // a bare /submit/i substring match would flag that verbatim clause as
    // "claim-performing" language if a future wording pass ever
    // reintroduces it here. What this guards against is a literal
    // "Submit" CTA verb/button (matching DelayRepayEstimate.test.tsx's
    // own "never claim-performing language" case), which a whole-word
    // match still catches without penalizing "submits" in hedge prose.
    expect(screen.queryByText(/\bsubmit\b/i)).not.toBeInTheDocument();
    // Scoped to the rollup section itself (spec Decision 4: "no outbound
    // claim link is rendered at the rollup level at all") -- not the
    // whole digest. The punctuality section's own "most delayed
    // journeys" list deliberately does link through to each journey's
    // existing detail page (spec Decision 2/Architecture), the same
    // /train/{uid}/{date} href TrackedTrainListRow already computes; that
    // is ordinary in-app navigation, not an outbound Delay Repay claim
    // link, and is not what this guardrail is about.
    const rollup = screen.getByTestId('delay-repay-rollup');
    expect(within(rollup).queryByRole('link')).not.toBeInTheDocument();
  });

  it('never renders a currency figure ("£") anywhere', () => {
    renderWithMantine(<ReliabilityDigest trains={[train({ serviceDate: '2026-09-01' })]} tickets={[ticket()]} />);
    expect(screen.queryByText(/£/)).not.toBeInTheDocument();
  });
});
