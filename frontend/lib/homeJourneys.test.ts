import { describe, it, expect } from 'vitest';
import {
  currentLeg,
  homeDetailReads,
  homeJourneyStatusGroup,
  isLoneTrain,
  isSharedNotFollowed,
  sharedHomeJourneys,
  TRAVEL_ARRIVAL_GRACE_MINUTES,
  journeyPhase,
  legEndMs,
  MAX_HOME_JOURNEY_DETAILS,
  splitHomeJourneys,
  type HomeEntry,
} from './homeJourneys';
import { trackedTrainRowStatus } from './trackedRowStatus';
import {
  journeyDetail,
  journeyListItem,
  loneTrain,
  stop,
  trainLeg,
  trainState,
  untrackedLeg,
} from '@/test/journeyFixtures';
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

/** "j1" for journey 1, "t5" for lone train 5, so an order mixing the two
 * reads plainly. */
const ids = (entries: readonly HomeEntry[]) =>
  entries.map((entry) => (isLoneTrain(entry) ? `t${entry.train.id}` : `j${entry.item.id}`));

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

describe('homeDetailReads', () => {
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
    expect(homeDetailReads([lastWeek, tomorrow, overnight, today], [], '2026-09-17')).toEqual({
      journeyIds: [3, 1],
      trainIds: [],
    });
  });

  it("reads lone trains dated today or yesterday that aren't travelled", () => {
    const trains = [
      loneTrain(5),
      loneTrain(6, { serviceDate: '2026-09-16', pinScheduledDeparture: '2026-09-16T22:00:00Z' }),
      loneTrain(7, { serviceDate: '2026-09-18', pinScheduledDeparture: '2026-09-18T09:00:00Z' }),
      loneTrain(8, { travelled: true }),
      loneTrain(9, { serviceDate: '2026-09-10', pinScheduledDeparture: '2026-09-10T09:00:00Z' }),
    ];
    expect(homeDetailReads([], trains, '2026-09-17')).toEqual({ journeyIds: [], trainIds: [5, 6] });
  });

  it('shares one cap between journeys and trains, keeping the soonest: today first, then by departure', () => {
    const journeys = Array.from({ length: MAX_HOME_JOURNEY_DETAILS - 2 }, (_, index) =>
      journeyListItem(twoLegs(index + 1)),
    );
    const trains = [
      loneTrain(50, { pinScheduledDeparture: '2026-09-17T18:00:00Z' }),
      loneTrain(51, { pinScheduledDeparture: '2026-09-17T08:00:00Z' }),
      loneTrain(52, { pinScheduledDeparture: '2026-09-17T12:00:00Z' }),
      loneTrain(53, { serviceDate: '2026-09-16', pinScheduledDeparture: '2026-09-16T23:00:00Z' }),
    ];
    const reads = homeDetailReads(journeys, trains, '2026-09-17');
    expect(reads.journeyIds).toHaveLength(MAX_HOME_JOURNEY_DETAILS - 2);
    expect(reads.trainIds).toEqual([51, 52]);
  });

  it('caps the reads', () => {
    const many = Array.from({ length: MAX_HOME_JOURNEY_DETAILS + 3 }, (_, index) =>
      journeyListItem(twoLegs(index + 1)),
    );
    expect(homeDetailReads(many, [loneTrain(50)], '2026-09-17').journeyIds).toHaveLength(MAX_HOME_JOURNEY_DETAILS);
  });
});

