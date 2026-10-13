import { describe, it, expect } from 'vitest';
import { clearedTitle, wasExpectedUntil } from './recentlyCleared';

describe('clearedTitle', () => {
  it("drops National Rail's CLEARED prefix", () => {
    expect(clearedTitle('CLEARED: Disruption between Woking and Guildford')).toBe(
      'Disruption between Woking and Guildford',
    );
    expect(clearedTitle('Cleared:Disruption at Barking')).toBe('Disruption at Barking');
  });

  it('keeps any other summary, and one that is only the prefix', () => {
    expect(clearedTitle('Disruption between Woking and Guildford has ended')).toBe(
      'Disruption between Woking and Guildford has ended',
    );
    expect(clearedTitle('Lines cleared: Woking')).toBe('Lines cleared: Woking');
    expect(clearedTitle('CLEARED:')).toBe('CLEARED:');
  });
});

describe('wasExpectedUntil', () => {
  const now = Date.parse('2026-10-02T20:40:00Z');

  it('states an end time still ahead, in UK time', () => {
    expect(wasExpectedUntil({ expectedUntil: '2026-10-02T21:30:00Z', expectedUntilEndOfDay: false }, now)).toBe(
      'Was expected until 22:30',
    );
  });

  it('says nothing once the stated end has passed, or when none was stated', () => {
    expect(wasExpectedUntil({ expectedUntil: '2026-10-02T20:40:00Z', expectedUntilEndOfDay: false }, now)).toBeNull();
    expect(wasExpectedUntil({ expectedUntil: null, expectedUntilEndOfDay: false }, now)).toBeNull();
  });

  it('states the end of the day', () => {
    expect(wasExpectedUntil({ expectedUntil: null, expectedUntilEndOfDay: true }, now)).toBe(
      'Was expected until the end of the day',
    );
  });
});
