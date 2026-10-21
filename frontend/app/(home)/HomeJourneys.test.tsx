import { describe, it, expect } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { expectNoUnguardedNowrapBadges } from '@/test/shrinkGuard';
import {
  journeyDetail,
  journeyListItem,
  loneTrain,
  stop,
  trainLeg,
  trainState,
  untrackedLeg,
} from '@/test/journeyFixtures';
import { loneTrainLeg, MAX_UPCOMING_JOURNEYS, type HomeJourney, type HomeLoneTrain } from '@/lib/homeJourneys';
import { trackedTrainRowStatus } from '@/lib/trackedRowStatus';
import type { JourneyDetail, TrackedTrainListItem } from '@/lib/types';
import { CurrentJourneyCard, CurrentJourneySection, UpcomingJourneysSection } from './HomeJourneys';

const KGX: [string, string] = ['KGX', 'London Kings Cross'];
const YRK: [string, string] = ['YRK', 'York'];
const EDB: [string, string] = ['EDB', 'Edinburgh'];

/** A Kings Cross to Edinburgh journey changing at York: 10:00 to 12:00,
 * then 12:20 to 14:30 UK time, the first train 12 minutes late. */
function journey(id = 7, customName: string | null = null): HomeJourney & { detail: JourneyDetail } {
  const detail = journeyDetail(
    id,
    [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', {
        status: 'en_route',
        delayMinutes: 12,
        journeyStops: [
          stop('KGX', {
            kind: 'Origin',
            scheduledDeparture: '2026-09-17T09:00:00Z',
            actualDeparture: '2026-09-17T09:12:00Z',
          }),
          stop('YRK', {
            kind: 'Terminate',
            scheduledArrival: '2026-09-17T11:00:00Z',
            estimatedArrival: '2026-09-17T11:12:00Z',
          }),
        ],
      }),
      trainLeg(2, YRK, EDB, '2026-09-17T11:20:00Z', '2026-09-17T13:30:00Z'),
    ],
    customName,
  );
  return { item: journeyListItem(detail), detail, departure: '2026-09-17T09:12:00Z' };
}

/** A Kings Cross to York train tracked on its own, as the home page gets
 * it from its list row alone (not read in full). */
function lone(id: number, overrides: Partial<TrackedTrainListItem> = {}): HomeLoneTrain {
  const train = loneTrain(id, overrides);
  return {
    train,
    status: trackedTrainRowStatus(train, '2026-09-17'),
    departure: train.pinScheduledDeparture,
    leg: null,
  };
}

/** The same train read in full (`GET /Train/{id}`): left 10:05, due into
 * York 12:00 UK time and expected 12:07. */
function loneRead(id: number): HomeLoneTrain {
  const entry = lone(id, { status: 'en_route', delayMinutes: 7 });
  const state = trainState('KGX', 'YRK', '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', {
    id,
    status: 'en_route',
    delayMinutes: 7,
    journeyStops: [
      stop('KGX', {
        kind: 'Origin',
        scheduledDeparture: '2026-09-17T09:00:00Z',
        actualDeparture: '2026-09-17T09:05:00Z',
      }),
      stop('YRK', {
        kind: 'Terminate',
        scheduledArrival: '2026-09-17T11:00:00Z',
        estimatedArrival: '2026-09-17T11:07:00Z',
      }),
    ],
  });
  return { ...entry, departure: '2026-09-17T09:05:00Z', leg: loneTrainLeg(entry.train, state) };
}

