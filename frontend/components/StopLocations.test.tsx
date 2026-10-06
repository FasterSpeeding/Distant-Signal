// Bus stops, ferry terminals and working-only calls on the train page and in
// the journey planner (docs/superpowers/specs/2026-10-06-tiploc-locations-design.md).
import { fireEvent, screen } from '@testing-library/react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { JourneyTimeline, isGenuineCallingPoint, journeyStopLabel } from './JourneyTimeline';
import { WorkingTimetable, locationLabel } from './WorkingTimetable';
import { isRoadOrWaterStop, isWorkingOnlyCall } from '@/lib/stopTimes';
import { codeRouteLabel, codeStationLabel, isTiplocCode, normalizeLocationCode } from '@/lib/stationLabel';
import { suggestionOptionLabel } from '@/lib/suggestionAutocomplete';
import { getStationNames, searchPlannerLocations } from '@/lib/suggestions';
import { buildTripPlanQuery } from '@/lib/tripPlan';
import type { JourneyStop } from '@/lib/types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: null,
    name: null,
    tiploc: null,
    kind: 'Intermediate',
    scheduledArrival: null,
    scheduledDeparture: null,
    actualArrival: null,
    actualDeparture: null,
    estimatedArrival: null,
    estimatedDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    stopStatus: 'Unknown',
    skipSource: null,
    platform: null,
    plannedPlatform: null,
    platformChanged: false,
    ...overrides,
  };
}

const busStop = stop({
  tiploc: 'HTRBUS3',
  name: 'Heathrow Terminal 3 (bus stop)',
  locationType: 'bus_stop',
  parentCrs: 'HXX',
  scheduledDeparture: '2026-10-07T07:00:00Z',
  publicDeparture: '2026-10-07T07:00:00Z',
  workingDeparture: '2026-10-07T07:00:00Z',
});

// `MARY10`: Chiltern trains stop at Marylebone's platform-10 signal on the
// working timetable only -- no public time, no station.
const signalCall = stop({
  tiploc: 'MARY10',
  name: 'Marylebone 10 Signal',
  locationType: 'passing_point',
  scheduledArrival: '2026-10-07T20:01:00Z',
  scheduledDeparture: '2026-10-07T20:02:00Z',
  workingArrival: '2026-10-07T20:01:00Z',
  workingDeparture: '2026-10-07T20:02:00Z',
});

const station = stop({
  crs: 'MYB',
  name: 'London Marylebone',
  locationType: 'station',
  scheduledArrival: '2026-10-07T20:03:00Z',
  publicArrival: '2026-10-07T20:03:00Z',
  workingArrival: '2026-10-07T20:03:00Z',
});

describe('working-only calls', () => {
  it('are recognised: a working stop with no public time, away from any station', () => {
    expect(isWorkingOnlyCall(signalCall)).toBe(true);
    expect(isWorkingOnlyCall(busStop)).toBe(false);
    expect(isWorkingOnlyCall(station)).toBe(false);
    // A station call without public times (an older schedule) is never one.
    expect(isWorkingOnlyCall(stop({ crs: 'RDG', workingDeparture: '2026-10-07T07:00:00Z' }))).toBe(false);
    // Nor is a stop from a response with no working times at all.
    expect(isWorkingOnlyCall(stop({ scheduledArrival: '2026-10-07T07:00:00Z' }))).toBe(false);
  });

  it('are hidden from the passenger timeline, where they used to show as "Stop N"', () => {
    expect(isGenuineCallingPoint(signalCall)).toBe(false);
    expect(isGenuineCallingPoint(busStop)).toBe(true);
    expect(isGenuineCallingPoint(station)).toBe(true);
  });

  it('are still listed, by name, in the working timetable', async () => {
    renderWithMantine(<WorkingTimetable stops={[busStop, signalCall, station]} />);
    fireEvent.click(screen.getByRole('button', { name: 'Detailed (working timetable)' }));
    expect(await screen.findByRole('table', { name: 'Working timetable' })).toBeInTheDocument();
    expect(screen.getByText('Marylebone 10 Signal')).toBeInTheDocument();
    expect(screen.queryByText('MARY10')).not.toBeInTheDocument();
  });
});

