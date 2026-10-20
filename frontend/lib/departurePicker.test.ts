import { describe, expect, it } from 'vitest';
import { cifDepartureDateTime, type CifPickerRow } from './departurePicker';

function row(overrides: Partial<CifPickerRow>): CifPickerRow {
  return {
    uid: 'C01355',
    scheduled: '18:15',
    publicDeparture: '18:15',
    dayOffset: 0,
    destinationCrs: 'WVH',
    destinationName: null,
    serviceDate: '2026-10-20',
    ...overrides,
  };
}

describe('cifDepartureDateTime', () => {
  // Schedule matching keys on the working time, so a pin made from a row
  // keeps using it now that `scheduled` is the public time (2026-10-20).
  it('pins the working departure when the row has one', () => {
    expect(cifDepartureDateTime(row({ workingDeparture: '18:18', workingDayOffset: 0 }))).toBe('2026-10-20 18:18:00');
  });

  it("uses the working time's own day when rounding crosses midnight", () => {
    expect(
      cifDepartureDateTime(row({ scheduled: '23:59', dayOffset: 0, workingDeparture: '00:01', workingDayOffset: 1 })),
    ).toBe('2026-10-21 00:01:00');
  });

  it('falls back to scheduled and dayOffset from an older backend', () => {
    expect(cifDepartureDateTime(row({ scheduled: '00:07', dayOffset: 1 }))).toBe('2026-10-21 00:07:00');
  });
});
