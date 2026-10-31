import { describe, expect, it } from 'vitest';
import { parsePlanSearchParams, planPageSearch } from './tripPlanUrl';
import type { TripPlanQuery } from './tripPlan';

function toRaw(search: string): Record<string, string> {
  return Object.fromEntries(new URLSearchParams(search));
}

describe('the operator filter in the /plan URL', () => {
  const base: TripPlanQuery = {
    originCrs: 'EUS',
    destinationCrs: 'GLA',
    waypointCrs: [],
    date: '2026-10-08',
    results: 'fastest',
  };

  it('round-trips "only these" and "avoid these"', () => {
    const only = planPageSearch({ ...base, operatorMode: 'only', operators: ['VT', 'group:GTR', 'ds:DLR'] });
    expect(new URLSearchParams(only).get('operator')).toBe('VT,group:GTR,ds:DLR');
    expect(parsePlanSearchParams(toRaw(only))).toMatchObject({
      operatorMode: 'only',
      operators: ['VT', 'group:GTR', 'ds:DLR'],
    });
    const avoid = planPageSearch({ ...base, operatorMode: 'avoid', operators: ['LT'] });
    expect(parsePlanSearchParams(toRaw(avoid))).toMatchObject({ operatorMode: 'avoid', operators: ['LT'] });
  });

  it('normalizes entries, drops malformed ones, and prefers operator over avoidOperator', () => {
    const initial = parsePlanSearchParams({ operator: 'vt,bad!,GROUP:gtr', avoidOperator: 'LT' });
    expect(initial).toMatchObject({ operatorMode: 'only', operators: ['VT', 'group:GTR'] });
    expect(parsePlanSearchParams({ operator: 'bad!', avoidOperator: 'lt' })).toMatchObject({
      operatorMode: 'avoid',
      operators: ['LT'],
    });
    const none = parsePlanSearchParams({ operator: '' });
    expect(none.operatorMode).toBeUndefined();
    expect(none.operators).toBeUndefined();
  });
});

