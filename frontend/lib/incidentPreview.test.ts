import { describe, expect, it } from 'vitest';
import {
  incidentPreviewAlt,
  incidentPreviewCard,
  incidentPreviewDescription,
  incidentPreviewImage,
  incidentStatusCard,
  incidentPreviewVersion,
  previewTitle,
  routePhrase,
} from './incidentPreview';
import { listPhrase } from './livePreview';
import type { IncidentDetail } from './types';

// 30 Oct 2026 is after the clocks went back, so UK time is UTC.
const NOW = Date.parse('2026-10-30T14:00:00Z');

const TOCS = new Map([
  ['VT', 'Avanti West Coast'],
  ['AW', 'Transport for Wales'],
  ['SN', 'Southern'],
]);

/** Shaped like a live incident: a real-time one between two stations,
 * open-ended, on two lines. */
function incident(overrides: Partial<IncidentDetail> = {}): IncidentDetail {
  return {
    incidentId: 'DD7A9B809383434BB74A06829BD86DCA',
    summary: 'Disruption between Chester and Llandudno Junction expected until 15:00',
    description: '<p>Lines closed.</p>',
    operators: ['VT', 'AW'],
    affectedStations: [],
    priority: 2,
    validityPeriods: [{ fromDate: '2026-10-30T13:50:00Z', toDate: null, isNow: true }],
    isPlanned: false,
    isCleared: false,
    firstSeenAt: '2026-10-30T13:53:09Z',
    fetchedAt: '2026-10-30T13:58:10Z',
    sourceRemovedAt: null,
    state: 'active',
    startsAt: null,
    validityEndedAt: null,
    currentlyAffectsLines: [
      { id: 'tfw-north-wales-coast', name: 'North Wales Coast Line' },
      { id: 'wcml-north-wales', name: 'West Coast Main Line: North Wales' },
    ],
    history: [
      {
        summary: 'Disruption between Chester and Llandudno Junction expected until 15:00',
        description: '<p>Lines closed.</p>',
        operators: ['VT', 'AW'],
        affectedStations: [],
        priority: 2,
        validityPeriods: [{ fromDate: '2026-10-30T13:50:00Z', toDate: null, isNow: true }],
        isPlanned: false,
        isCleared: false,
        recordedAt: '2026-10-30T13:58:10Z',
      },
    ],
    ...overrides,
  };
}

function card(overrides: Partial<IncidentDetail> = {}) {
  return incidentPreviewCard(incident(overrides), { operatorNames: TOCS, now: NOW });
}

/** A cleared incident with no lines left, shaped like a live one. */
const CLEARED: Partial<IncidentDetail> = {
  summary: 'CLEARED: Delays between Haywards Heath and Lewes ',
  operators: ['SN'],
  isCleared: true,
  state: 'cleared',
  validityEndedAt: '2026-10-30T12:56:00Z',
  validityPeriods: [{ fromDate: '2026-10-30T12:20:00Z', toDate: '2026-10-30T12:56:00Z', isNow: false }],
  currentlyAffectsLines: [],
};

