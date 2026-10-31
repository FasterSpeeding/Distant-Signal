import { describe, expect, it } from 'vitest';
import { directionHeading, frequencyText, groupLinesByMode, transitNetworkSummary } from './transitModes';

describe('transitNetworkSummary', () => {
  it('names the modes of the TfL network only', () => {
    expect(transitNetworkSummary('tfl')).toBe('Underground, DLR and London Trams');
    expect(transitNetworkSummary('metrolink')).toBeNull();
  });
});

describe('groupLinesByMode', () => {
  it('keeps a single-mode network as one unlabelled list', () => {
    const lines = [{ mode: 'tram' }, { mode: 'tram' }];
    expect(groupLinesByMode(lines)).toEqual([{ mode: 'tram', label: null, lines }]);
    expect(groupLinesByMode([{}, {}])).toEqual([{ mode: null, label: null, lines: [{}, {}] }]);
  });

  it('groups a mixed network by mode, in line order', () => {
    const victoria = { id: 'v', mode: 'tube' };
    const dlr = { id: 'd', mode: 'dlr' };
    const central = { id: 'c', mode: 'tube' };
    const tram = { id: 't', mode: 'tram' };
    expect(groupLinesByMode([victoria, dlr, central, tram])).toEqual([
      { mode: 'tube', label: 'Underground', lines: [victoria, central] },
      { mode: 'dlr', label: 'DLR', lines: [dlr] },
      { mode: 'tram', label: 'Trams', lines: [tram] },
    ]);
  });
});

describe('frequencyText', () => {
  it('says how often, or how many', () => {
    expect(frequencyText({ departures: 30, minGapMinutes: 2, maxGapMinutes: 4 })).toBe('Every 2 to 4 minutes');
    expect(frequencyText({ departures: 12, minGapMinutes: 5, maxGapMinutes: 5 })).toBe('Every 5 minutes');
    expect(frequencyText({ departures: 2, minGapMinutes: 1, maxGapMinutes: 1 })).toBe('Every 1 minute');
    expect(frequencyText({ departures: 1, minGapMinutes: null, maxGapMinutes: null })).toBe('One departure');
    expect(frequencyText({ departures: 3, minGapMinutes: null, maxGapMinutes: null })).toBe('3 departures');
  });
});

describe('directionHeading', () => {
  const stations = [
    { id: 'A', name: 'Brixton' },
    { id: 'B', name: 'Walthamstow Central' },
  ];
  it('prefers the headsign, then the last station', () => {
    expect(directionHeading({ direction: 'outbound', headsign: 'Aldgate', stations })).toBe('Towards Aldgate');
    expect(directionHeading({ direction: 'outbound', headsign: null, stations })).toBe('Towards Walthamstow Central');
    expect(directionHeading({ direction: 'inbound', headsign: null, stations: [] })).toBe('The other way');
  });
});
