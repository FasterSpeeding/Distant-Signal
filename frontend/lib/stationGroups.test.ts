import { afterEach, describe, expect, it, vi } from 'vitest';
import {
  fetchStationGroups,
  groupLabels,
  groupOptionLabel,
  loadStationGroups,
  matchingGroupSuggestions,
  resetStationGroupsCache,
  withGroupSuggestions,
  type StationGroup,
} from './stationGroups';

const london: StationGroup = {
  group: 'LON',
  code: 'group:LON',
  name: 'London Terminals',
  members: [
    { crs: 'KGX', name: 'London Kings Cross' },
    { crs: 'STP', name: 'London St Pancras International' },
  ],
};

function okResponse(body: unknown) {
  return { ok: true, json: () => Promise.resolve(body) };
}

describe('station groups', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    resetStationGroupsCache();
  });

  it('labels a group with its station count', () => {
    expect(groupOptionLabel(london)).toBe('Any of the London Terminals (2 stations)');
    expect(groupOptionLabel({ name: 'Solo', members: [{ crs: 'AAA', name: null }] })).toBe(
      'Any of the Solo (1 station)',
    );
    expect(groupLabels([london])).toEqual(new Map([['group:LON', 'Any of the London Terminals (2 stations)']]));
  });

  it('suggests the groups whose name or code matches, none for an empty query', () => {
    expect(matchingGroupSuggestions('london', [london])).toEqual([
      { code: 'group:LON', name: 'Any of the London Terminals (2 stations)' },
    ]);
    expect(matchingGroupSuggestions('GROUP:lo', [london])).toHaveLength(1);
    expect(matchingGroupSuggestions('terminals', [london])).toHaveLength(1);
    expect(matchingGroupSuggestions('york', [london])).toEqual([]);
    expect(matchingGroupSuggestions('  ', [london])).toEqual([]);
  });

  it('fetches GET /api/Trips/station-groups, and falls back to none', async () => {
    const fetchMock = vi.fn().mockResolvedValueOnce(okResponse({ groups: [london] }));
    vi.stubGlobal('fetch', fetchMock);
    expect(await fetchStationGroups()).toEqual([london]);
    expect(fetchMock).toHaveBeenCalledWith('/api/Trips/station-groups', {});

    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce({ ok: false, json: () => Promise.resolve({}) }));
    expect(await fetchStationGroups()).toEqual([]);
    vi.stubGlobal('fetch', vi.fn().mockRejectedValueOnce(new TypeError('offline')));
    expect(await fetchStationGroups()).toEqual([]);
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(okResponse({ nope: true })));
    expect(await fetchStationGroups()).toEqual([]);
  });

  it('loads the groups once, but retries after a failure', async () => {
    const fetchMock = vi
      .fn()
      .mockRejectedValueOnce(new TypeError('offline'))
      .mockResolvedValueOnce(okResponse({ groups: [london] }));
    vi.stubGlobal('fetch', fetchMock);
    expect(await loadStationGroups()).toEqual([]);
    expect(await loadStationGroups()).toEqual([london]);
    expect(await loadStationGroups()).toEqual([london]);
    expect(fetchMock).toHaveBeenCalledTimes(2);
  });

  it('lists matching groups before the station search results', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(okResponse({ groups: [london] })));
    const search = withGroupSuggestions(async (q) => [{ code: 'KGX', name: `London Kings Cross (${q})` }]);
    const signal = new AbortController().signal;
    expect(await search('London', signal)).toEqual([
      { code: 'group:LON', name: 'Any of the London Terminals (2 stations)' },
      { code: 'KGX', name: 'London Kings Cross (London)' },
    ]);
    expect(await search('Kings', signal)).toEqual([{ code: 'KGX', name: 'London Kings Cross (Kings)' }]);
  });
});
