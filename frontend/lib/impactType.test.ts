import { describe, it, expect } from 'vitest';
import { impactTypeLabel, IMPACT_TYPE_LABELS } from './impactType';

describe('impactTypeLabel', () => {
  it('returns the correct label for each known value', () => {
    expect(impactTypeLabel('rail_replacement_bus')).toBe('Rail Replacement Bus');
    expect(impactTypeLabel('no_scheduled_service')).toBe('No Scheduled Service');
    expect(impactTypeLabel('diversion')).toBe('Diversion');
  });

  it('returns null for null', () => {
    expect(impactTypeLabel(null)).toBeNull();
  });

  it('returns null for undefined', () => {
    expect(impactTypeLabel(undefined)).toBeNull();
  });

  it('returns null for an unrecognized value, not the raw string', () => {
    expect(impactTypeLabel('some_future_taxonomy_value')).toBeNull();
  });

  it('exposes exactly the three known keys', () => {
    expect(Object.keys(IMPACT_TYPE_LABELS).sort()).toEqual(
      ['diversion', 'no_scheduled_service', 'rail_replacement_bus'].sort(),
    );
  });
});
