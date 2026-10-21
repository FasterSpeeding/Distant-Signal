import { describe, it, expect } from 'vitest';
import {
  currentLeg,
  homeJourneyDetailIds,
  homeJourneyStatusGroup,
  TRAVEL_ARRIVAL_GRACE_MINUTES,
  journeyPhase,
  legEndMs,
  MAX_HOME_JOURNEY_DETAILS,
  splitHomeJourneys,
} from './homeJourneys';
import { journeyDetail, journeyListItem, stop, trainLeg, untrackedLeg } from '@/test/journeyFixtures';
import type { JourneyDetail } from './types';

const KGX: [string, string] = ['KGX', 'London Kings Cross'];
const YRK: [string, string] = ['YRK', 'York'];
const EDB: [string, string] = ['EDB', 'Edinburgh'];

/** 10:00 to 12:00 UK time on 17 Sep 2026, then 12:20 to 14:30. */
function twoLegs(id = 1, legState: Parameters<typeof trainLeg>[5] = {}): JourneyDetail {
  return journeyDetail(id, [
    trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', legState),
    trainLeg(2, YRK, EDB, '2026-09-17T11:20:00Z', '2026-09-17T13:30:00Z'),
  ]);
}

const at = (iso: string) => new Date(iso);

describe('journeyPhase', () => {
  it('is upcoming before the first departure, in progress between, past after the last arrival', () => {
    expect(journeyPhase(twoLegs(), at('2026-09-17T08:59:00Z'))).toBe('upcoming');
    expect(journeyPhase(twoLegs(), at('2026-09-17T09:01:00Z'))).toBe('inProgress');
    // Between legs, at the change: still in progress.
    expect(journeyPhase(twoLegs(), at('2026-09-17T11:10:00Z'))).toBe('inProgress');
    // No live data on the last train: past only once the grace margin after
    // its 13:30Z timetabled arrival has gone too.
    expect(journeyPhase(twoLegs(), at('2026-09-17T14:01:00Z'))).toBe('past');
  });

  it(`with no live data, waits ${TRAVEL_ARRIVAL_GRACE_MINUTES} min past the timetabled arrival`, () => {
    const quiet = journeyDetail(1, [trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z')]);
    expect(legEndMs(quiet.legs[0]!)).toBe(Date.parse('2026-09-17T11:30:00Z'));
    expect(journeyPhase(quiet, at('2026-09-17T11:05:00Z'))).toBe('inProgress');
    expect(journeyPhase(quiet, at('2026-09-17T11:29:00Z'))).toBe('inProgress');
    expect(journeyPhase(quiet, at('2026-09-17T11:31:00Z'))).toBe('past');
  });

  it('ignores early running, as the api does: an early train ends at its timetabled arrival', () => {
    const early = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'en_route', delayMinutes: -4 }),
    ]);
    expect(legEndMs(early.legs[0]!)).toBe(Date.parse('2026-09-17T11:00:00Z'));
  });

  it('stays current past the timetabled arrival while the train runs late, with no estimate at the stop', () => {
    const late = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'en_route', delayMinutes: 40 }),
    ]);
    // Timetable plus the current delay, and no grace on top of live data.
    expect(legEndMs(late.legs[0]!)).toBe(Date.parse('2026-09-17T11:40:00Z'));
    expect(journeyPhase(late, at('2026-09-17T11:35:00Z'))).toBe('inProgress');
    expect(journeyPhase(late, at('2026-09-17T11:41:00Z'))).toBe('past');
  });

  it('keeps a journey upcoming while its first train is late leaving', () => {
    const late = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', {
        journeyStops: [
          stop('KGX', {
            kind: 'Origin',
            scheduledDeparture: '2026-09-17T09:00:00Z',
            estimatedDeparture: '2026-09-17T09:20:00Z',
          }),
          stop('YRK', { kind: 'Terminate', scheduledArrival: '2026-09-17T11:00:00Z' }),
        ],
      }),
    ]);
    expect(journeyPhase(late, at('2026-09-17T09:10:00Z'))).toBe('upcoming');
  });

  it('keeps a journey in progress past its timetabled arrival while the last train runs late', () => {
    const late = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', {
        status: 'en_route',
        delayMinutes: 25,
        journeyStops: [
          stop('KGX', {
            kind: 'Origin',
            scheduledDeparture: '2026-09-17T09:00:00Z',
            actualDeparture: '2026-09-17T09:05:00Z',
          }),
          stop('YRK', {
            kind: 'Terminate',
            scheduledArrival: '2026-09-17T11:00:00Z',
            estimatedArrival: '2026-09-17T11:25:00Z',
          }),
        ],
      }),
    ]);
    expect(journeyPhase(late, at('2026-09-17T11:10:00Z'))).toBe('inProgress');
    expect(journeyPhase(late, at('2026-09-17T11:26:00Z'))).toBe('past');
  });

  it('is past as soon as the train reports an actual arrival at the alighting stop, or completed', () => {
    const arrived = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', {
        status: 'en_route',
        delayMinutes: 20,
        journeyStops: [
          stop('KGX', { kind: 'Origin', scheduledDeparture: '2026-09-17T09:00:00Z' }),
          stop('YRK', {
            kind: 'Intermediate',
            scheduledArrival: '2026-09-17T11:00:00Z',
            actualArrival: '2026-09-17T10:58:00Z',
          }),
          stop('EDB', { kind: 'Terminate', scheduledArrival: '2026-09-17T13:30:00Z' }),
        ],
      }),
    ]);
    // Early, before the timetable and the 20 min delay would say so.
    expect(journeyPhase(arrived, at('2026-09-17T10:59:00Z'))).toBe('past');
    const done = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'completed' }),
    ]);
    expect(journeyPhase(done, at('2026-09-17T10:30:00Z'))).toBe('past');
  });

  it("doesn't end a journey early on the may-have-arrived guess", () => {
    const mayHave = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', {
        status: 'en_route',
        delayMinutes: 30,
        mayHaveArrived: true,
      }),
    ]);
    expect(journeyPhase(mayHave, at('2026-09-17T11:20:00Z'))).toBe('inProgress');
  });

  it('keeps a journey current through a cancelled earlier leg, until its timetabled arrival', () => {
    const cancelled = twoLegs(1, { status: 'cancelled' });
    expect(journeyPhase(cancelled, at('2026-09-17T09:30:00Z'))).toBe('inProgress');
    expect(currentLeg(cancelled, at('2026-09-17T11:05:00Z'))?.id).toBe(2);
  });

  it('ends a journey whose last train is cancelled once it was due to leave, and lists it before then', () => {
    const cancelled = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'cancelled' }),
    ]);
    expect(journeyPhase(cancelled, at('2026-09-17T08:30:00Z'))).toBe('upcoming');
    expect(journeyPhase(cancelled, at('2026-09-17T09:01:00Z'))).toBe('past');
  });

  it('reads an untracked leg by its planned times', () => {
    const walk = journeyDetail(1, [untrackedLeg(1, 'Home', 'Euston', '2026-09-17T09:00:00Z', '2026-09-17T09:20:00Z')]);
    expect(journeyPhase(walk, at('2026-09-17T08:00:00Z'))).toBe('upcoming');
    expect(journeyPhase(walk, at('2026-09-17T09:10:00Z'))).toBe('inProgress');
    expect(journeyPhase(walk, at('2026-09-17T09:21:00Z'))).toBe('past');
  });

  it('with no times known (no train picked yet), is upcoming until its day ends', () => {
    const open = journeyDetail(1, [
      { ...trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z'), trackedTrainState: null },
    ]);
    expect(journeyPhase(open, at('2026-09-17T20:00:00Z'))).toBe('upcoming');
    expect(journeyPhase(open, at('2026-09-17T23:30:00Z'))).toBe('past');
  });

  it('is past for a journey with no legs', () => {
    expect(journeyPhase(journeyDetail(1, []), at('2026-09-17T09:00:00Z'))).toBe('past');
  });
});

