import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DelayRepayEstimate } from './DelayRepayEstimate';
import type { DelayRepayEstimateResponse } from '@/lib/types';

const TOP_LEVEL_DISCLAIMER =
  'This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled. This app never submits a claim on your behalf -- verify eligibility and claim directly from the operator using the link above.';
const ESTIMATE_DISCLAIMER =
  'This is a rough, community-sourced estimate, not a guarantee of compensation and not proof you travelled. Always verify eligibility and submit any claim directly with the operator -- this app never submits a claim on your behalf.';

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

  it('estimate null with a real delayMinutes: does not assert a specific reason', () => {
    renderWithMantine(<DelayRepayEstimate response={response({ delayMinutes: 10 })} />);
    expect(screen.getByText(/10 minutes/)).toBeInTheDocument();
    expect(screen.getByText(/rules vary and this estimate can be wrong/)).toBeInTheDocument();
  });

  it('estimate and delayMinutes both null: says no delay data recorded yet', () => {
    renderWithMantine(<DelayRepayEstimate response={response()} />);
    expect(screen.getByText(/No delay data recorded yet/)).toBeInTheDocument();
  });

  it('always renders the top-level disclaimer verbatim, in every branch', () => {
    const cases = [
      response(),
      response({ delayMinutes: 10 }),
      response({ delayMinutes: 35, estimate: { scheme: 'DR15', bandMinutes: 30, percentage: 50, disclaimer: ESTIMATE_DISCLAIMER } }),
    ];
    for (const r of cases) {
      const { unmount } = renderWithMantine(<DelayRepayEstimate response={r} />);
      expect(screen.getByText(TOP_LEVEL_DISCLAIMER)).toBeInTheDocument();
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
});
