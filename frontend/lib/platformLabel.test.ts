import { describe, expect, it } from 'vitest';
import {
  BOOKED_PLATFORM_TOOLTIP,
  CANCELLED_PLATFORM_TOOLTIP,
  changedFromPlatform,
  platformDisplay,
  platformTooltip,
  spokenPlatformLabel,
  type PlatformDisplay,
} from './platformLabel';

const live = { platform: '4', plannedPlatform: '4', platformChanged: false };
const changed = { platform: '9', plannedPlatform: '6', platformChanged: true };

describe('platformDisplay', () => {
  it('is null when no platform is known', () => {
    expect(platformDisplay({ platform: null, plannedPlatform: null, platformChanged: false })).toBeNull();
  });

  it('is booked when only the timetable platform is known', () => {
    expect(platformDisplay({ ...live, platform: null, bookedPlatform: '4', platformBasis: 'booked' })).toStrictEqual({
      state: 'booked',
      platform: '4',
    });
  });

  it('is null for a booked platform without the booked basis, or a booked basis without a platform', () => {
    expect(platformDisplay({ ...live, platform: null, bookedPlatform: '4' })).toBeNull();
    expect(platformDisplay({ ...live, platform: null, bookedPlatform: null, platformBasis: 'booked' })).toBeNull();
  });

  it('is live for a board platform, with or without a basis', () => {
    expect(platformDisplay(live)).toStrictEqual({ state: 'live', platform: '4', confirmed: false });
    expect(platformDisplay({ ...live, platformBasis: 'board' })).toStrictEqual({
      state: 'live',
      platform: '4',
      confirmed: false,
    });
  });

  it('is confirmed live once the api says so', () => {
    expect(platformDisplay({ ...live, platformBasis: 'confirmed' })).toStrictEqual({
      state: 'live',
      platform: '4',
      confirmed: true,
    });
  });

  it('is changed, naming the earlier platform', () => {
    expect(platformDisplay(changed)).toStrictEqual({
      state: 'changed',
      platform: '9',
      previous: '6',
      confirmed: false,
    });
  });

  it('compares a change against the booked platform once the api sends a basis', () => {
    expect(
      platformDisplay({ ...changed, plannedPlatform: '7', bookedPlatform: '6', platformBasis: 'confirmed' }),
    ).toStrictEqual({ state: 'changed', platform: '9', previous: '6', confirmed: true });
  });

  it('reads a change with nothing to name, or naming the same platform, as live', () => {
    expect(platformDisplay({ ...changed, plannedPlatform: null })?.state).toBe('live');
    expect(platformDisplay({ ...changed, plannedPlatform: '9' })?.state).toBe('live');
  });

  it('is cancelled whatever else it is', () => {
    expect(platformDisplay({ ...changed, platformStatus: 'cancelled' })).toStrictEqual({
      state: 'cancelled',
      platform: '9',
    });
  });

  it('reads an unknown status or basis as a live board platform', () => {
    const unknown = {
      ...live,
      platformStatus: 'provisional' as unknown as 'active',
      platformBasis: 'guessed' as unknown as 'board',
    };
    expect(platformDisplay(unknown)).toStrictEqual({ state: 'live', platform: '4', confirmed: false });
  });
});

describe('spokenPlatformLabel and platformTooltip', () => {
  const cases: [PlatformDisplay, string, string | undefined][] = [
    [{ state: 'booked', platform: '4' }, 'Platform 4 (booked, not confirmed)', BOOKED_PLATFORM_TOOLTIP],
    [{ state: 'live', platform: '4', confirmed: false }, 'Platform 4', undefined],
    [{ state: 'live', platform: '4A', confirmed: true }, 'Platform 4A (confirmed)', 'Confirmed platform'],
    [
      { state: 'changed', platform: '9', previous: '6', confirmed: false },
      'Platform 9 (changed from 6)',
      'Changed from platform 6',
    ],
    [
      { state: 'changed', platform: '9', previous: '6', confirmed: true },
      'Platform 9 (confirmed, changed from 6)',
      'Changed from platform 6, now confirmed',
    ],
    [{ state: 'cancelled', platform: '4' }, 'Platform 4 (cancelled)', CANCELLED_PLATFORM_TOOLTIP],
  ];

  it.each(cases)('words %o', (display, spoken, tooltip) => {
    expect(spokenPlatformLabel(display)).toBe(spoken);
    expect(platformTooltip(display)).toBe(tooltip);
  });
});

describe('changedFromPlatform', () => {
  it('names the booked platform once the api compares against it', () => {
    expect(changedFromPlatform({ plannedPlatform: '9', bookedPlatform: '6', platformBasis: 'board' })).toBe('6');
    expect(changedFromPlatform({ plannedPlatform: '9', bookedPlatform: null, platformBasis: 'board' })).toBe('9');
  });

  it('keeps the first platform seen for an older response', () => {
    expect(changedFromPlatform({ plannedPlatform: '9', bookedPlatform: '6' })).toBe('9');
  });
});
