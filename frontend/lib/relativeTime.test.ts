import { describe, it, expect } from 'vitest';
import { relativeTime } from './relativeTime';

describe('relativeTime', () => {
  it('returns "just now" for under a minute', () => {
    const from = new Date('2026-07-15T09:00:00Z');
    const to = new Date('2026-07-15T09:00:30Z');
    expect(relativeTime(from, to)).toBe('just now');
  });

  it('returns whole minutes under an hour', () => {
    const from = new Date('2026-07-15T09:00:00Z');
    const to = new Date('2026-07-15T09:02:30Z');
    expect(relativeTime(from, to)).toBe('2m ago');
  });

  it('returns whole hours under a day', () => {
    const from = new Date('2026-07-15T09:00:00Z');
    const to = new Date('2026-07-15T12:00:00Z');
    expect(relativeTime(from, to)).toBe('3h ago');
  });

  it('returns whole days at a day or more', () => {
    const from = new Date('2026-07-13T09:00:00Z');
    const to = new Date('2026-07-15T09:00:00Z');
    expect(relativeTime(from, to)).toBe('2d ago');
  });

  it('clamps a future "from" (clock skew) to "just now" instead of a negative value', () => {
    const from = new Date('2026-07-15T09:05:00Z');
    const to = new Date('2026-07-15T09:00:00Z');
    expect(relativeTime(from, to)).toBe('just now');
  });
});