describe('a lone train read in full', () => {
  /** Kings Cross 10:00 to York 12:00 UK time, as `GET /Train/{id}` gives it. */
  function state(overrides: Parameters<typeof trainState>[4] = {}) {
    return trainState('KGX', 'YRK', '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { id: 5, ...overrides });
  }
  const split = (states: Map<number, ReturnType<typeof state>>, now: Date, train = loneTrain(5)) =>
    splitHomeJourneys([], new Map(), [train], now, states);
  const lateLeaving = state({
    journeyStops: [
      stop('KGX', {
        kind: 'Origin',
        scheduledDeparture: '2026-09-17T09:00:00Z',
        estimatedDeparture: '2026-09-17T09:20:00Z',
      }),
      stop('YRK', { kind: 'Terminate', scheduledArrival: '2026-09-17T11:00:00Z' }),
    ],
  });

  it('is not current while it is late leaving, though its timetabled departure has passed', () => {
    const result = split(new Map([[5, lateLeaving]]), at('2026-09-17T09:10:00Z'));
    expect(result.current).toBeNull();
    expect(result.upcoming[0]?.departure).toBe('2026-09-17T09:20:00Z');
    // Once it has gone, it is.
    expect(split(new Map([[5, lateLeaving]]), at('2026-09-17T09:21:00Z')).current).not.toBeNull();
  });

  it('ends as a journey leg does: the actual arrival, else timetable plus delay, else plus the grace', () => {
    const quiet = new Map([[5, state()]]);
    expect(split(quiet, at('2026-09-17T11:29:00Z')).current).not.toBeNull();
    expect(split(quiet, at('2026-09-17T11:31:00Z')).current).toBeNull();
    const late = new Map([[5, state({ status: 'en_route', delayMinutes: 40 })]]);
    expect(split(late, at('2026-09-17T11:39:00Z')).current).not.toBeNull();
    expect(split(late, at('2026-09-17T11:41:00Z')).current).toBeNull();
    const arrived = new Map([
      [
        5,
        state({
          status: 'en_route',
          delayMinutes: 40,
          journeyStops: [
            stop('KGX', { kind: 'Origin', scheduledDeparture: '2026-09-17T09:00:00Z' }),
            stop('YRK', {
              kind: 'Terminate',
              scheduledArrival: '2026-09-17T11:00:00Z',
              actualArrival: '2026-09-17T10:58:00Z',
            }),
          ],
        }),
      ],
    ]);
    expect(split(arrived, at('2026-09-17T10:59:00Z')).current).toBeNull();
  });

  it('wins current from a lone train known only by its list row, its arrival being known', () => {
    const result = splitHomeJourneys(
      [],
      new Map(),
      [loneTrain(5), loneTrain(6, { status: 'en_route', pinScheduledDeparture: '2026-09-17T09:30:00Z' })],
      at('2026-09-17T10:00:00Z'),
      new Map([[5, state()]]),
    );
    expect(ids(result.current ? [result.current] : [])).toEqual(['t5']);
    expect(ids(result.alsoInProgress)).toEqual(['t6']);
  });

  it('falls back to its list row when its read failed (no state)', () => {
    // The timetabled departure has passed: by the row alone it is under way.
    const result = split(new Map(), at('2026-09-17T09:10:00Z'));
    const current = result.current;
    expect(current !== null && isLoneTrain(current) ? current.leg : 'none').toBeNull();
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
    const split = splitHomeJourneys([journeyListItem(later), journeyListItem(sooner)], details, [], now);
    expect(ids(split.current ? [split.current] : [])).toEqual(['j2']);
    expect(ids(split.alsoInProgress)).toEqual(['j1']);
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
      [],
      now,
    );
    expect(split.current).toBeNull();
    expect(ids(split.upcoming)).toEqual(['j3', 'j2', 'j4']);
    expect(split.upcoming[0]?.departure).toBe('2026-09-17T12:00:00Z');
    expect(split.upcoming[2]?.departure).toBeNull();
  });

  it('lists a journey starting today whose detail failed to load as upcoming, not lost', () => {
    const split = splitHomeJourneys([journeyListItem(twoLegs(1))], new Map(), [], now);
    expect(ids(split.upcoming)).toEqual(['j1']);
  });

  it('has nothing to show for no journeys and no trains', () => {
    expect(splitHomeJourneys([], new Map(), [], now)).toEqual({ current: null, alsoInProgress: [], upcoming: [] });
  });

  describe('a train tracked on its own, as a one-leg journey', () => {
    it('is the current journey while under way: reported moving, or past its departure', () => {
      const moving = splitHomeJourneys(
        [],
        new Map(),
        [loneTrain(5, { status: 'en_route' })],
        at('2026-09-17T08:59:00Z'),
      );
      expect(ids(moving.current ? [moving.current] : [])).toEqual(['t5']);
      const departed = splitHomeJourneys([], new Map(), [loneTrain(5)], at('2026-09-17T09:01:00Z'));
      expect(ids(departed.current ? [departed.current] : [])).toEqual(['t5']);
      expect(departed.upcoming).toEqual([]);
    });

    it('carries the Track list badge status (`trackedTrainRowStatus`)', () => {
      const split = splitHomeJourneys([], new Map(), [loneTrain(5, { status: 'en_route', delayMinutes: 7 })], now);
      const current = split.current;
      expect(current !== null && isLoneTrain(current) ? current.status : null).toEqual(
        trackedTrainRowStatus(loneTrain(5, { status: 'en_route', delayMinutes: 7 }), '2026-09-17'),
      );
    });

    it('is upcoming before its departure', () => {
      const split = splitHomeJourneys([], new Map(), [loneTrain(5)], at('2026-09-17T08:00:00Z'));
      expect(split.current).toBeNull();
      expect(ids(split.upcoming)).toEqual(['t5']);
      expect(split.upcoming[0]?.departure).toBe('2026-09-17T09:00:00Z');
    });

    it("is gone once the api says it's travelled, however late it is running", () => {
      const split = splitHomeJourneys([], new Map(), [loneTrain(5, { status: 'en_route', travelled: true })], now);
      expect(split).toEqual({ current: null, alsoInProgress: [], upcoming: [] });
    });

    it('stays current past its timetabled arrival while the api says it is not travelled', () => {
      // The api applies the arrival rule and grace (`legIsOver`'s) and sends
      // the verdict; the list row has no arrival time to second-guess it.
      const split = splitHomeJourneys([], new Map(), [loneTrain(5, { delayMinutes: 40 })], at('2026-09-17T11:20:00Z'));
      expect(ids(split.current ? [split.current] : [])).toEqual(['t5']);
    });

    it('is over once completed, or cancelled and due to have left, as a last leg is', () => {
      expect(splitHomeJourneys([], new Map(), [loneTrain(5, { status: 'completed' })], now).current).toBeNull();
      const cancelled = loneTrain(5, { status: 'cancelled' });
      expect(splitHomeJourneys([], new Map(), [cancelled], now).current).toBeNull();
      // Before then it is listed, badged cancelled.
      const before = splitHomeJourneys([], new Map(), [cancelled], at('2026-09-17T08:00:00Z'));
      expect(ids(before.upcoming)).toEqual(['t5']);
    });

    it('with an older api (no `travelled`), drops a train from an earlier day as the Journeys page does', () => {
      const old = { ...loneTrain(5, { serviceDate: '2026-09-16', pinScheduledDeparture: '2026-09-16T09:00:00Z' }) };
      delete old.travelled;
      expect(splitHomeJourneys([], new Map(), [old], now)).toEqual({ current: null, alsoInProgress: [], upcoming: [] });
    });

    it('is ordered in with the upcoming journeys by day, then departure', () => {
      const soonToday = journeyDetail(3, [
        untrackedLeg(1, 'Home', 'Gym', '2026-09-17T12:00:00Z', '2026-09-17T12:30:00Z'),
      ]);
      const laterToday = journeyDetail(2, [
        untrackedLeg(1, 'Euston', 'Home', '2026-09-17T17:00:00Z', '2026-09-17T17:30:00Z'),
      ]);
      const nextWeekItem = journeyListItem(twoLegs(4), undefined, {
        firstServiceDate: '2026-09-24',
        serviceDate: '2026-09-24',
      });
      const trains = [
        loneTrain(6, { pinScheduledDeparture: '2026-09-17T14:00:00Z' }),
        loneTrain(7, { serviceDate: '2026-09-20', pinScheduledDeparture: '2026-09-20T08:00:00Z' }),
        loneTrain(8, { pinScheduledDeparture: '2026-09-17T11:00:00Z' }),
      ];
      const split = splitHomeJourneys(
        [nextWeekItem, journeyListItem(laterToday), journeyListItem(soonToday)],
        new Map([
          [2, laterToday],
          [3, soonToday],
        ]),
        trains,
        now,
      );
      expect(ids(split.upcoming)).toEqual(['t8', 'j3', 't6', 'j2', 't7', 'j4']);
    });

    it('gives way as current to a journey under way, whose arrival is known, and is listed under it', () => {
      const underWay = journeyDetail(1, [
        untrackedLeg(1, 'Home', 'Euston', '2026-09-17T09:30:00Z', '2026-09-17T10:30:00Z'),
      ]);
      const split = splitHomeJourneys(
        [journeyListItem(underWay)],
        new Map([[1, underWay]]),
        [loneTrain(5, { status: 'en_route' })],
        now,
      );
      expect(ids(split.current ? [split.current] : [])).toEqual(['j1']);
      expect(ids(split.alsoInProgress)).toEqual(['t5']);
    });
  });
});

