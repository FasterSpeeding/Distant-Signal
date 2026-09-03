import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import {
  withStaleFallback,
  __resetStaleCacheForTests,
  STALE_DATA_TTL_MS,
  STALE_CACHE_MAX_ENTRIES,
} from './liveDataCache';
import { ApiNotFoundError, ApiUnauthorizedError, ApiForbiddenError } from './api';

// Same shape as lib/api.test.ts's own `next/headers` stub (there is no Next
// request context in a unit test), extended with the `.get()` this module
// uses to read the session cookie. `incomingSession` is what each test
// dials in.
const incomingSession = { value: '' };
vi.mock('next/headers', () => ({
  cookies: async () => ({
    toString: () => (incomingSession.value ? `distant_signal_session=${incomingSession.value}` : ''),
    get: (name: string) =>
      name === 'distant_signal_session' && incomingSession.value
        ? { name, value: incomingSession.value }
        : undefined,
  }),
}));

describe('withStaleFallback', () => {
  beforeEach(() => {
    __resetStaleCacheForTests();
    incomingSession.value = '';
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('rethrows when the fetcher fails and nothing is cached', async () => {
    const boom = new Error('connect ECONNREFUSED');
    await expect(withStaleFallback('k', () => Promise.reject(boom))).rejects.toBe(boom);
  });

  it('returns fresh data on success', async () => {
    await expect(withStaleFallback('k', async () => ['fresh'])).resolves.toEqual(['fresh']);
  });

  it('serves the cached value when a later fetch fails', async () => {
    await withStaleFallback('k', async () => ['fresh']);
    await expect(
      withStaleFallback('k', () => Promise.reject(new Error('network'))),
    ).resolves.toEqual(['fresh']);
  });

  it('stops serving a stale entry once it is older than the TTL', async () => {
    vi.useFakeTimers();
    await withStaleFallback('k', async () => ['fresh']);

    vi.advanceTimersByTime(STALE_DATA_TTL_MS + 1);

    const boom = new Error('network');
    await expect(withStaleFallback('k', () => Promise.reject(boom))).rejects.toBe(boom);
  });

  it.each([
    ['ApiNotFoundError', () => new ApiNotFoundError('404')],
    ['ApiUnauthorizedError', () => new ApiUnauthorizedError('401')],
    ['ApiForbiddenError', () => new ApiForbiddenError('403')],
  ])('rethrows %s even with a fresh entry cached -- never stale-served', async (_name, make) => {
    await withStaleFallback('k', async () => ['fresh']);
    const err = make();
    await expect(withStaleFallback('k', () => Promise.reject(err))).rejects.toBe(err);
  });

  // The regression test for Correction 2, and the most important case in
  // this file. /Line/Mode/{mode}/Status and /Line/{ids}/Status filter
  // private custom lines by owner (crates/api/src/routes/line_status.rs),
  // so an unscoped cache would serve one visitor's private line to
  // another during an outage -- a cross-user data leak, not a staleness
  // inconvenience.
  it('never serves one session\'s cached data to a different session', async () => {
    incomingSession.value = 'session-a';
    await withStaleFallback('lineStatusForMode:national-rail', async () => ['A private line']);

    incomingSession.value = 'session-b';
    const boom = new Error('network');
    await expect(
      withStaleFallback('lineStatusForMode:national-rail', () => Promise.reject(boom)),
    ).rejects.toBe(boom);
  });

  it('does not serve a logged-in visitor\'s data to an anonymous one', async () => {
    incomingSession.value = 'session-a';
    await withStaleFallback('k', async () => ['A private line']);

    incomingSession.value = '';
    const boom = new Error('network');
    await expect(withStaleFallback('k', () => Promise.reject(boom))).rejects.toBe(boom);
  });

  it('shares one entry between anonymous visitors, who get identical responses', async () => {
    await withStaleFallback('k', async () => ['public']);
    await expect(
      withStaleFallback('k', () => Promise.reject(new Error('network'))),
    ).resolves.toEqual(['public']);
  });

  it('evicts oldest-first once past the entry cap', async () => {
    for (let i = 0; i < STALE_CACHE_MAX_ENTRIES; i++) {
      await withStaleFallback(`k${i}`, async () => [i]);
    }
    // Exactly at the cap, nothing has been evicted yet.
    await expect(
      withStaleFallback('k0', () => Promise.reject(new Error('network'))),
    ).resolves.toEqual([0]);

    // One more distinct key pushes the map over the cap, evicting the
    // oldest. Note the stale *serve* above did not renew k0's position --
    // only a successful fetch re-inserts -- so k0 is still the oldest.
    await withStaleFallback('overflow', async () => ['overflow']);

    const boom = new Error('network');
    await expect(withStaleFallback('k0', () => Promise.reject(boom))).rejects.toBe(boom);
    await expect(
      withStaleFallback('k1', () => Promise.reject(new Error('network'))),
    ).resolves.toEqual([1]);
  });

  it('renews an entry\'s eviction position when it is successfully refreshed', async () => {
    for (let i = 0; i < STALE_CACHE_MAX_ENTRIES; i++) {
      await withStaleFallback(`k${i}`, async () => [i]);
    }
    // A successful re-fetch of the oldest key moves it to the end of the
    // Map's insertion order (the `delete` before `set` in the module). A
    // plain `Map.set` on an existing key keeps its original position,
    // which would evict a frequently-refreshed key ahead of idle ones.
    await withStaleFallback('k0', async () => ['refreshed']);

    await withStaleFallback('overflow', async () => ['overflow']);

    // k1 is now the oldest and takes the eviction; the refreshed k0 lives.
    const boom = new Error('network');
    await expect(withStaleFallback('k1', () => Promise.reject(boom))).rejects.toBe(boom);
    await expect(
      withStaleFallback('k0', () => Promise.reject(new Error('network'))),
    ).resolves.toEqual(['refreshed']);
  });
});
