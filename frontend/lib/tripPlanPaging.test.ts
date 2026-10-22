import { describe, expect, it } from 'vitest';
import { itineraryKey, listPlan, mergePage, pageEntries, pageQuery, startList, type PlanList } from './tripPlanPaging';
import { apiQuery } from './tripPlanTiming';
import { buildTripPlanQuery, type TripPlanQuery } from './tripPlan';
import type { TripPlanItinerary, TripPlanLeg, TripPlanResponse } from './types';

const base: TripPlanQuery = {
  originCrs: 'EUS',
  destinationCrs: 'MKC',
  waypointCrs: [],
  date: '2026-10-24',
  departAfter: '08:00',
  results: 'fastest',
};

type At = [string, number];

interface Ride {
  uid: string;
  departure: At;
  arrival: At;
  /** Live minutes late at the boarding call (the journey's times include it). */
  late?: number;
  serviceDate?: string;
}

function minutes([time, dayOffset]: At): number {
  const [h = 0, m = 0] = time.split(':').map(Number);
  return dayOffset * 1440 + h * 60 + m;
}

function clock(total: number): At {
  const day = Math.floor(total / 1440);
  const rest = total - day * 1440;
  return [`${String(Math.floor(rest / 60)).padStart(2, '0')}:${String(rest % 60).padStart(2, '0')}:00`, day];
}

function trainLeg(ride: Ride, serviceDate: string): Extract<TripPlanLeg, { kind: 'train' }> {
  return {
    kind: 'train',
    trainUid: ride.uid,
    serviceDate: ride.serviceDate ?? serviceDate,
    originCrs: 'EUS',
    destinationCrs: 'MKC',
    scheduledDeparture: ride.departure[0],
    departureDayOffset: ride.departure[1],
    scheduledArrival: ride.arrival[0],
    arrivalDayOffset: ride.arrival[1],
    ...(ride.late
      ? {
          live: {
            status: 'Late',
            cancelled: false,
            delayMinutes: ride.late,
            arrivalDelayMinutes: ride.late,
            reason: null,
            reasonSource: 'trust',
            platform: null,
            observedAt: null,
            interchangeFeasible: null,
          },
        }
      : {}),
  };
}

/** A one-segment page of single-train journeys on `serviceDate`; each
 * journey's times are the train's, plus its live delay. */
function page(serviceDate: string, ...rides: Ride[]): TripPlanResponse {
  const itineraries: TripPlanItinerary[] = rides.map((ride) => ({
    legs: [trainLeg(ride, serviceDate)],
    changeCount: 0,
    totalDurationMinutes: minutes(ride.arrival) - minutes(ride.departure),
  }));
  return {
    results: 'fastest',
    segments: [{ originCrs: 'EUS', destinationCrs: 'MKC', cappedByMaxChanges: false, itineraries }],
    journeys: rides.map((ride) => {
      const departure = clock(minutes(ride.departure) + (ride.late ?? 0));
      const arrival = clock(minutes(ride.arrival) + (ride.late ?? 0));
      return {
        changeCount: 0,
        departure: { time: departure[0], dayOffset: departure[1] },
        arrival: { time: arrival[0], dayOffset: arrival[1] },
        totalDurationMinutes: minutes(ride.arrival) - minutes(ride.departure),
      };
    }),
  };
}

const ride = (uid: string, departure: string, arrival: string, extra: Partial<Ride> = {}): Ride => ({
  uid,
  departure: [`${departure}:00`, 0],
  arrival: [`${arrival}:00`, 0],
  ...extra,
});

function started(query: TripPlanQuery, plan: TripPlanResponse): PlanList {
  const list = startList(query, apiQuery(query).date, plan);
  if (!list) throw new Error('expected a list');
  return list;
}

function uids(list: PlanList): string[] {
  return list.entries.map((entry) => {
    const leg = entry.parts[0]?.legs[0];
    return leg?.kind === 'train' ? leg.trainUid : '?';
  });
}