describe('CurrentJourneyCard', () => {
  it('shows the journey, its live status, the leg under way and the next change, linking to the journey', () => {
    renderWithMantine(<CurrentJourneyCard journey={journey()} now={new Date('2026-09-17T10:00:00Z')} />);
    const link = screen.getByRole('link');
    expect(link).toHaveAttribute('href', '/journeys/7');
    expect(within(link).getByText('London Kings Cross (KGX) to Edinburgh (EDB), 17 Sep 2026')).toBeInTheDocument();
    // The journey's worst status, then the leg's own delay in minutes.
    expect(within(link).getByText('Delayed')).toBeInTheDocument();
    expect(within(link).getByText('12 min late')).toBeInTheDocument();
    expect(within(link).getByText('Now, leg 1 of 2')).toBeInTheDocument();
    expect(within(link).getByText('London Kings Cross (KGX) to York (YRK) · 10:00')).toBeInTheDocument();
    expect(within(link).getByText('Next change')).toBeInTheDocument();
    expect(within(link).getByText('York (YRK), arrive est. 12:12')).toBeInTheDocument();
  });

  it("headlines the whole journey's status, with the train under way's own status second", () => {
    const detail = journeyDetail(9, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'en_route', delayMinutes: 0 }),
      trainLeg(2, YRK, EDB, '2026-09-17T11:20:00Z', '2026-09-17T13:30:00Z', { status: 'cancelled' }),
    ]);
    renderWithMantine(
      <CurrentJourneyCard
        journey={{ item: journeyListItem(detail), detail, departure: null }}
        now={new Date('2026-09-17T10:00:00Z')}
      />,
    );
    const [headline, leg] = screen.getAllByText(/^(Cancelled|On time)$/);
    expect(headline).toHaveTextContent('Cancelled');
    expect(leg).toHaveTextContent('On time');
  });

  it('on the last leg, gives the destination and arrival instead of a change', () => {
    renderWithMantine(<CurrentJourneyCard journey={journey()} now={new Date('2026-09-17T12:00:00Z')} />);
    expect(screen.getByText('Now, leg 2 of 2')).toBeInTheDocument();
    expect(screen.getByText('Destination')).toBeInTheDocument();
    expect(screen.getByText('Edinburgh (EDB), arrive est. 14:30')).toBeInTheDocument();
    expect(screen.queryByText('Next change')).not.toBeInTheDocument();
  });

  it('says a cancelled train is cancelled', () => {
    const cancelled = journeyDetail(3, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'cancelled' }),
    ]);
    renderWithMantine(
      <CurrentJourneyCard
        journey={{ item: journeyListItem(cancelled), detail: cancelled, departure: null }}
        now={new Date('2026-09-17T10:00:00Z')}
      />,
    );
    expect(screen.getByText('Cancelled')).toBeInTheDocument();
    expect(screen.getByText('Now')).toBeInTheDocument();
  });

  it('describes an untracked leg by its mode, places and planned times', () => {
    const walk = journeyDetail(4, [
      untrackedLeg(1, 'Euston', 'Victoria', '2026-09-17T09:50:00Z', '2026-09-17T10:15:00Z'),
    ]);
    renderWithMantine(
      <CurrentJourneyCard
        journey={{ item: journeyListItem(walk), detail: walk, departure: null }}
        now={new Date('2026-09-17T10:00:00Z')}
      />,
    );
    expect(screen.getByText('Tube: Victoria line, Euston to Victoria · 10:50 to 11:15')).toBeInTheDocument();
    expect(screen.getByText('Victoria, arrive est. 11:15')).toBeInTheDocument();
  });

  it('shows a custom name with the route and date under it', () => {
    renderWithMantine(<CurrentJourneyCard journey={journey(7, 'Trip north')} now={new Date('2026-09-17T10:00:00Z')} />);
    expect(screen.getByText('Trip north')).toBeInTheDocument();
    expect(screen.getByText('London Kings Cross (KGX) to Edinburgh (EDB), 17 Sep 2026 · 2 legs')).toBeInTheDocument();
  });

  it('never lets a long name squeeze its status badge', () => {
    const { container } = renderWithMantine(
      <CurrentJourneyCard
        journey={journey(7, 'A very long journey name '.repeat(8))}
        now={new Date('2026-09-17T10:00:00Z')}
      />,
    );
    expectNoUnguardedNowrapBadges(container);
  });
});