describe('incidentPreviewCard', () => {
  it('draws a live incident: state, place, lines, operators and times', () => {
    expect(card()).toEqual({
      status: { tone: 'mild', label: 'Disruption in progress' },
      title: 'Disruption between Chester and Llandudno Junction expected until 15:00',
      lines: { names: ['North Wales Coast Line', 'West Coast Main Line: North Wales'], more: 0 },
      operators: 'Avanti West Coast and Transport for Wales',
      // The title already says where, whole.
      route: null,
      timing: ['Since Fri 30 Oct, 13:50', 'No end time given'],
      updated: 'Updated Fri 30 Oct, 13:58',
    });
  });

  it.each([
    [{ state: 'active' as const, isPlanned: false }, 'mild', 'Disruption in progress'],
    [{ state: 'active' as const, isPlanned: true }, 'planned', 'Planned work in progress'],
    [{ state: 'upcoming' as const, isPlanned: true }, 'planned', 'Upcoming planned work'],
    [{ state: 'upcoming' as const, isPlanned: false }, 'planned', 'Upcoming'],
    [{ state: 'cleared' as const, isCleared: true }, 'good', 'Cleared'],
    [{ state: 'ended' as const, isPlanned: true }, 'informational', 'Ended'],
  ])('bands %o as %s, in words', (overrides, tone, label) => {
    expect(card(overrides).status).toEqual({ tone, label });
  });

  it('works the state out itself against an api that sends none', () => {
    const stateless = (overrides: Partial<IncidentDetail>) => {
      const { state: _state, ...rest } = incident(overrides);
      return incidentPreviewCard(rest, { operatorNames: TOCS, now: NOW }).status.label;
    };
    expect(stateless({ isCleared: true })).toBe('Cleared');
    expect(stateless({ sourceRemovedAt: '2026-10-29T02:00:00Z' })).toBe('Ended');
    expect(stateless({ validityPeriods: [{ fromDate: '2026-11-02T00:00:00Z', toDate: null, isNow: true }] })).toBe(
      'Upcoming',
    );
  });

  it('draws a cleared incident with no lines: no "CLEARED:" twice, when it ran', () => {
    const cleared = card(CLEARED);
    expect(cleared.status).toEqual({ tone: 'good', label: 'Cleared' });
    expect(cleared.title).toBe('Delays between Haywards Heath and Lewes');
    expect(cleared.lines).toEqual({ names: [], more: 0 });
    expect(cleared.route).toBeNull();
    expect(cleared.timing).toEqual(['Ran Fri 30 Oct, 12:20 to 12:56']);
  });

  it('says when upcoming work starts and ends', () => {
    const upcoming = card({
      summary: 'Buses replace trains between Middlesbrough and Saltburn from Monday 25 to Thursday 28 January',
      isPlanned: true,
      state: 'upcoming',
      startsAt: '2027-01-25T00:00:00Z',
      validityPeriods: [{ fromDate: '2027-01-25T00:00:00Z', toDate: '2027-01-28T05:00:00Z', isNow: false }],
    });
    expect(upcoming.route).toBe('Between Middlesbrough and Saltburn');
    expect(upcoming.timing).toEqual(['Runs Mon 25 Jan, 00:00 to Thu 28 Jan, 05:00']);
  });

  it('gives the expected end of the current window when there is one', () => {
    expect(
      card({ validityPeriods: [{ fromDate: '2026-10-30T13:50:00Z', toDate: '2026-10-30T15:00:00Z', isNow: false }] })
        .timing,
    ).toEqual(['Since Fri 30 Oct, 13:50', 'Expected to end Fri 30 Oct, 15:00']);
  });

  it('says when the feed last listed an ended incident', () => {
    expect(card({ state: 'ended', sourceRemovedAt: '2026-10-29T02:00:00Z' }).timing).toEqual([
      'Last listed Thu 29 Oct, 02:00',
    ]);
  });

  it('shows three lines and counts the rest', () => {
    const lines = ['A', 'B', 'C', 'D', 'E'].map((name) => ({ id: name.toLowerCase(), name: `${name} Line` }));
    expect(card({ currentlyAffectsLines: lines }).lines).toEqual({ names: ['A Line', 'B Line', 'C Line'], more: 2 });
  });

  it('keeps a very long summary and line name whole: the image clamps them', () => {
    const long = `Disruption between ${'Somewhere Very Long '.repeat(12)}and Elsewhere`;
    const lineName = 'West Coast Main Line: '.repeat(6).trim();
    const drawn = card({ summary: long, currentlyAffectsLines: [{ id: 'x', name: lineName }] });
    expect(drawn.title).toBe(long.replace(/\s+/g, ' ').trim());
    // It may be clamped, so the place gets its own row.
    expect(drawn.route).toMatch(/^Between Somewhere Very Long/);
    expect(drawn.lines.names).toEqual([lineName]);
  });

  it('names many operators briefly, and an unknown one by its code', () => {
    expect(card({ operators: ['VT', 'AW', 'SN', 'ZZ', 'XX'] }).operators).toBe(
      'Avanti West Coast, Transport for Wales, Southern and 2 more',
    );
    expect(card({ operators: ['ZZ'] }).operators).toBe('ZZ');
  });

  it('leaves out what it has no data for', () => {
    const bare = card({
      summary: 'Signalling problem',
      operators: [],
      currentlyAffectsLines: [],
      validityPeriods: [],
      history: [],
      state: 'ended',
    });
    expect(bare.operators).toBeNull();
    expect(bare.route).toBeNull();
    expect(bare.lines).toEqual({ names: [], more: 0 });
    expect(bare.timing).toEqual([]);
    // No recorded change: when it was first seen.
    expect(bare.updated).toBe('Updated Fri 30 Oct, 13:53');
  });

  it('ignores dates it cannot read', () => {
    expect(card({ validityPeriods: [{ fromDate: 'soon', toDate: 'later', isNow: true }] }).timing).toEqual([
      'No end time given',
    ]);
  });

  it('names the affected stations when the summary names no place', () => {
    const drawn = incidentPreviewCard(
      incident({ summary: 'Signalling problem', affectedStations: ['KGX', 'EUS', 'PBO', 'YRK'] }),
      { operatorNames: TOCS, stationNames: new Map([['KGX', 'London Kings Cross']]), now: NOW },
    );
    expect(drawn.route).toBe('At London Kings Cross, EUS, PBO and 1 more');
  });
});