describe('stop labels', () => {
  it('use the API name for a bus stop, never the "Stop N" fallback', () => {
    expect(journeyStopLabel(busStop, 0, 3)).toBe('Heathrow Terminal 3 (bus stop)');
    expect(journeyStopLabel(stop({ tiploc: 'NOWHERE' }), 1, 3)).toBe('Stop 2');
  });

  it('name a timing point in the working timetable rather than its raw TIPLOC', () => {
    expect(locationLabel(signalCall)).toBe('Marylebone 10 Signal');
    expect(locationLabel(stop({ tiploc: 'XYZ123' }))).toBe('XYZ123');
  });

  it('link a bus stop to its parent station', () => {
    renderWithMantine(<JourneyTimeline stops={[busStop, station]} />);
    const link = screen.getByRole('link', { name: 'Heathrow Terminal 3 (bus stop)' });
    expect(link).toHaveAttribute('href', '/stations/HXX');
  });

  it('leave a bus stop with no parent as plain text', () => {
    const keswick = stop({ ...busStop, name: 'Keswick (bus station)', parentCrs: null });
    renderWithMantine(<JourneyTimeline stops={[keswick, station]} />);
    expect(screen.getByText('Keswick (bus station)')).toBeInTheDocument();
    expect(screen.queryByRole('link', { name: /Keswick/ })).not.toBeInTheDocument();
  });

  it('know a bus stop or ferry terminal from a station', () => {
    expect(isRoadOrWaterStop(busStop)).toBe(true);
    expect(isRoadOrWaterStop({ locationType: 'ferry_terminal' })).toBe(true);
    expect(isRoadOrWaterStop(station)).toBe(false);
    expect(isRoadOrWaterStop({})).toBe(false);
  });
});

describe('planner locations', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('show a bus stop by its name and mode, never its tiploc: code', () => {
    expect(suggestionOptionLabel({ code: 'tiploc:KESWICK', name: 'Keswick (bus)', kind: 'bus' })).toBe('Keswick (bus)');
    expect(suggestionOptionLabel({ code: 'tiploc:BDICK', name: 'Brodick (ferry)', kind: 'ferry' })).toBe(
      'Brodick (ferry)',
    );
    expect(suggestionOptionLabel({ code: 'EDB', name: 'Edinburgh' })).toBe('EDB — Edinburgh');
    expect(codeStationLabel('tiploc:SANWBUS', 'St Andrews (bus)')).toBe('St Andrews (bus)');
    expect(codeRouteLabel('tiploc:SANWBUS', 'St Andrews (bus)', 'EDB', 'Edinburgh')).toBe(
      'St Andrews (bus) → EDB — Edinburgh',
    );
  });

  it('normalise tiploc: codes the way the API does', () => {
    expect(isTiplocCode('TIPLOC:sanwbus')).toBe(true);
    expect(isTiplocCode('SAO')).toBe(false);
    expect(normalizeLocationCode(' Tiploc:sanwbus ')).toBe('tiploc:SANWBUS');
    expect(normalizeLocationCode(' edb ')).toBe('EDB');
    const query = new URLSearchParams(
      buildTripPlanQuery({
        originCrs: 'tiploc:sanwbus',
        destinationCrs: 'edb',
        waypointCrs: ['tiploc:keswick'],
        date: '2026-10-07',
        results: 'fastest',
      }),
    );
    expect(query.get('origin')).toBe('tiploc:SANWBUS');
    expect(query.get('destination')).toBe('EDB');
    expect(query.get('waypoints')).toBe('tiploc:KESWICK');
  });

  it('search stops only through the planner search', async () => {
    const fetchMock = vi.fn(
      async (_input: string) =>
        new Response(JSON.stringify([{ code: 'tiploc:KESWICK', name: 'Keswick (bus)', kind: 'bus' }]), {
          status: 200,
        }),
    );
    vi.stubGlobal('fetch', fetchMock);
    const results = await searchPlannerLocations('kesw');
    expect(fetchMock.mock.calls[0]![0]).toBe('/api/stations?q=kesw&stops=true');
    expect(results[0]!.name).toBe('Keswick (bus)');

    const names = await getStationNames(['tiploc:KESWICK']);
    expect(fetchMock.mock.calls[1]![0]).toBe('/api/stations?q=tiploc%3AKESWICK&stops=true');
    expect(names.get('tiploc:KESWICK')).toBe('Keswick (bus)');
  });
});