describe('currentLeg', () => {
  it('is the first leg not yet over', () => {
    expect(currentLeg(twoLegs(), at('2026-09-17T10:00:00Z'))?.id).toBe(1);
    // The first train has no live data: it is still the current leg within
    // the grace margin after its 11:00Z arrival, and the second after it.
    expect(currentLeg(twoLegs(), at('2026-09-17T11:10:00Z'))?.id).toBe(1);
    expect(currentLeg(twoLegs(), at('2026-09-17T11:31:00Z'))?.id).toBe(2);
  });
});

describe('homeJourneyDetailIds', () => {
  it('reads journeys starting today, and yesterday ones still on yesterday or later, newest travel first', () => {
    const today = journeyListItem(twoLegs(1));
    const tomorrow = journeyListItem(twoLegs(2), undefined, {
      firstServiceDate: '2026-09-18',
      serviceDate: '2026-09-18',
    });
    const overnight = journeyListItem(twoLegs(3), undefined, {
      firstServiceDate: '2026-09-16',
      serviceDate: '2026-09-17',
    });
    const lastWeek = journeyListItem(twoLegs(4), undefined, {
      firstServiceDate: '2026-09-10',
      serviceDate: '2026-09-10',
    });
    expect(homeJourneyDetailIds([lastWeek, tomorrow, overnight, today], '2026-09-17')).toEqual([3, 1]);
  });

  it('caps the reads', () => {
    const many = Array.from({ length: MAX_HOME_JOURNEY_DETAILS + 3 }, (_, index) =>
      journeyListItem(twoLegs(index + 1)),
    );
    expect(homeJourneyDetailIds(many, '2026-09-17')).toHaveLength(MAX_HOME_JOURNEY_DETAILS);
  });
});

