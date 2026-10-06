import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DelayRepayEstimate } from './DelayRepayEstimate';
import type { DelayRepayEstimateResponse } from '@/lib/types';

const TOP_LEVEL_DISCLAIMER =
  'This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled. This app never submits a claim on your behalf — verify eligibility and claim directly from the operator using the link above.';
const ESTIMATE_DISCLAIMER =
  'This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled. Always verify eligibility and submit any claim directly with the operator — this app never submits a claim on your behalf.';

function response(overrides: Partial<DelayRepayEstimateResponse> = {}): DelayRepayEstimateResponse {
  return {
    delayMinutes: null,
    estimate: null,
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: TOP_LEVEL_DISCLAIMER,
    ...overrides,
  };
}

describe('DelayRepayEstimate', () => {
  it('estimate present: shows the scheme/band/percentage, framed as an estimate', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 35,
          estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: ESTIMATE_DISCLAIMER },
        })}
      />,
    );
    expect(screen.getByText(/50% of your fare/)).toBeInTheDocument();
    expect(screen.getByText(/DR30/)).toBeInTheDocument();
    expect(screen.getByText(/This is an estimate, not a guarantee/)).toBeInTheDocument();
  });

  it('a provisional estimate says so, names where it is measured, and the 120-minute band is of the return fare', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 125,
          provisional: true,
          delayBasis: 'publicSchedule',
          measuredAtCrs: 'EDB',
          estimate: {
            scheme: 'DR30',
            bandMinutes: 120,
            percentage: 100,
            fareBasis: 'return',
            provisional: true,
            disclaimer: ESTIMATE_DISCLAIMER,
          },
        })}
      />,
    );
    expect(screen.getByText('Provisional Delay Repay estimate')).toBeInTheDocument();
    expect(screen.getByText(/100% of your return fare/)).toBeInTheDocument();
    expect(screen.getByText(/hasn.t reached EDB yet/)).toBeInTheDocument();
  });

  it('a final estimate carries no provisional note', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 35,
          provisional: false,
          measuredAtCrs: 'EDB',
          estimate: {
            scheme: 'DR30',
            bandMinutes: 30,
            percentage: 50,
            provisional: false,
            disclaimer: ESTIMATE_DISCLAIMER,
          },
        })}
      />,
    );
    expect(screen.getByText('Estimated Delay Repay eligibility')).toBeInTheDocument();
    expect(screen.queryByText(/Provisional/)).not.toBeInTheDocument();
  });

  it('estimate null with a real delayMinutes: does not assert a specific reason', () => {
    renderWithMantine(<DelayRepayEstimate response={response({ delayMinutes: 10 })} />);
    expect(screen.getByText(/10 minutes/)).toBeInTheDocument();
    expect(screen.getByText(/rules vary and this estimate can be wrong/)).toBeInTheDocument();
  });

  it('renders a space between the delayMinutes interpolation and "minutes)" (review §4.4: explicit {\' \'} guard against dropped whitespace)', () => {
    renderWithMantine(<DelayRepayEstimate response={response({ delayMinutes: 10 })} />);
    expect(screen.getByText(/\(10 minutes\)/)).toBeInTheDocument();
    expect(screen.queryByText(/10minutes/)).not.toBeInTheDocument();
  });

  it('estimate and delayMinutes both null: says no delay data recorded yet', () => {
    renderWithMantine(<DelayRepayEstimate response={response()} />);
    expect(screen.getByText(/No delay data recorded yet/)).toBeInTheDocument();
  });

  // Task 3.6.11: the full ~120-word `response.disclaimer` used to render
  // verbatim here, repeated on every ticket -- it now renders once, in
  // full, at the `/track/mine` card level (`ReliabilityDigest.tsx`'s
  // `DelayRepaySection`). This component keeps only the short anti-CTA
  // reminder next to its own claim link, in every branch.
  it('always renders the short "never submits a claim" reminder, in every branch, never the full backend disclaimer', () => {
    const cases = [
      response(),
      response({ delayMinutes: 10 }),
      response({
        delayMinutes: 35,
        estimate: { scheme: 'DR15', bandMinutes: 30, percentage: 50, disclaimer: ESTIMATE_DISCLAIMER },
      }),
    ];
    for (const r of cases) {
      const { unmount } = renderWithMantine(<DelayRepayEstimate response={r} />);
      expect(screen.getByText('This app never submits a claim on your behalf.')).toBeInTheDocument();
      expect(screen.queryByText(TOP_LEVEL_DISCLAIMER)).not.toBeInTheDocument();
      unmount();
    }
  });

  it('never renders estimate.disclaimer a second time alongside the top-level one', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 60,
          estimate: { scheme: 'DR15', bandMinutes: 60, percentage: 100, disclaimer: ESTIMATE_DISCLAIMER },
        })}
      />,
    );
    expect(screen.queryByText(ESTIMATE_DISCLAIMER)).not.toBeInTheDocument();
  });

  it('always renders claimUrl as an external, new-tab link, never claim-performing language', () => {
    renderWithMantine(<DelayRepayEstimate response={response({ claimUrl: 'https://example.com/claim' })} />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', 'https://example.com/claim');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener noreferrer');
    expect(screen.queryByText(/^Claim now$/)).not.toBeInTheDocument();
    expect(screen.queryByText(/^Submit claim$/)).not.toBeInTheDocument();
  });

  // LEG-14
  it("says when the rules were last checked and that delays may differ from the operator's records", () => {
    renderWithMantine(<DelayRepayEstimate response={response({ estimate: null, delayMinutes: 12 })} />);
    expect(screen.getByText(/Rules last checked: 29 August 2026/)).toBeInTheDocument();
    expect(screen.getByText(/may differ from the operator.s own records/)).toBeInTheDocument();
  });
});

describe('DelayRepayEstimate: bus and ferry legs', () => {
  it('explains a bus leg cannot be measured, and still links to the claim', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          serviceMode: 'replacementBus',
          liveTracking: false,
          unmeasurableReason: "Buses and ferries aren't tracked live, so we can't measure a delay on this leg.",
        })}
      />,
    );
    expect(screen.getByText(/Rail replacement bus: Buses and ferries aren't tracked live/)).toBeInTheDocument();
    expect(screen.queryByText(/No delay data recorded yet/)).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: /See how to claim from the operator/ })).toBeInTheDocument();
  });

  it('falls back to its own wording for a ferry when the backend sends no reason', () => {
    renderWithMantine(<DelayRepayEstimate response={response({ serviceMode: 'ferry', liveTracking: false })} />);
    expect(screen.getByText(/Ferry: buses and ferries aren't tracked live/)).toBeInTheDocument();
  });
});