describe('CurrentJourneySection', () => {
  it('heads the section and lists other journeys under way', () => {
    const other = journey(8, 'Second trip');
    renderWithMantine(
      <CurrentJourneySection current={journey()} alsoInProgress={[other]} now={new Date('2026-09-17T10:00:00Z')} />,
    );
    const section = screen.getByRole('region', { name: 'Current journey' });
    expect(within(section).getByRole('heading', { name: 'Current journey', level: 2 })).toBeInTheDocument();
    expect(within(section).getByRole('heading', { name: 'Also in progress', level: 3 })).toBeInTheDocument();
    expect(within(section).getByRole('link', { name: /Second trip/ })).toHaveAttribute('href', '/journeys/8');
  });

  it('has no "Also in progress" list with one journey under way', () => {
    renderWithMantine(
      <CurrentJourneySection current={journey()} alsoInProgress={[]} now={new Date('2026-09-17T10:00:00Z')} />,
    );
    expect(screen.queryByRole('heading', { name: 'Also in progress' })).not.toBeInTheDocument();
  });

  it('gives a lone train read in full its destination and arrival, as a journey leg', () => {
    renderWithMantine(
      <CurrentJourneySection current={loneRead(5)} alsoInProgress={[]} now={new Date('2026-09-17T10:00:00Z')} />,
    );
    const card = within(screen.getByRole('region', { name: 'Current journey' })).getByRole('link', {
      name: /^London Kings Cross \(KGX\) to York \(YRK\), 17 Sep/,
    });
    expect(within(card).getByText('Destination')).toBeInTheDocument();
    expect(within(card).getByText('York (YRK), arrive est. 12:07')).toBeInTheDocument();
    expect(within(card).getByText('London Kings Cross (KGX) to York (YRK) · 10:00')).toBeInTheDocument();
  });

  it('gives a lone train known only by its list row its destination, without an arrival', () => {
    renderWithMantine(
      <CurrentJourneySection
        current={lone(5, { status: 'en_route' })}
        alsoInProgress={[]}
        now={new Date('2026-09-17T10:00:00Z')}
      />,
    );
    const card = within(screen.getByRole('region', { name: 'Current journey' })).getByRole('link');
    expect(within(card).getByText('York (YRK)')).toBeInTheDocument();
    expect(within(card).queryByText(/arrive/)).not.toBeInTheDocument();
  });

  it('draws a lone train under way as the current journey, linking to its train page', () => {
    renderWithMantine(
      <CurrentJourneySection
        current={lone(5, { status: 'en_route', delayMinutes: 6, customName: 'Commute' })}
        alsoInProgress={[journey(8, 'Second trip')]}
        now={new Date('2026-09-17T10:00:00Z')}
      />,
    );
    const section = screen.getByRole('region', { name: 'Current journey' });
    const card = within(section).getByRole('link', { name: /^Commute/ });
    expect(card).toHaveAttribute('href', '/train/W5/2026-09-17');
    expect(card.closest('.currentJourneyCard')).not.toBeNull();
    expect(within(card).getByText('London Kings Cross (KGX) to York (YRK), 17 Sep 2026 · 10:00')).toBeInTheDocument();
    expect(within(card).getByText('London Kings Cross (KGX) to York (YRK) · 10:00')).toBeInTheDocument();
    expect(within(card).getByText('York (YRK)')).toBeInTheDocument();
    expect(within(card).getByText('6 min late')).toBeInTheDocument();
    expect(within(section).getByRole('link', { name: /Second trip/ })).toHaveAttribute('href', '/journeys/8');
  });

  it('lists a lone train under way beside the current journey', () => {
    renderWithMantine(
      <CurrentJourneySection
        current={journey()}
        alsoInProgress={[lone(5, { status: 'en_route', resolutionStatus: 'pending' })]}
        now={new Date('2026-09-17T10:00:00Z')}
      />,
    );
    const section = screen.getByRole('region', { name: 'Current journey' });
    expect(
      within(section).getByRole('link', { name: /^London Kings Cross \(KGX\) to York \(YRK\), 17 Sep/ }),
    ).toHaveAttribute('href', '/train/by-id/5');
  });
});

