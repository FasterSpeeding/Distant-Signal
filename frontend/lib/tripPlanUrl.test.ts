import { describe, expect, it } from 'vitest';
import { parsePlanSearchParams, planPageSearch } from './tripPlanUrl';
import type { TripPlanQuery } from './tripPlan';

function toRaw(search: string): Record<string, string> {
  return Object.fromEntries(new URLSearchParams(search));
}

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

  it('drops malformed values instead of passing them to the API', () => {
    expect(
      parsePlanSearchParams({
        origin: 'London',
        destination: 'tiploc:',
        date: 'tomorrow',
        departAfter: '25:00',
        results: 'all',
        maxChanges: '7',
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
});