describe('itineraryKey', () => {
  it('is the same for the same journey whatever its live times', () => {
    const onTime = page('2026-10-24', ride('A', '08:00', '08:50'));
    const late = page('2026-10-24', ride('A', '08:00', '08:50', { late: 12 }));
    expect(itineraryKey(onTime.segments[0]!.itineraries)).toBe(itineraryKey(late.segments[0]!.itineraries));
  });

  it('tells apart another train, service date, boarding point and transfer', () => {
    const key = (legs: TripPlanLeg[]) => itineraryKey([{ legs, changeCount: 0, totalDurationMinutes: 0 }]);
    const a = trainLeg(ride('A', '08:00', '08:50'), '2026-10-24');
    const keys = new Set([
      key([a]),
      key([{ ...a, trainUid: 'B' }]),
      key([{ ...a, serviceDate: '2026-10-25' }]),
      key([{ ...a, originCrs: 'WFJ' }]),
      key([{ ...a, scheduledDeparture: '08:01:00' }]),
      key([a, { kind: 'transfer', mode: 'WALK', originCrs: 'MKC', destinationCrs: 'MKC', minutes: 5 }]),
    ]);
    expect(keys.size).toBe(6);
  });
});

describe('pageQuery (the cursors)', () => {
  it('pages later from just after the latest departure, earlier from just before the earliest arrival', () => {
    // An options page: the API orders it by changes, not time.
    const list = started(base, page('2026-10-24', ride('A', '08:45', '10:30'), ride('B', '08:20', '10:10')));
    expect(pageQuery(list, 'later')).toEqual({ ...base, departAfter: '08:46' });
    const { departAfter: _departAfter, ...rest } = base;
    expect(pageQuery(list, 'earlier')).toEqual({ ...rest, arriveBy: '10:09' });
  });

  it('keeps the rest of the search and drops a stale overnight mapping', () => {
    const query: TripPlanQuery = { ...base, results: 'options', viaCrs: ['RUG'], maxChanges: 2 };
    const list = started(query, page('2026-10-24', ride('A', '08:00', '08:50')));
    expect(pageQuery(list, 'later')).toMatchObject({ results: 'options', viaCrs: ['RUG'], maxChanges: 2 });
    expect(pageQuery(list, 'later').dayOffset).toBeUndefined();
  });

  it('pages from the live departure of a late first train, not its timetabled one', () => {
    // Timetabled 08:00, running 10 late: the planner searched on 08:10, so
    // "after 08:01" would find the same train again, for ever.
    const list = started(base, page('2026-10-24', ride('A', '08:00', '08:50', { late: 10 })));
    expect(pageQuery(list, 'later').departAfter).toBe('08:11');
    expect(pageQuery(list, 'earlier').arriveBy).toBe('08:59');
  });

  it('crosses midnight into the next calendar day, searched across both service dates', () => {
    const list = started(base, page('2026-10-24', { uid: 'A', departure: ['23:58:00', 0], arrival: ['00:40:00', 1] }));
    const later = pageQuery(list, 'later');
    expect(later).toMatchObject({ date: '2026-10-24', departAfter: '23:59' });
    const next = mergePage(
      list,
      'later',
      page('2026-10-24', { uid: 'B', departure: ['23:59:00', 0], arrival: ['00:45:00', 1] }),
      '2026-10-24',
    ).list;
    const afterMidnight = pageQuery(next, 'later');
    expect(afterMidnight).toMatchObject({ date: '2026-10-25', departAfter: '00:00' });
    const sent = new URLSearchParams(buildTripPlanQuery(apiQuery(afterMidnight)));
    expect(sent.get('date')).toBe('2026-10-24');
    expect(sent.get('departAfter')).toBe('00:00:00');
    expect(sent.get('dayOffset')).toBe('1');
    expect(sent.get('acrossMidnight')).toBe('true');
  });

  it("counts a page sent as the previous service date from the list's own date", () => {
    // The visitor searched 00:30 on the 25th: sent as the 24th, dayOffset 1.
    const query = { ...base, date: '2026-10-25', departAfter: '00:30' };
    const list = started(query, page('2026-10-24', { uid: 'A', departure: ['00:35:00', 1], arrival: ['01:20:00', 1] }));
    expect(pageQuery(list, 'later')).toMatchObject({ date: '2026-10-25', departAfter: '00:36' });
    expect(pageQuery(list, 'earlier')).toMatchObject({ date: '2026-10-25', arriveBy: '01:19' });
    // A later page from the 25th's own timetable lines up with it.
    const next = mergePage(list, 'later', page('2026-10-25', ride('B', '05:30', '06:20')), '2026-10-25').list;
    expect(next.entries.map((entry) => entry.departure)).toEqual([1475, 1440 + 330]);
    expect(pageQuery(next, 'later')).toMatchObject({ date: '2026-10-25', departAfter: '05:31' });
  });
});