describe('homeJourneyStatusGroup', () => {
  it('summarises the whole journey when read in full, not only the current train', () => {
    const detail = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'en_route', delayMinutes: 0 }),
      trainLeg(2, YRK, EDB, '2026-09-17T11:20:00Z', '2026-09-17T13:30:00Z', { status: 'cancelled' }),
    ]);
    expect(homeJourneyStatusGroup({ item: journeyListItem(detail), detail, departure: null })).toBe('partCancelled');
  });

  it("shows a missed connection the api found, as the journey page's badge does", () => {
    const detail = journeyDetail(1, [
      trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z', { status: 'en_route', delayMinutes: 0 }),
      {
        ...trainLeg(2, YRK, EDB, '2026-09-17T11:20:00Z', '2026-09-17T13:30:00Z', { status: 'awaiting_activation' }),
        connection: { gapMinutes: 3, minimumChangeMinutes: 5, state: 'missed' },
      },
    ]);
    expect(homeJourneyStatusGroup({ item: journeyListItem(detail), detail, departure: null })).toBe('connectionMissed');
    const atRisk = {
      ...detail,
      legs: detail.legs.map((leg, i) =>
        i === 1 ? { ...leg, connection: { gapMinutes: 6, minimumChangeMinutes: 5, state: 'atRisk' as const } } : leg,
      ),
    };
    expect(homeJourneyStatusGroup({ item: journeyListItem(atRisk), detail: atRisk, departure: null })).toBe(
      'connectionAtRisk',
    );
  });

  it("falls back to the list row's status when not read in full", () => {
    const detail = twoLegs(1, { status: 'en_route', delayMinutes: 5 });
    expect(homeJourneyStatusGroup({ item: journeyListItem(detail), detail: null, departure: null })).toBe('delayed');
  });
});

