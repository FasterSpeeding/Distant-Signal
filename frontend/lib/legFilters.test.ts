import { describe, it, expect } from 'vitest';
import {
  NO_LEG_FILTERS,
  countLegFilters,
  legFilterErrors,
  legFiltersFromApi,
  legFiltersSummary,
  legFiltersToApi,
  operatorChoicesFromList,
} from './legFilters';
import { NO_STOP_FILTERS } from './trainStopFilters';

describe('legFilters', () => {
  it("round-trips the api's searchFilters, every list normalized as /trains sends it", () => {
    const api = {
      operator: 'sw, GW,sw',
      startsAt: 'wok',
      terminatesAt: 'VIC',
      previouslyCallsAt: 'WOK',
      previouslyStopsAt: 'SUR',
      callsAt: 'vxh,group:LON',
      via: 'QRP',
      avoid: 'EUS',
      avoidStop: 'EWR',
    };
    const filters = legFiltersFromApi(api);
    expect(filters.operators).toEqual(['SW', 'GW']);
    expect(filters.stops.callsAt).toEqual(['VXH', 'group:LON']);
    expect(filters.stops.startsAt).toEqual(['WOK']);
    expect(legFiltersToApi(filters)).toEqual({
      operator: 'SW,GW',
      startsAt: 'WOK',
      terminatesAt: 'VIC',
      previouslyCallsAt: 'WOK',
      previouslyStopsAt: 'SUR',
      callsAt: 'VXH,group:LON',
      via: 'QRP',
      avoid: 'EUS',
      avoidStop: 'EWR',
    });
    expect(countLegFilters(filters)).toBe(9);
  });

  it('is no filters for none, and none is null on the wire', () => {
    expect(legFiltersFromApi(null)).toEqual(NO_LEG_FILTERS);
    expect(legFiltersFromApi(undefined)).toEqual(NO_LEG_FILTERS);
    expect(legFiltersToApi(NO_LEG_FILTERS)).toBeNull();
    expect(countLegFilters(NO_LEG_FILTERS)).toBe(0);
  });

  it('names the set filters in one line', () => {
    const filters = { operators: ['SW'], stops: { ...NO_STOP_FILTERS, callsAt: ['RDG'], avoid: ['EUS'] } };
    expect(legFiltersSummary(filters, new Map([['RDG', 'Reading']]))).toBe('Operator SW · Call at Reading · Avoid EUS');
    expect(legFiltersSummary({ ...filters, operators: ['SW', 'GW'] })).toMatch(/^Operators SW, GW · /);
    expect(legFiltersSummary(NO_LEG_FILTERS)).toBe('');
  });

  it("checks the options against the leg's two stations, as the Between stations view does", () => {
    const errors = legFilterErrors(
      { operators: [], stops: { ...NO_STOP_FILTERS, avoid: ['CLJ'], callsAt: ['WAT'], terminatesAt: ['CLJ'] } },
      { origin: 'WAT', destination: 'CLJ' },
    );
    expect(errors.avoid).toBe("CLJ is where you get off, so it can't be avoided.");
    expect(errors.callsAt).toBe('WAT is where you get on: list a station elsewhere on the route.');
    // Between two stations a train may end where you get off, avoided or not.
    expect(errors.terminatesAt).toBeUndefined();
    expect(legFilterErrors(NO_LEG_FILTERS, { origin: null, destination: null })).toEqual({});
  });

  it("builds the Operator field's options as the /trains page does", () => {
    expect(
      operatorChoicesFromList([
        { code: 'SW', name: 'South Western Railway' },
        { code: 'QC', name: null },
        { code: 'GW', name: 'Great Western Railway' },
      ]),
    ).toEqual([
      { value: 'GW', label: 'GW · Great Western Railway' },
      { value: 'SW', label: 'SW · South Western Railway' },
      { value: 'QC', label: 'QC' },
    ]);
  });
});
