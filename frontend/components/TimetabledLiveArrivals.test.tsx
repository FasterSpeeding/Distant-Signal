import { afterEach, describe, expect, it, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { TimetabledLiveArrivals } from './TimetabledLiveArrivals';
import type { JourneyLegDetail, LegLiveArrivalsFilter, TflStopArrivals } from '@/lib/types';

const FILTER: LegLiveArrivalsFilter = { stopId: '940GZZLUKSX', lineId: 'victoria', destinations: ['Brixton'] };

function leg(departInMinutes: number): JourneyLegDetail {
  const at = (minutes: number) => new Date(Date.now() + minutes * 60_000).toISOString();
  return {
    id: 7,
    kind: 'untracked',
    originCrs: null,
    originName: "King's Cross St. Pancras",
    destinationCrs: null,
    destinationName: 'Victoria',
    serviceDate: '2026-11-04',
    departAfter: null,
    departBefore: null,
    arriveAfter: null,
    arriveBefore: null,
    windowSearched: false,
    matchMode: 'manual',
    trackedTrainState: null,
    legSkip: null,
    transitMode: 'tube',
    plannedDeparture: at(departInMinutes),
    plannedArrival: at(departInMinutes + 10),
    timetable: {
      network: 'tfl',
      line: { id: 'tfl:victoria', name: 'Victoria', colour: null, mode: 'tube' },
      state: 'matched',
      scheduledDeparture: at(departInMinutes),
      scheduledArrival: at(departInMinutes + 10),
      headsign: 'Brixton',
      calls: [],
      notices: [],
      liveArrivals: FILTER,
    },
  };
}

function board(): TflStopArrivals {
  const at = (seconds: number) => new Date(Date.now() + seconds * 1000).toISOString();
  return {
    stopId: '940GZZLUKSX',
    fetchedAt: at(0),
    groups: [
      {
        lineId: 'victoria',
        lineName: 'Victoria',
        platformName: 'Southbound - Platform 5',
        arrivals: [
          { id: 'a', destination: 'Brixton', expectedArrival: at(45), timeToStationSecs: 45 },
          { id: 'b', destination: 'Brixton', expectedArrival: at(405), timeToStationSecs: 405 },
        ],
      },
      {
        lineId: 'victoria',
        lineName: 'Victoria',
        platformName: 'Northbound - Platform 6',
        arrivals: [{ id: 'c', destination: 'Walthamstow Central', expectedArrival: at(165), timeToStationSecs: 165 }],
      },
      {
        lineId: 'piccadilly',
        lineName: 'Piccadilly',
        platformName: 'Westbound',
        arrivals: [{ id: 'd', destination: 'Brixton', expectedArrival: at(60), timeToStationSecs: 60 }],
      },
    ],
  };
}

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('TimetabledLiveArrivals', () => {
  it("lists TfL's trains for the leg's line that reach its stop, and says they are predictions", async () => {
    const fetchMock = vi.fn().mockResolvedValue(new Response(JSON.stringify(board())));
    vi.stubGlobal('fetch', fetchMock);
    renderWithMantine(<TimetabledLiveArrivals leg={leg(10)} filter={FILTER} />);
    expect(await screen.findByText("Next trains at King's Cross St. Pancras (live from TfL)")).toBeInTheDocument();
    const rows = await screen.findAllByRole('listitem');
    expect(rows.map((row) => row.getAttribute('data-live-arrival'))).toEqual(['a', 'b']);
    expect(fetchMock).toHaveBeenCalledWith('/api/tfl/stops/940GZZLUKSX/arrivals');
    expect(screen.getByText(/TfL's predictions for trains that go to Victoria\./)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: "Every live arrival at King's Cross St. Pancras" })).toHaveAttribute(
      'href',
      '/tfl/stops/940GZZLUKSX',
    );
  });

  it('says when TfL lists none for the stop, and when it could not ask', async () => {
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response(JSON.stringify({ ...board(), groups: [] }))));
    const { unmount } = renderWithMantine(<TimetabledLiveArrivals leg={leg(10)} filter={FILTER} />);
    expect(await screen.findByText('TfL lists no trains for Victoria here right now.')).toBeInTheDocument();
    unmount();
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('{}', { status: 503 })));
    renderWithMantine(<TimetabledLiveArrivals leg={leg(10)} filter={FILTER} />);
    expect(await screen.findByText(/^Couldn't load TfL's live arrivals\./)).toBeInTheDocument();
  });

  it('stays hidden, and asks nothing, outside the hour before the leg until it ends', () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);
    const { container } = renderWithMantine(<TimetabledLiveArrivals leg={leg(120)} filter={FILTER} />);
    expect(container.querySelector('[data-live-arrivals]')).toBeNull();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
