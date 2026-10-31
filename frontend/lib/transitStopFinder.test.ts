import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  loadFinderStops,
  rankStops,
  resetFinderStops,
  stopIndexHref,
  stopLineNames,
  type FinderStop,
} from './transitStopFinder';

const stop = (name: string, lines: string[] = []): FinderStop => ({ id: name.toUpperCase().slice(0, 8), name, lines });

const STOPS = [
  stop('Piccadilly Gardens'),
  stop('Piccadilly'),
  stop('Market Street'),
  stop("King's Cross St. Pancras"),
  stop('St Peter’s Square'),
  stop('Old Trafford'),
  stop('Trafford Bar'),
];

describe('rankStops', () => {
  it('is empty for a blank query', () => {
    expect(rankStops(STOPS, '   ')).toEqual([]);
  });

  it('puts the exact name first, then prefixes, then word prefixes, then the rest', () => {
    expect(rankStops(STOPS, 'piccadilly').map((s) => s.name)).toEqual(['Piccadilly', 'Piccadilly Gardens']);
    expect(rankStops(STOPS, 'traff').map((s) => s.name)).toEqual(['Trafford Bar', 'Old Trafford']);
    expect(rankStops(STOPS, 'af').map((s) => s.name)).toEqual(['Old Trafford', 'Trafford Bar']);
  });

  it('matches every word in any order, ignoring case and punctuation', () => {
    expect(rankStops(STOPS, 'kings cross').map((s) => s.name)).toEqual(["King's Cross St. Pancras"]);
    expect(rankStops(STOPS, 'PANCRAS st').map((s) => s.name)).toEqual(["King's Cross St. Pancras"]);
    expect(rankStops(STOPS, 'zzz')).toEqual([]);
  });
});

describe('stopLineNames', () => {
  const names = { 'm:blue': 'Blue', 'm:green': 'Green', 'm:red': 'Red' };

  it('names the lines in a sentence', () => {
    expect(stopLineNames(stop('A', ['m:blue']), names)).toBe('Blue');
    expect(stopLineNames(stop('A', ['m:blue', 'm:green']), names)).toBe('Blue and Green');
    expect(stopLineNames(stop('A', ['m:blue', 'm:green', 'm:red']), names)).toBe('Blue, Green and Red');
  });

  it('leaves out lines the network does not name', () => {
    expect(stopLineNames(stop('A', ['m:pink', 'm:red']), names)).toBe('Red');
    expect(stopLineNames(stop('A', []), names)).toBe('');
  });
});

describe('stopIndexHref', () => {
  it('is the A to Z, filtered when a query is given', () => {
    expect(stopIndexHref('metrolink')).toBe('/transit/metrolink/stops');
    expect(stopIndexHref('metrolink', '  ')).toBe('/transit/metrolink/stops');
    expect(stopIndexHref('tfl', "king's x")).toBe('/transit/tfl/stops?q=king%27s+x');
  });
});

describe('loadFinderStops', () => {
  const fetchMock = vi.fn();

  beforeEach(() => {
    resetFinderStops();
    fetchMock.mockReset();
    vi.stubGlobal('fetch', fetchMock);
  });
  afterEach(() => vi.unstubAllGlobals());

  it('fetches once per network and keeps only well-formed stations', async () => {
    fetchMock.mockResolvedValue(
      new Response(
        JSON.stringify({
          stops: [
            { id: '9400ZZMAPIC', name: 'Piccadilly', lines: ['metrolink:blue', 7], lat: 1, lon: 2 },
            { id: 'bad id', name: 'Nope', lines: [] },
            { id: '9400ZZMAECC', name: 'Eccles' },
            null,
          ],
        }),
      ),
    );
    const first = await loadFinderStops('metrolink');
    const second = await loadFinderStops('metrolink');
    expect(fetchMock).toHaveBeenCalledTimes(1);
    expect(fetchMock).toHaveBeenCalledWith('/api/transit/metrolink/stops');
    expect(second).toBe(first);
    expect(first).toEqual([
      { id: '9400ZZMAPIC', name: 'Piccadilly', lines: ['metrolink:blue'] },
      { id: '9400ZZMAECC', name: 'Eccles', lines: [] },
    ]);
  });

  it('forgets a failed load, so the next one retries', async () => {
    fetchMock.mockResolvedValueOnce(new Response('', { status: 503 }));
    await expect(loadFinderStops('tfl')).rejects.toThrow('stops 503');
    fetchMock.mockResolvedValueOnce(new Response(JSON.stringify({ stops: [] })));
    await expect(loadFinderStops('tfl')).resolves.toEqual([]);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });
});