describe('planPageSearch / parsePlanSearchParams', () => {
  it('round-trips a full search, advanced options included', () => {
    const query: TripPlanQuery = {
      originCrs: 'eus',
      destinationCrs: 'gla',
      waypointCrs: ['PRE'],
      date: '2026-10-08',
      departAfter: '09:15',
      results: 'options',
      viaCrs: ['STA', 'CRE'],
      avoidCrs: ['BHM'],
      avoidStopCrs: ['tiploc:keswbus'],
      avoidChangeCrs: ['WVH', 'LIV'],
      maxChanges: 4,
    };
    const search = planPageSearch(query);
    const params = new URLSearchParams(search);
    expect(params.get('via')).toBe('STA,CRE');
    expect(params.get('avoidStop')).toBe('tiploc:KESWBUS');
    expect(params.get('maxChanges')).toBe('4');
    // Form times, not the API's HH:MM:SS.
    expect(params.get('departAfter')).toBe('09:15');

    expect(parsePlanSearchParams(toRaw(search))).toEqual({
      ...query,
      originCrs: 'EUS',
      destinationCrs: 'GLA',
      avoidStopCrs: ['tiploc:KESWBUS'],
    });
  });

  it('omits unset options and the default results', () => {
    const search = planPageSearch({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: [],
      date: '2026-10-08',
      results: 'fastest',
    });
    expect(search).toBe('origin=EUS&destination=MKC&date=2026-10-08');
  });

  it('keeps maxChanges=0', () => {
    const search = planPageSearch({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: [],
      date: '2026-10-08',
      results: 'options',
      maxChanges: 0,
    });
    expect(parsePlanSearchParams(toRaw(search)).maxChanges).toBe(0);
  });

  it('keeps a two-digit maxChanges up to the API ceiling of 12', () => {
    for (const maxChanges of [10, 12]) {
      const search = planPageSearch({
        originCrs: 'EUS',
        destinationCrs: 'MKC',
        waypointCrs: [],
        date: '2026-10-08',
        results: 'options',
        maxChanges,
      });
      expect(parsePlanSearchParams(toRaw(search)).maxChanges).toBe(maxChanges);
    }
    expect(parsePlanSearchParams({ maxChanges: '13' }).maxChanges).toBeUndefined();
    expect(parsePlanSearchParams({ maxChanges: '012' }).maxChanges).toBeUndefined();
  });

  it('drops malformed values instead of passing them to the API', () => {
    expect(
      parsePlanSearchParams({
        origin: 'London',
        destination: 'tiploc:',
        date: 'tomorrow',
        departAfter: '25:00',
        results: 'all',
        maxChanges: '13',
        via: 'ZZZZ',
        avoid: '',
      }),
    ).toEqual({});
  });

  it('keeps vias in order, at most 3, never twice in a row, allowing bus stops', () => {
    expect(parsePlanSearchParams({ via: 'sta,STA,cre,sta,bhm' }).viaCrs).toEqual(['STA', 'CRE', 'STA']);
    expect(parsePlanSearchParams({ via: 'sta,tiploc:keswbus,TIPLOC:KESWBUS,cre' }).viaCrs).toEqual([
      'STA',
      'tiploc:KESWBUS',
      'CRE',
    ]);
  });

  it('dedupes and caps the avoid lists at 8, allowing bus stops', () => {
    const avoid = 'AAA,BBB,AAA,CCC,DDD,EEE,FFF,GGG,HHH,III,tiploc:X1';
    expect(parsePlanSearchParams({ avoid }).avoidCrs).toEqual(['AAA', 'BBB', 'CCC', 'DDD', 'EEE', 'FFF', 'GGG', 'HHH']);
    expect(parsePlanSearchParams({ avoidChange: 'tiploc:x1' }).avoidChangeCrs).toEqual(['tiploc:X1']);
  });

  it('reads the first of a repeated parameter', () => {
    expect(parsePlanSearchParams({ origin: ['EUS', 'MKC'] }).originCrs).toBe('EUS');
  });

  it('keeps station groups in via and waypoints, normalized, and drops malformed ones', () => {
    const initial = parsePlanSearchParams({
      via: 'group:lon,STA,group:',
      waypoints: 'GROUP:LON,YRK,group:x',
      avoid: 'group:LON,BHM',
    });
    expect(initial.viaCrs).toEqual(['group:LON', 'STA']);
    expect(initial.waypointCrs).toEqual(['group:LON', 'YRK']);
    // The form's avoid lists do not offer groups.
    expect(initial.avoidCrs).toEqual(['BHM']);
    const search = planPageSearch({
      originCrs: 'CBG',
      destinationCrs: 'BTN',
      waypointCrs: ['group:LON'],
      viaCrs: ['group:LON'],
      date: '2026-10-08',
      results: 'fastest',
    });
    expect(parsePlanSearchParams(toRaw(search))).toMatchObject({ waypointCrs: ['group:LON'], viaCrs: ['group:LON'] });
  });

  it('round-trips an arrive-by search, never writing both times', () => {
    const query: TripPlanQuery = {
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      waypointCrs: [],
      date: '2026-10-24',
      arriveBy: '00:30',
      results: 'fastest',
    };
    const search = planPageSearch({ ...query, departAfter: '08:00' });
    // The visitor's own calendar date and time: the overnight mapping is
    // the API request's business, not the address bar's.
    expect(search).toBe('origin=EUS&destination=MKC&date=2026-10-24&arriveBy=00%3A30');
    expect(parsePlanSearchParams(toRaw(search))).toEqual({
      originCrs: 'EUS',
      destinationCrs: 'MKC',
      date: '2026-10-24',
      arriveBy: '00:30',
    });
  });

  it('keeps arriveBy when a hand-edited URL has both times, and drops a malformed one', () => {
    expect(parsePlanSearchParams({ departAfter: '08:00', arriveBy: '09:00' })).toEqual({ arriveBy: '09:00' });
    expect(parsePlanSearchParams({ departAfter: '08:00', arriveBy: '25:00' })).toEqual({ departAfter: '08:00' });
    expect(parsePlanSearchParams({ arriveBy: '9am' })).toEqual({});
  });
});

describe('tram stops as From and To', () => {
  it('takes either NaPTAN spelling as origin or destination, upper-cased', () => {
    expect(parsePlanSearchParams({ origin: '9400ZZMAPIC', destination: 'man' })).toMatchObject({
      originCrs: '9400ZZMAPIC',
      destinationCrs: 'MAN',
    });
    expect(parsePlanSearchParams({ origin: '940gzzcrwmb', destination: '9400zzmavic' })).toMatchObject({
      originCrs: '940GZZCRWMB',
      destinationCrs: '9400ZZMAVIC',
    });
  });

  it('round-trips a tram station end', () => {
    const search = planPageSearch({
      originCrs: '9400ZZMAPIC',
      destinationCrs: 'MCV',
      waypointCrs: [],
      date: '2026-11-04',
      results: 'fastest',
    });
    expect(toRaw(search).origin).toBe('9400ZZMAPIC');
    expect(parsePlanSearchParams(toRaw(search)).originCrs).toBe('9400ZZMAPIC');
  });

  it('drops a malformed code, and keeps stops out of the waypoint and avoid lists', () => {
    expect(parsePlanSearchParams({ origin: '9400', destination: '9401ZZMAPIC' })).toEqual({});
    expect(parsePlanSearchParams({ via: '9400ZZMAPIC', avoid: '940GZZCRWMB' })).toEqual({});
  });
});
