import { describe, expect, it } from 'vitest';
import { dayOffsetMarker, delayLabel, liveStatusLabel } from './serviceStatus';

const live = { status: 'en_route', delayMinutes: 0, delayProvisional: false, cancelled: false };

describe('delayLabel', () => {
  it('says minutes late, minutes early, or on time', () => {
    expect(delayLabel(12)).toEqual({ text: '12 min late', tone: 'late' });
    expect(delayLabel(-3)).toEqual({ text: '3 min early', tone: 'early' });
    expect(delayLabel(0)).toEqual({ text: 'On time', tone: 'onTime' });
  });

  it('prefixes a provisional delay with "Exp."', () => {
    expect(delayLabel(12, true).text).toBe('Exp. 12 min late');
    expect(delayLabel(-2, true).text).toBe('Exp. 2 min early');
    expect(delayLabel(0, true).text).toBe('On time');
  });
});

describe('liveStatusLabel', () => {
  it('says cancelled, late, on time, scheduled, or timetable-only for a bus', () => {
    expect(liveStatusLabel({ ...live, cancelled: true }, 'train')).toEqual({ text: 'Cancelled', tone: 'cancelled' });
    expect(liveStatusLabel({ ...live, status: 'cancelled' }, 'train').text).toBe('Cancelled');
    expect(liveStatusLabel({ ...live, delayMinutes: 7 }, 'train')).toEqual({ text: '7 min late', tone: 'late' });
    expect(liveStatusLabel({ ...live, delayMinutes: 7, delayProvisional: true }, 'train').text).toBe('Exp. 7 min late');
    expect(liveStatusLabel(live, 'train')).toEqual({ text: 'On time', tone: 'onTime' });
    expect(liveStatusLabel(null, 'train')).toEqual({ text: 'Scheduled', tone: 'none' });
    expect(liveStatusLabel({ ...live, delayMinutes: null }, 'train').text).toBe('Scheduled');
    expect(liveStatusLabel(null, 'replacementBus').text).toBe('Timetable only');
    expect(liveStatusLabel(null, 'ferry').text).toBe('Timetable only');
    expect(liveStatusLabel(null, undefined)).toEqual({ text: 'Scheduled', tone: 'none' });
  });

  it('says early running as early, not "On time" (user decision, 2026-10-07)', () => {
    expect(liveStatusLabel({ ...live, delayMinutes: -4 }, 'train')).toEqual({ text: '4 min early', tone: 'early' });
  });

  it('says a cancelled bus is cancelled rather than timetable-only', () => {
    expect(liveStatusLabel({ ...live, cancelled: true }, 'bus').text).toBe('Cancelled');
  });
});

describe('dayOffsetMarker', () => {
  it('marks a later day and leaves the same day unmarked', () => {
    expect(dayOffsetMarker(1)).toEqual({ short: '+1', spoken: 'next day' });
    expect(dayOffsetMarker(2)).toEqual({ short: '+2', spoken: '2 days later' });
    for (const none of [0, -1, null, undefined]) expect(dayOffsetMarker(none)).toBeNull();
  });
});
