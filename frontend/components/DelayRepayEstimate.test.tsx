import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DelayRepayEstimate } from './DelayRepayEstimate';
import type { DelayRepayEstimate as DelayRepayEstimateType, DelayRepayEstimateResponse } from '@/lib/types';

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
    expect(screen.getByText(/50% of the single fare \(25% of a return\)/)).toBeInTheDocument();
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
      expect(screen.getByText('Distant Signal never claims on your behalf.')).toBeInTheDocument();
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
    expect(screen.getByText(/Rules last checked: 7 October 2026/)).toBeInTheDocument();
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

describe('DelayRepayEstimate: 2026-10-07 states', () => {
  function estimate(overrides: Partial<DelayRepayEstimateType> = {}): DelayRepayEstimateType {
    return {
      scheme: 'DR15',
      bandMinutes: 15,
      percentage: 25,
      fareBasis: 'single',
      ticketKind: 'unknown',
      roomSupplementPercentage: null,
      borderline: false,
      thresholdMinutes: null,
      provisional: false,
      disclaimer: ESTIMATE_DISCLAIMER,
      ...overrides,
    };
  }

  it('a borderline projection names the threshold and shows no percentage', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 16,
          provisional: true,
          measuredAtCrs: 'ASH',
          estimate: estimate({ percentage: null, borderline: true, thresholdMinutes: 15, provisional: true }),
        })}
      />,
    );
    expect(screen.getByText('Borderline: could go either way')).toBeInTheDocument();
    expect(screen.getByText(/just over the 15-minute threshold/)).toBeInTheDocument();
    expect(screen.queryByText(/%/)).not.toBeInTheDocument();
    expect(screen.getByText(/Provisional: the train hasn.t reached ASH yet/)).toBeInTheDocument();
  });

  it('words the 60-119 band as the single fare, half of a return', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({ delayMinutes: 75, estimate: estimate({ bandMinutes: 60, percentage: 100 }) })}
      />,
    );
    expect(screen.getByText(/100% of the single fare \(50% of a return\)/)).toBeInTheDocument();
  });

  it.each([
    ['return', /100% of your return fare \(DR15/],
    ['single', /100% of your single fare: singles are already refunded in full from 60 minutes/],
    [
      'unknown',
      /100% of your return fare if you hold a return \(a single is already refunded in full from 60 minutes\)/,
    ],
  ] as const)('words the 120+ band for a %s ticket', (ticketKind, text) => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 130,
          estimate: estimate({
            bandMinutes: 120,
            percentage: 100,
            fareBasis: ticketKind === 'single' ? 'single' : 'return',
            ticketKind,
          }),
        })}
      />,
    );
    expect(screen.getByText(text)).toBeInTheDocument();
  });

  it('adds the Caledonian Sleeper room supplement, and words Heathrow Express by ticket price', () => {
    const { unmount } = renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 40,
          estimate: estimate({ scheme: 'DR30', bandMinutes: 30, percentage: 50, roomSupplementPercentage: 50 }),
        })}
      />,
    );
    expect(screen.getByText(/and 50% of your room supplement/)).toBeInTheDocument();
    unmount();
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 45,
          estimate: estimate({ scheme: 'HX', bandMinutes: 30, percentage: 25, fareBasis: 'ticket' }),
        })}
      />,
    );
    expect(
      screen.getByText(/25% of your ticket price \(Heathrow Express, more than 30 minutes late\)/),
    ).toBeInTheDocument();
  });

  it('an own-scheme operator shows no percentage, just its own scheme and the claim link', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 45,
          ownScheme: true,
          schemeOperator: 'Merseyrail',
          claimUrl: 'https://m.example/',
        })}
      />,
    );
    expect(screen.getByText('This operator runs its own compensation scheme.')).toBeInTheDocument();
    expect(screen.queryByText(/%/)).not.toBeInTheDocument();
    expect(screen.getByRole('link', { name: /See how to claim from the operator/ })).toHaveAttribute(
      'href',
      'https://m.example/',
    );
  });

  it("a train that didn't reach the destination names it and the operator, with no percentage", () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          outcome: 'notReached',
          measuredAtCrs: 'EDB',
          measuredAtName: 'Edinburgh Waverley',
          schemeOperator: 'LNER',
        })}
      />,
    );
    expect(screen.getByText("This train didn't reach Edinburgh Waverley")).toBeInTheDocument();
    expect(
      screen.getByText(/likely eligible for compensation, depending on your replacement journey: claim with LNER/),
    ).toBeInTheDocument();
    expect(screen.queryByText(/%/)).not.toBeInTheDocument();
    expect(screen.queryByText(/No delay data recorded yet/)).not.toBeInTheDocument();
  });

  it('a departure-only report is final and says what it is based on', () => {
    renderWithMantine(
      <DelayRepayEstimate
        response={response({
          delayMinutes: 35,
          provisional: false,
          outcome: 'departedOnly',
          measuredAtCrs: 'XYZ',
          measuredAtName: 'Exampleton',
          estimate: estimate({ bandMinutes: 30, percentage: 50 }),
        })}
      />,
    );
    expect(screen.getByText('Estimated Delay Repay eligibility')).toBeInTheDocument();
    expect(screen.getByText(/Final, based on its departure from Exampleton/)).toBeInTheDocument();
    expect(screen.queryByText(/Provisional/)).not.toBeInTheDocument();
  });

  it('puts the summary in a polite live region and shows the served rules date', () => {
    const { container } = renderWithMantine(
      <DelayRepayEstimate response={response({ delayMinutes: 5, rulesCheckedOn: '2026-10-07' })} />,
    );
    expect(container.querySelector('[aria-live="polite"]')).toHaveTextContent(/5 minutes/);
    expect(screen.getByText(/Rules last checked: 7 October 2026/)).toBeInTheDocument();
  });
});