describe('splitHomeJourneys', () => {
  const now = at('2026-09-17T10:00:00Z');

  it('makes the soonest-arriving journey under way the current one, and lists the others', () => {
    const later = twoLegs(1);
    const sooner = journeyDetail(2, [
      untrackedLeg(1, 'Home', 'Euston', '2026-09-17T09:50:00Z', '2026-09-17T10:15:00Z'),
    ]);
    const details = new Map([
      [1, later],
      [2, sooner],
    ]);
    const split = splitHomeJourneys([journeyListItem(later), journeyListItem(sooner)], details, now);
    expect(split.current?.item.id).toBe(2);
    expect(split.alsoInProgress.map((journey) => journey.item.id)).toEqual([1]);
    expect(split.upcoming).toEqual([]);
  });

  it('lists upcoming journeys soonest first: by day, then departure, and leaves past ones out', () => {
    const past = journeyDetail(1, [untrackedLeg(1, 'Home', 'Euston', '2026-09-17T07:00:00Z', '2026-09-17T07:30:00Z')]);
    const laterToday = journeyDetail(2, [
      untrackedLeg(1, 'Euston', 'Home', '2026-09-17T17:00:00Z', '2026-09-17T17:30:00Z'),
    ]);
    const soonToday = journeyDetail(3, [
      untrackedLeg(1, 'Home', 'Gym', '2026-09-17T12:00:00Z', '2026-09-17T12:30:00Z'),
    ]);
    const nextWeek = twoLegs(4);
    const nextWeekItem = journeyListItem(nextWeek, undefined, {
      firstServiceDate: '2026-09-24',
      serviceDate: '2026-09-24',
    });
    const details = new Map([
      [1, past],
      [2, laterToday],
      [3, soonToday],
    ]);
    const split = splitHomeJourneys(
      [nextWeekItem, journeyListItem(past), journeyListItem(laterToday), journeyListItem(soonToday)],
      details,
      now,
    );
    expect(split.current).toBeNull();
    expect(split.upcoming.map((journey) => journey.item.id)).toEqual([3, 2, 4]);
    expect(split.upcoming[0]?.departure).toBe('2026-09-17T12:00:00Z');
    expect(split.upcoming[2]?.departure).toBeNull();
  });

  it('lists a journey starting today whose detail failed to load as upcoming, not lost', () => {
    const split = splitHomeJourneys([journeyListItem(twoLegs(1))], new Map(), now);
    expect(split.upcoming.map((journey) => journey.item.id)).toEqual([1]);
  });

  it('has nothing to show for no journeys', () => {
    expect(splitHomeJourneys([], new Map(), now)).toEqual({ current: null, alsoInProgress: [], upcoming: [] });
  });
});

describe('homeJourneyStatusGroup', () => {
  it('summarises the whole journey when read in full, not only the current train', () => {
    const detail = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'en_route', delayMinutes: 0 }),
      trainLeg(2, YRK, EDB, '2026-09-17T11:20:00Z', '2026-09-17T13:30:00Z', { status: 'cancelled' }),
    ]);
    expect(homeJourneyStatusGroup({ item: journeyListItem(detail), detail, departure: null })).toBe('severe');
  });

  it("falls back to the list row's status when not read in full", () => {
    const detail = twoLegs(1, { status: 'en_route', delayMinutes: 5 });
    expect(homeJourneyStatusGroup({ item: journeyListItem(detail), detail: null, departure: null })).toBe('delayed');
  });
});