describe('shared journeys (DS-199)', () => {
  const now = at('2026-09-17T10:00:00Z');
  const sharedVia = [
    { groupId: 'g1', groupName: 'Family', addedByName: 'Olive', addedByTag: null, addedAt: '2026-09-16T08:00:00Z' },
  ];
  const followedFrom = {
    via: 'group' as const,
    ownerName: 'Olive',
    ownerTag: null,
    groupId: 'g1',
    groupName: 'Family',
    followedAt: '2026-09-16T08:00:00Z',
  };

  it('tells a shared journey the caller does not follow from their own and the ones they follow', () => {
    const item = journeyListItem(twoLegs(1));
    expect(isSharedNotFollowed(item)).toBe(false);
    expect(isSharedNotFollowed({ ...item, sharedVia })).toBe(true);
    expect(isSharedNotFollowed({ ...item, sharedVia, followedFrom })).toBe(false);
    expect(isSharedNotFollowed({ ...item, followedFrom })).toBe(false);
  });

  it('lists the ones under way first, then the ones to come soonest first, and leaves past ones out', () => {
    const underWay = twoLegs(1);
    const past = journeyDetail(2, [untrackedLeg(1, 'Home', 'Euston', '2026-09-17T07:00:00Z', '2026-09-17T07:30:00Z')]);
    const later = journeyDetail(3, [untrackedLeg(1, 'Euston', 'Home', '2026-09-17T17:00:00Z', '2026-09-17T17:30:00Z')]);
    const nextWeek = journeyListItem(twoLegs(4), undefined, {
      firstServiceDate: '2026-09-24',
      serviceDate: '2026-09-24',
    });
    const details = new Map([
      [1, underWay],
      [2, past],
      [3, later],
    ]);
    const items = [nextWeek, journeyListItem(later), journeyListItem(past), journeyListItem(underWay)].map((item) => ({
      ...item,
      sharedVia,
    }));
    const listed = sharedHomeJourneys(items, details, now);
    expect(ids(listed)).toEqual(['j1', 'j3', 'j4']);
    expect(listed[1]?.departure).toBe('2026-09-17T17:00:00Z');
  });
});
