import { describe, expect, it } from 'vitest';
import {
  formatWorkingTime,
  isPassingPoint,
  stopDirectionLabels,
  stopDisplayArrival,
  stopDisplayTime,
  workingTimeAccessibleLabel,
} from './stopTimes';
import type { JourneyStop } from './types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: 'MKC',
    name: 'Milton Keynes Central',
    tiploc: 'MKNSCEN',
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

describe('stopDisplayTime', () => {
  it('shows the public time, not the working one', () => {
    // WTT 20:50H arrival, 20:52H departure; public 20:51 / 20:52.
    const milton = stop({
      scheduledArrival: '2026-10-01T19:50:00Z',
      scheduledDeparture: '2026-10-01T19:52:00Z',
      publicArrival: '2026-10-01T19:51:00Z',
      publicDeparture: '2026-10-01T19:52:00Z',
    });
    expect(stopDisplayTime(milton)).toBe('2026-10-01T19:52:00Z');
    expect(stopDisplayArrival(milton)).toBe('2026-10-01T19:51:00Z');
  });

  it('shows the public arrival at a set-down-only stop, which has no public departure', () => {
    const motherwell = stop({
      scheduledArrival: '2026-10-01T16:00:00Z',
      scheduledDeparture: '2026-10-01T16:02:00Z',
      publicArrival: '2026-10-01T16:01:00Z',
      publicDeparture: null,
    });
    expect(stopDisplayTime(motherwell)).toBe('2026-10-01T16:01:00Z');
  });

  it('has no passenger arrival at a pick-up-only stop', () => {
    const watford = stop({
      scheduledArrival: '2026-10-01T19:29:00Z',
      scheduledDeparture: '2026-10-01T19:31:00Z',
      publicArrival: null,
      publicDeparture: '2026-10-01T19:31:00Z',
    });
    expect(stopDisplayArrival(watford)).toBeNull();
  });

  it('falls back to the working time when the API sent no public time', () => {
    const older = stop({ scheduledDeparture: '2026-10-01T19:52:00Z' });
    expect(stopDisplayTime(older)).toBe('2026-10-01T19:52:00Z');
    expect(stopDisplayArrival(stop({ scheduledArrival: '2026-10-01T19:50:00Z' }))).toBe('2026-10-01T19:50:00Z');
  });
});

describe('stopDirectionLabels', () => {
  it('labels set-down-only, pick-up-only and request stops', () => {
    expect(stopDirectionLabels(stop({ canBoard: false, canAlight: true }))).toEqual(['Set down only']);
    expect(stopDirectionLabels(stop({ canBoard: true, canAlight: false }))).toEqual(['Pick up only']);
    expect(stopDirectionLabels(stop({ canBoard: true, canAlight: true, requestStop: true }))).toEqual(['Request stop']);
    expect(stopDirectionLabels(stop({ canBoard: true, canAlight: true }))).toEqual([]);
  });

  it('does not label the ends of the line or an older response', () => {
    expect(stopDirectionLabels(stop({ kind: 'Origin', canBoard: true, canAlight: false }))).toEqual([]);
    expect(stopDirectionLabels(stop({ kind: 'Terminate', canBoard: false, canAlight: true }))).toEqual([]);
    expect(stopDirectionLabels(stop({}))).toEqual([]);
  });
});

describe('isPassingPoint', () => {
  it('is a location with only a pass time', () => {
    expect(isPassingPoint(stop({ workingPass: '2026-10-01T10:52:30Z' }))).toBe(true);
    expect(isPassingPoint(stop({ scheduledArrival: '2026-10-01T10:52:00Z' }))).toBe(false);
  });
});

describe('formatWorkingTime', () => {
  it('prints a half-minute as ½ and reads it out in words', () => {
    expect(formatWorkingTime('2026-10-01T19:50:30Z')).toBe('20:50½');
    expect(workingTimeAccessibleLabel('2026-10-01T19:50:30Z')).toBe('20:50 and a half');
    expect(formatWorkingTime('2026-10-01T19:50:00Z')).toBe('20:50');
  });
});