describe('routePhrase', () => {
  it.each([
    [
      'Disruption between Chester and Llandudno Junction expected until 15:00',
      'between Chester and Llandudno Junction',
    ],
    [
      'Lines reopened: disruption between Stratford (London) and Clapham Junction / Richmond expected until 15:00',
      'between Stratford (London) and Clapham Junction / Richmond',
    ],
    ['CLEARED: Delays between Haywards Heath and Lewes ', 'between Haywards Heath and Lewes'],
    ['Disruption through Rochester expected until 15:00', 'through Rochester'],
    ['Signal failure at Woking', 'at Woking'],
    [
      'Buses replace late night trains between Hebden Bridge / Todmorden and Blackburn from Monday 25',
      'between Hebden Bridge / Todmorden and Blackburn',
    ],
  ])('finds the place in %j', (summary, place) => {
    expect(routePhrase(summary)).toBe(place);
  });

  it('is null when the summary names no place', () => {
    expect(routePhrase('Signalling problem')).toBeNull();
    expect(routePhrase('Disruption expected until 15:00')).toBeNull();
    expect(routePhrase('Delays at 15:00')).toBeNull();
  });
});

describe('previewTitle and listPhrase', () => {
  it('drops a leading "CLEARED:" but never empties the title', () => {
    expect(previewTitle('CLEARED:  Delays  at Lewes ')).toBe('Delays at Lewes');
    expect(previewTitle('CLEARED:')).toBe('CLEARED:');
  });

  it('joins lists in plain English', () => {
    expect(listPhrase([])).toBe('');
    expect(listPhrase(['A'])).toBe('A');
    expect(listPhrase(['A', 'B'])).toBe('A and B');
    expect(listPhrase(['A', 'B', 'C'])).toBe('A, B and C');
    expect(listPhrase(['A', 'B', 'C', 'D'])).toBe('A, B, C and 1 more');
  });
});

describe('incidentPreviewDescription', () => {
  it('says what, where and who, and not the state', () => {
    const live = incidentPreviewDescription(incident(), TOCS);
    expect(live).toBe(
      'Disruption between Chester and Llandudno Junction affecting Avanti West Coast and Transport for Wales.',
    );
    // Still true once cleared.
    expect(incidentPreviewDescription(incident({ isCleared: true, state: 'cleared' }), TOCS)).toBe(live);
  });

  it('calls planned work planned work', () => {
    expect(incidentPreviewDescription(incident({ isPlanned: true, operators: [] }), TOCS)).toBe(
      'Planned work between Chester and Llandudno Junction.',
    );
  });

  it('falls back to the lines, then to the summary', () => {
    expect(incidentPreviewDescription(incident({ summary: 'Signalling problem', operators: [] }), TOCS)).toBe(
      'Disruption on North Wales Coast Line and West Coast Main Line: North Wales.',
    );
    expect(
      incidentPreviewDescription(
        incident({ summary: 'Signalling problem.', operators: [], currentlyAffectsLines: [] }),
        TOCS,
      ),
    ).toBe('Disruption: Signalling problem.');
  });
});

describe('incidentPreviewAlt', () => {
  it('names the incident and its state', () => {
    expect(incidentPreviewAlt(card(CLEARED), 'Distant Signal')).toBe(
      'Delays between Haywards Heath and Lewes (cleared) on Distant Signal',
    );
  });
});

describe('incidentPreviewVersion', () => {
  it('is stable for the same incident and ignores the poll time', () => {
    expect(incidentPreviewVersion(incident())).toBe(incidentPreviewVersion(incident({ fetchedAt: 'later' })));
    expect(incidentPreviewVersion(incident())).toMatch(/^[0-9a-f]{8}$/);
  });

  it('changes when what the card shows changes', () => {
    const base = incidentPreviewVersion(incident());
    expect(incidentPreviewVersion(incident(CLEARED))).not.toBe(base);
    expect(incidentPreviewVersion(incident({ state: 'ended' }))).not.toBe(base);
    expect(incidentPreviewVersion(incident({ currentlyAffectsLines: [] }))).not.toBe(base);
  });

  it('is part of the image URL, with the id encoded', () => {
    expect(incidentPreviewImage(incident({ incidentId: 'a/b' }), 'Alt')).toEqual({
      url: expect.stringMatching(/^\/incidents\/a%2Fb\/card\.png\?v=[0-9a-f]{8}$/) as unknown,
      alt: 'Alt',
      width: 1200,
      height: 630,
      type: 'image/png',
    });
  });
});

describe('incidentStatusCard', () => {
  it('puts the place first, then who and when, and leaves out what is missing', () => {
    const long = `Disruption between Chester and Llandudno Junction ${'with a very long explanation '.repeat(4)}`;
    expect(incidentStatusCard(card({ summary: long }))).toEqual({
      band: { tone: 'mild', label: 'Disruption in progress', aside: 'Updated Fri 30 Oct, 13:58' },
      title: long.trim(),
      lead: 'Between Chester and Llandudno Junction with a very long explanation with a very long explanation with a very long explanation with a very long explanation',
      chips: { names: ['North Wales Coast Line', 'West Coast Main Line: North Wales'], more: 0 },
      rows: ['Avanti West Coast and Transport for Wales', 'Since Fri 30 Oct, 13:50 · No end time given (UK time)'],
    });
    expect(incidentStatusCard(card({ operators: [], validityPeriods: [], state: 'ended' })).rows).toEqual([]);
  });
});
