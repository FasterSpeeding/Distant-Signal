import { describe, expect, it } from 'vitest';
import {
  branchesHeading,
  directionHeading,
  frequencyText,
  groupLinesByMode,
  isHighFrequencyMode,
  networkModesPhrase,
  transitNetworkSummary,
} from './transitModes';

describe('transitNetworkSummary', () => {
  it('names the modes of the TfL network only', () => {
    expect(transitNetworkSummary('tfl')).toBe('Underground, DLR and London Trams');
    expect(transitNetworkSummary('metrolink')).toBeNull();
  });
});

describe('networkModesPhrase', () => {
  it("names a single-mode network's mode from the network when its lines have none", () => {
    expect(networkModesPhrase({ mode: 'tram', lines: [{}, {}] })).toBe('tram');
    expect(networkModesPhrase({ mode: 'tram', lines: [] })).toBe('tram');
  });

  it("lists a mixed network's modes in line order", () => {
    const lines = [{ mode: 'tube' }, { mode: 'dlr' }, { mode: 'tube' }, { mode: 'tram' }];
    expect(networkModesPhrase({ mode: 'metro', lines })).toBe('Tube, DLR and tram');
    expect(networkModesPhrase({ mode: 'metro', lines: lines.slice(0, 2) })).toBe('Tube and DLR');
  });

  it('is null when a mode has no word, rather than guess', () => {
    expect(networkModesPhrase({ mode: 'metro', lines: [{}] })).toBeNull();
    expect(networkModesPhrase({ mode: 'metro', lines: [{ mode: 'tube' }, { mode: 'ferry' }] })).toBeNull();
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

describe('branchesHeading', () => {
  const to = (...names: string[]) => ({ stations: names.map((name) => ({ id: name, name })) });
  it('names each end once, at most three', () => {
    expect(branchesHeading([to('Morden', 'Edgware')])).toBe('Towards Edgware');
    expect(branchesHeading([to('A', 'Edgware'), to('B', 'High Barnet'), to('C', 'Edgware')])).toBe(
      'Towards Edgware or High Barnet',
    );
    expect(branchesHeading([to('Q', 'A'), to('Q', 'B'), to('Q', 'C'), to('Q', 'D')])).toBe('Towards A, B, C and more');
    expect(branchesHeading([to('Q', 'A'), to('Q', 'B'), to('Q', 'C')])).toBe('Towards A, B or C');
    expect(branchesHeading([])).toBe('Branches');
  });
});

describe('isHighFrequencyMode', () => {
  it('is the Underground and the DLR', () => {
    expect(isHighFrequencyMode('tube')).toBe(true);
    expect(isHighFrequencyMode('dlr')).toBe(true);
    expect(isHighFrequencyMode('tram')).toBe(false);
    expect(isHighFrequencyMode(undefined)).toBe(false);
  });
});
