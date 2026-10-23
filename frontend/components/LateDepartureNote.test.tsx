import { describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { journeyDetail, trainLeg } from '@/test/journeyFixtures';
import { JourneyDetailView } from './JourneyDetailView';
import {
  instantLateDepartureTimes,
  LateDepartureNote,
  lateDepartureText,
  plannerLateDepartureTimes,
  trainToSubject,
} from './LateDepartureNote';
import type { LegConnection } from '@/lib/types';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/journeys/42',
  useSearchParams: () => new URLSearchParams(''),
}));

const RELIANCE = {
  scheduledDeparture: '2026-09-17T09:20:00Z',
  expectedDeparture: '2026-09-17T09:35:00Z',
  delayMinutes: 15,
  slackIfOnTimeMinutes: -3,
};

describe('LateDepartureNote', () => {
  it('says the change relies on the train running late, with both times', () => {
    renderWithMantine(<LateDepartureNote scheduled="10:20" expected="10:35" />);
    expect(
      screen.getByText(
        "Relies on this train running late. It's due to leave at 10:35, booked 10:20. It could leave earlier.",
      ),
    ).toBeInTheDocument();
  });

  it('reads planner clock times and journey instants as UK clock times', () => {
    expect(
      plannerLateDepartureTimes({ ...RELIANCE, scheduledDeparture: '10:20:00', expectedDeparture: '10:35:00' }),
    ).toEqual({ scheduled: '10:20', expected: '10:35' });
    expect(instantLateDepartureTimes(RELIANCE)).toEqual({ scheduled: '10:20', expected: '10:35' });
  });

  it('names a train by where it goes, else as the next one', () => {
    expect(lateDepartureText('10:20', '10:35', trainToSubject('Edinburgh'))).toMatch(
      /^Relies on the train to Edinburgh running late\./,
    );
    expect(trainToSubject(null)).toBe('the next train');
  });
});

describe('JourneyDetailView connector', () => {
  const KGX: [string, string] = ['KGX', 'London Kings Cross'];
  const YRK: [string, string] = ['YRK', 'York'];
  const EDB: [string, string] = ['EDB', 'Edinburgh'];

  function render(connection: LegConnection) {
    const detail = journeyDetail(42, [
      trainLeg(1, KGX, YRK, '2026-09-17T07:00:00Z', '2026-09-17T09:00:00Z'),
      { ...trainLeg(2, YRK, EDB, '2026-09-17T09:20:00Z', '2026-09-17T11:00:00Z'), connection },
    ]);
    return renderWithMantine(
      <JourneyDetailView journey={detail} fetchedAt="2026-09-17T08:30:00Z" origin="https://example.test" />,
    );
  }

  it('notes a change that relies on the next train running late under the connector', () => {
    render({ gapMinutes: 35, minimumChangeMinutes: 5, state: 'ok', reliesOnLateDeparture: RELIANCE });
    const note = screen.getByText(
      "Relies on the next train running late. It's due to leave at 10:35, booked 10:20. It could leave earlier.",
    );
    expect(note.closest('[data-leg-connector]')).toBeNull();
    expect(note.parentElement?.previousElementSibling).toHaveAttribute('data-leg-connector');
  });

  it('adds nothing to a change that works on time', () => {
    render({ gapMinutes: 20, minimumChangeMinutes: 5, state: 'ok' });
    expect(document.querySelector('[data-leg-connector]')).not.toBeNull();
    expect(screen.queryByText(/running late/)).not.toBeInTheDocument();
  });
});
