import { describe, expect, it } from 'vitest';
import {
  NO_STOP_FILTERS,
  countStopFilters,
  parseStopFilterParams,
  setStopFilterParams,
  stopFilterEntries,
  stopFilterErrors,
  stopFiltersSummary,
  type TrainStopFilters,
} from './trainStopFilters';
import { TRAIN_URL_PARAMS, trainsRedirect } from './trainSearch';

const filters = (partial: Partial<TrainStopFilters>): TrainStopFilters => ({ ...NO_STOP_FILTERS, ...partial });

describe('parseStopFilterParams', () => {
  it('reads the four lists, normalized', () => {
    const params = new URLSearchParams('calls_at=swi,group:lon&via=did&avoid=bhm,tiploc:stafbus&avoid_stop=oxf');
    expect(parseStopFilterParams(params)).toEqual({
      callsAt: ['SWI', 'group:LON'],
      via: ['DID'],
      avoid: ['BHM', 'tiploc:STAFBUS'],
      avoidStop: ['OXF'],
      previouslyCallsAt: [],
      previouslyStopsAt: [],
      startsAt: [],
      terminatesAt: [],
    });
  });

  it('reads the two Previously lists like Call at', () => {
    const params = new URLSearchParams('previously_calls_at=eus,group:lon&previously_stops_at=cre,cre,AAA,BBB,CCC');
    const parsed = parseStopFilterParams(params);
    expect(parsed.previouslyCallsAt).toEqual(['EUS', 'group:LON']);
    expect(parsed.previouslyStopsAt).toEqual(['CRE', 'AAA', 'BBB']);
  });

  it('reads Starts at from origin, so an old link keeps its exact meaning, and Terminates at', () => {
    const old = parseStopFilterParams(new URLSearchParams('station=MAN&origin=eus&terminates_at=wat'));
    expect(old.startsAt).toEqual(['EUS']);
    expect(old.terminatesAt).toEqual(['WAT']);
    // Not a superset: Previously calls at stays empty.
    expect(old.previouslyCallsAt).toEqual([]);
    // One station code each; anything else is dropped.
    const bad = parseStopFilterParams(new URLSearchParams('origin=EUSX&terminates_at=group:LON'));
    expect(bad.startsAt).toEqual([]);
    expect(bad.terminatesAt).toEqual([]);
  });

  it('drops what it cannot read and cuts each list to its cap', () => {
    const params = new URLSearchParams(
      'calls_at=AAA,AAA,BBB,CCC,DDD&via=12,!!&avoid=AAA,BBB,CCC,DDD,EEE,FFF,GGG,HHH,III&avoid_stop=group:LON',
    );
    const parsed = parseStopFilterParams(params);
    // Twice in a row is listed once; three at most.
    expect(parsed.callsAt).toEqual(['AAA', 'BBB', 'CCC']);
    expect(parsed.via).toEqual([]);
    expect(parsed.avoid).toHaveLength(8);
    // The avoid lists take no groups, as the planner's pickers.
    expect(parsed.avoidStop).toEqual([]);
  });

  it('is empty with no params', () => {
    expect(parseStopFilterParams(null)).toEqual(NO_STOP_FILTERS);
    expect(parseStopFilterParams(new URLSearchParams('station=RDG'))).toEqual(NO_STOP_FILTERS);
  });
});

describe('stopFilterEntries', () => {
  it('round-trips through the URL', () => {
    const set = filters({
      callsAt: ['SWI', 'group:LON'],
      avoidStop: ['OXF'],
      previouslyCallsAt: ['EUS'],
      previouslyStopsAt: ['CRE', 'group:LON'],
      startsAt: ['EUS'],
      terminatesAt: ['WAT'],
    });
    const entries = stopFilterEntries(set);
    expect(entries).toEqual([
      ['calls_at', 'SWI,group:LON'],
      ['avoid_stop', 'OXF'],
      ['previously_calls_at', 'EUS'],
      ['previously_stops_at', 'CRE,group:LON'],
      ['origin', 'EUS'],
      ['terminates_at', 'WAT'],
    ]);
    expect(parseStopFilterParams(new URLSearchParams(entries))).toEqual(set);
  });

  it('names only the lists that are set', () => {
    expect(stopFilterEntries(NO_STOP_FILTERS)).toEqual([]);
    expect(countStopFilters(filters({ via: ['DID'], avoid: ['BHM', 'CRE'] }))).toBe(2);
    expect(setStopFilterParams(filters({ via: ['DID'], avoid: ['BHM'] }))).toEqual(['via', 'avoid']);
    expect(setStopFilterParams(filters({ previouslyCallsAt: ['EUS'], previouslyStopsAt: ['CRE'] }))).toEqual([
      'previously_calls_at',
      'previously_stops_at',
    ]);
    expect(setStopFilterParams(filters({ startsAt: ['EUS'], terminatesAt: ['WAT'] }))).toEqual([
      'origin',
      'terminates_at',
    ]);
  });

  it('are /trains URL params, so switching view clears them', () => {
    for (const param of [
      'calls_at',
      'via',
      'avoid',
      'avoid_stop',
      'previously_calls_at',
      'previously_stops_at',
      'origin',
      'terminates_at',
    ]) {
      expect(TRAIN_URL_PARAMS).toContain(param);
    }
  });

  it('survive an old-shape link redirect unchanged', () => {
    const redirect = trainsRedirect(new URLSearchParams('station=RDG&stops_at=PAD&avoid=DID'), '2026-10-30');
    expect(redirect).toEqual({ location: '/trains?avoid=DID&from=RDG&to=PAD', status: 308 });
    expect(trainsRedirect(new URLSearchParams('station=RDG&via=DID'), '2026-10-30')).toBeNull();
  });
});