describe('mergePage', () => {
  const first = () => started(base, page('2026-10-24', ride('B', '08:00', '08:50')));

  it('appends a later page below and prepends an earlier one above', () => {
    let list = first();
    list = mergePage(list, 'later', page('2026-10-24', ride('C', '08:30', '09:20')), '2026-10-24').list;
    list = mergePage(list, 'earlier', page('2026-10-24', ride('A', '07:30', '08:20')), '2026-10-24').list;
    expect(uids(list)).toEqual(['A', 'B', 'C']);
    expect(listPlan(list).segments[0]!.itineraries).toHaveLength(3);
    expect(listPlan(list).journeys).toHaveLength(3);
  });

  it('drops a journey already shown, even with new live times', () => {
    const list = first();
    const { list: next, added } = mergePage(
      list,
      'later',
      page('2026-10-24', ride('B', '08:00', '08:50', { late: 5 }), ride('C', '08:30', '09:20')),
      '2026-10-24',
    );
    expect(added).toBe(1);
    expect(uids(next)).toEqual(['B', 'C']);
  });

  it('keeps an earlier page to journeys leaving before the earliest shown', () => {
    // Arrives before 08:50 but leaves after 08:00: not "earlier".
    const { list, added } = mergePage(
      first(),
      'earlier',
      page('2026-10-24', ride('X', '08:05', '08:45'), ride('A', '07:30', '08:20')),
      '2026-10-24',
    );
    expect(added).toBe(1);
    expect(uids(list)).toEqual(['A', 'B']);
  });

  it('moves the cursor on even when nothing was added', () => {
    const list = first();
    const { list: next, added } = mergePage(
      list,
      'later',
      page('2026-10-24', ride('B', '08:00', '08:50', { late: 3 })),
      '2026-10-24',
    );
    expect(added).toBe(0);
    expect(pageQuery(next, 'later').departAfter).toBe('08:04');
    expect(next.laterDone).toBe(false);
  });

  it('marks a direction done when its page has no journey', () => {
    const empty: TripPlanResponse = { ...page('2026-10-24'), journeys: [] };
    const { list, added } = mergePage(first(), 'earlier', empty, '2026-10-24');
    expect(added).toBe(0);
    expect(list.earlierDone).toBe(true);
    expect(list.laterDone).toBe(false);
    expect(uids(list)).toEqual(['B']);
  });

  it('carries a provisional page into the list', () => {
    const { list } = mergePage(
      first(),
      'later',
      { ...page('2026-10-24', ride('C', '08:30', '09:20')), provisional: true },
      '2026-10-24',
    );
    expect(listPlan(list).provisional).toBe(true);
  });
});

describe('startList / pageEntries', () => {
  it('is null for a search with no journey, or one whose segments and journeys disagree', () => {
    expect(startList(base, '2026-10-24', { ...page('2026-10-24'), journeys: [] })).toBeNull();
    const plan = page('2026-10-24', ride('A', '08:00', '08:50'));
    const { journeys: _journeys, ...older } = plan;
    expect(pageEntries(older, '2026-10-24', '2026-10-24')).toBeNull();
  });

  it('keeps each journey of a waypoint plan whole, across its segments', () => {
    const one = page('2026-10-24', ride('A', '08:00', '08:50'), ride('B', '08:20', '09:10'));
    const two = page('2026-10-24', ride('C', '09:00', '09:40'), ride('D', '09:20', '10:00'));
    const plan: TripPlanResponse = {
      ...one,
      segments: [one.segments[0]!, { ...two.segments[0]!, originCrs: 'MKC', destinationCrs: 'BHM' }],
      journeys: [
        { ...one.journeys![0]!, arrival: two.journeys![0]!.arrival },
        { ...one.journeys![1]!, arrival: two.journeys![1]!.arrival },
      ],
    };
    const list = started(base, plan);
    expect(list.entries.map((entry) => entry.parts.length)).toEqual([2, 2]);
    expect(pageQuery(list, 'earlier').arriveBy).toBe('09:39');
    expect(listPlan(list).segments[1]!.itineraries).toEqual(two.segments[0]!.itineraries);
  });
});