describe('UpcomingJourneysSection', () => {
  function upcoming(id: number, customName: string | null = null, departure: string | null = null): HomeJourney {
    const detail = journeyDetail(
      id,
      [
        trainLeg(1, KGX, YRK, '2026-09-20T09:00:00Z', '2026-09-20T11:00:00Z', {
          resolutionStatus: 'resolved',
          trackingPhase: 'planned',
        }),
      ],
      customName,
    );
    return { item: journeyListItem(detail), detail: null, departure };
  }

  it('lists each journey with its status, linking to it, and "View all" to the Journeys page', () => {
    renderWithMantine(
      <UpcomingJourneysSection upcoming={[upcoming(1, null, '2026-09-20T09:00:00Z'), upcoming(2, 'Weekend away')]} />,
    );
    const section = screen.getByRole('region', { name: 'Upcoming journeys' });
    const first = within(section).getByRole('link', {
      name: /^London Kings Cross \(KGX\) to York \(YRK\), 20 Sep 2026/,
    });
    expect(first).toHaveAttribute('href', '/journeys/1');
    expect(within(first).getByText('Departs 10:00')).toBeInTheDocument();
    expect(within(first).getByText('Planned')).toBeInTheDocument();
    const second = within(section).getByRole('link', { name: /Weekend away/ });
    expect(within(second).getByText('London Kings Cross (KGX) to York (YRK), 20 Sep 2026')).toBeInTheDocument();
    expect(within(section).getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/journeys');
  });

  it('offers New journey at the top of the section, as a button', () => {
    renderWithMantine(<UpcomingJourneysSection upcoming={[upcoming(1)]} />);
    const section = screen.getByRole('region', { name: 'Upcoming journeys' });
    const button = within(section).getByRole('link', { name: 'New journey' });
    expect(button).toHaveAttribute('href', '/journeys/new');
    expect(button).toHaveClass('mantine-Button-root');
    // Above the list: it comes before the first journey's row.
    const firstRow = within(section).getByRole('link', { name: /to York/ });
    expect(button.compareDocumentPosition(firstRow) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it('with nothing upcoming, says so and still offers New journey, without View all', () => {
    renderWithMantine(<UpcomingJourneysSection upcoming={[]} />);
    const section = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(section).getByText('No upcoming journeys.')).toBeInTheDocument();
    expect(within(section).getByRole('link', { name: 'New journey' })).toHaveAttribute('href', '/journeys/new');
    expect(within(section).queryByRole('link', { name: 'View all' })).not.toBeInTheDocument();
  });

  it('when the list could not be read, says that rather than "no upcoming journeys"', () => {
    renderWithMantine(<UpcomingJourneysSection upcoming={[]} loadFailed />);
    const section = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(section).getByText("Couldn't load your journeys. Try again.")).toBeInTheDocument();
    expect(within(section).queryByText('No upcoming journeys.')).not.toBeInTheDocument();
    expect(within(section).getByRole('link', { name: 'New journey' })).toBeInTheDocument();
  });

  it(`caps the list at ${MAX_UPCOMING_JOURNEYS} and says how many more there are`, () => {
    const many = Array.from({ length: MAX_UPCOMING_JOURNEYS + 2 }, (_, index) => upcoming(index + 1));
    renderWithMantine(<UpcomingJourneysSection upcoming={many} />);
    expect(screen.getAllByRole('link', { name: /to York/ })).toHaveLength(MAX_UPCOMING_JOURNEYS);
    expect(screen.getByText('2 more upcoming journeys are on the Journeys page.')).toBeInTheDocument();
  });

  it('lists a lone train among the journeys, with the Track list badge, linking to its train page', () => {
    renderWithMantine(
      <UpcomingJourneysSection upcoming={[lone(5, { status: 'cancelled' }), upcoming(1, 'Weekend away')]} />,
    );
    const section = screen.getByRole('region', { name: 'Upcoming journeys' });
    const train = within(section).getByRole('link', { name: /^London Kings Cross \(KGX\) to York \(YRK\), 17 Sep/ });
    expect(train).toHaveAttribute('href', '/train/W5/2026-09-17');
    expect(within(train).getByText('Cancelled')).toBeInTheDocument();
    expect(within(section).queryByText('No upcoming journeys.')).not.toBeInTheDocument();
    expect(within(section).getByRole('link', { name: 'View all' })).toHaveAttribute('href', '/journeys');
  });

  it("when the journeys couldn't be read, says so and still lists the lone trains", () => {
    renderWithMantine(<UpcomingJourneysSection upcoming={[lone(5)]} loadFailed />);
    const section = screen.getByRole('region', { name: 'Upcoming journeys' });
    expect(within(section).getByText("Couldn't load your journeys. Try again.")).toBeInTheDocument();
    expect(within(section).getByRole('link', { name: /to York/ })).toHaveAttribute('href', '/train/W5/2026-09-17');
  });
});