describe('stopFiltersSummary', () => {
  it('lists each set option, by name where known', () => {
    const names = new Map([['SWI', 'Swindon']]);
    expect(
      stopFiltersSummary(filters({ callsAt: ['SWI'], via: ['DID'], avoid: ['BHM'], avoidStop: ['OXF'] }), names),
    ).toBe("Call at Swindon · Pass through DID · Avoid BHM · Don't stop at OXF");
    expect(stopFiltersSummary(NO_STOP_FILTERS)).toBe('');
    expect(
      stopFiltersSummary(filters({ previouslyCallsAt: ['SWI'], previouslyStopsAt: ['DID'], via: ['BHM'] }), names),
    ).toBe('Previously calls at Swindon · Previously stops at DID · Pass through BHM');
    expect(stopFiltersSummary(filters({ startsAt: ['SWI'], terminatesAt: ['OXF'], callsAt: ['DID'] }), names)).toBe(
      'Starts at Swindon · Terminates at OXF · Call at DID',
    );
  });
});

describe('stopFilterErrors', () => {
  const between = {
    stations: [['RDG', 'where you get on'] as [string, string], ['pad', 'where you get off'] as [string, string]],
    names: new Map<string, string>(),
  };

  it('needs no fixing when nothing clashes', () => {
    expect(stopFilterErrors(filters({ callsAt: ['SWI'], via: ['DID'], avoidStop: ['DID'] }), between)).toEqual({});
  });

  it('refuses to avoid a searched station', () => {
    expect(stopFilterErrors(filters({ avoid: ['PAD'] }), between)).toEqual({
      avoid: "PAD is where you get off, so it can't be avoided.",
    });
    expect(stopFilterErrors(filters({ avoidStop: ['RDG'] }), between).avoidStop).toBe(
      "RDG is where you get on, so it can't be avoided.",
    );
  });

  it('refuses a searched station as a stop or pass-through', () => {
    expect(stopFilterErrors(filters({ callsAt: ['RDG'] }), between).callsAt).toBe(
      'RDG is where you get on: list a station elsewhere on the route.',
    );
    expect(stopFilterErrors(filters({ via: ['PAD'] }), between).via).toBe(
      'PAD is where you get off: list a station elsewhere on the route.',
    );
  });

  it('refuses lists that contradict each other', () => {
    expect(stopFilterErrors(filters({ callsAt: ['SWI'], avoidStop: ['SWI'] }), between).callsAt).toBe(
      'SWI is also in "Avoid completely" or "Don\'t stop at": a train can\'t both stop there and not.',
    );
    expect(stopFilterErrors(filters({ via: ['DID'], avoid: ['DID'] }), between).via).toBe(
      'DID is also in "Avoid completely": a train can\'t both pass through it and avoid it.',
    );
  });

  it('refuses a searched station as a Previously entry', () => {
    expect(stopFilterErrors(filters({ previouslyCallsAt: ['RDG'] }), between).previouslyCallsAt).toBe(
      'RDG is where you get on: list a station elsewhere on the route.',
    );
    expect(stopFilterErrors(filters({ previouslyStopsAt: ['PAD'] }), between).previouslyStopsAt).toBe(
      'PAD is where you get off: list a station elsewhere on the route.',
    );
  });

  it('lets a Previously entry be avoided after the station, but not on the arrivals board', () => {
    const set = filters({ previouslyStopsAt: ['SWI'], avoid: ['SWI'] });
    expect(stopFilterErrors(set, between)).toEqual({});
    expect(stopFilterErrors(set, { ...between, arrivals: true }).previouslyStopsAt).toBe(
      'SWI is also in "Avoid completely" or "Don\'t stop at": a train can\'t both stop there and not.',
    );
  });

  it('takes Starts at and Terminates at as station codes, the station searched included', () => {
    expect(stopFilterErrors(filters({ startsAt: ['RDG'], terminatesAt: ['PAD'] }), between)).toEqual({});
    expect(stopFilterErrors(filters({ startsAt: ['RD'], terminatesAt: ['pa'] }), between)).toEqual({
      startsAt: 'Must be a 3-letter station code',
      terminatesAt: 'Must be a 3-letter station code',
    });
  });

  it('refuses an end of the train the avoid lists rule out, where they look at it', () => {
    const departures = {
      stations: [['RDG', 'the station on this board'] as [string, string]],
      names: new Map<string, string>(),
    };
    const set = filters({ startsAt: ['SWI'], terminatesAt: ['OXF'], avoid: ['SWI'], avoidStop: ['OXF'] });
    // Departures: the lists look on to the end of the train.
    expect(stopFilterErrors(set, departures)).toEqual({
      terminatesAt: 'OXF is also in "Avoid completely" or "Don\'t stop at": a train can\'t both end there and not.',
    });
    // Arrivals: they look back to where it starts.
    expect(stopFilterErrors(set, { ...departures, arrivals: true })).toEqual({
      startsAt: 'SWI is also in "Avoid completely" or "Don\'t stop at": a train can\'t both start there and not.',
    });
    // Between two stations: neither end is ridden.
    expect(stopFilterErrors(set, { ...between, between: true })).toEqual({});
  });

  it('refuses the same station twice in a row', () => {
    expect(stopFilterErrors(filters({ via: ['DID', 'DID'] }), between).via).toBe(
      'DID is listed twice in a row: list it once.',
    );
  });
});
