import { createHash } from 'node:crypto';
import { cookies } from 'next/headers';
import { ApiForbiddenError, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';

/** 10 minutes. Justification (the implementation plan's Resolved open
 * questions #2): 20 AutoRefresh cycles, so unambiguously past any transient
 * blip; an order of magnitude shorter than getStationName's 1-hour window,
 * which is for data that "changes on the order of years"; and inside the
 * 5-15 minute scale on which rail disruption meaningfully evolves, so a
 * value served from here is still worth showing rather than confidently
 * wrong. Past this, app/error.tsx's auto-retrying state is the honest
 * answer instead. */
export const STALE_DATA_TTL_MS = 10 * 60 * 1000;

/** Entries are scoped per session (see `scopedKey`), so the map's size is
 * distinct-visitors x distinct-keys and would otherwise grow without
 * bound in a long-lived server process -- the TTL governs whether an
 * entry is *usable*, not whether it is *retained*. JS Maps iterate in
 * insertion order, so evicting the first key is an oldest-first eviction. */
export const STALE_CACHE_MAX_ENTRIES = 500;

const cache = new Map<string, { data: unknown; at: number }>();

/** The cached line-status endpoints are NOT user-independent, despite
 * being unauthenticated routes: crates/api/src/routes/line_status.rs
 * filters private custom lines by owner, and lib/api.ts forwards the
 * session cookie to them. Keying on the logical key alone would serve one
 * visitor's private custom line to another during an outage. Call sites
 * pass only the logical key and can't forget this, because they never
 * build the real one. The cookie value is hashed rather than used
 * directly so raw session tokens never sit in a long-lived map key. */
async function scopedKey(key: string): Promise<string> {
  const session = (await cookies()).get('distant_signal_session')?.value ?? '';
  return `${createHash('sha256').update(session).digest('hex')} ${key}`;
}

/** Opt-in, per-call-site stale-data fallback for read-only live status
 * fetches. On success, caches and returns fresh data. On a connectivity-
 * shaped failure, returns a cached value if one exists and is younger
 * than STALE_DATA_TTL_MS; otherwise rethrows, leaving the call site's own
 * error handling (ultimately app/error.tsx, which is connectivity-aware
 * and self-healing) to deal with it.
 *
 * ApiNotFoundError / ApiUnauthorizedError / ApiForbiddenError are always
 * rethrown and never stale-served: those are meaningful application
 * states (a deleted line, a logged-out session, a revoked permission),
 * not connectivity failures, and papering over them with old data would
 * be wrong rather than merely stale. Only the generic Error case --
 * network failure, 5xx, timeout -- triggers the substitution.
 *
 * Deliberately NOT applied to session-, mutation- or write-adjacent data.
 * See the design spec's Decision 5. */
export async function withStaleFallback<T>(key: string, fetcher: () => Promise<T>): Promise<T> {
  const mapKey = await scopedKey(key);
  try {
    const data = await fetcher();
    // Delete before set so a refreshed entry moves to the *end* of the
    // Map's insertion order rather than keeping its original position --
    // otherwise a frequently-refreshed key would still be evicted first.
    cache.delete(mapKey);
    cache.set(mapKey, { data, at: Date.now() });
    if (cache.size > STALE_CACHE_MAX_ENTRIES) {
      const oldest = cache.keys().next();
      if (!oldest.done) cache.delete(oldest.value);
    }
    return data;
  } catch (err) {
    if (
      err instanceof ApiNotFoundError ||
      err instanceof ApiUnauthorizedError ||
      err instanceof ApiForbiddenError
    ) {
      throw err;
    }
    const entry = cache.get(mapKey);
    if (entry === undefined) throw err;
    if (Date.now() - entry.at > STALE_DATA_TTL_MS) {
      cache.delete(mapKey);
      throw err;
    }
    return entry.data as T;
  }
}

/** Test-only. No vi.resetModules() pattern exists in this repo to follow,
 * and an explicit reset is clearer than dynamic re-imports per case. */
export function __resetStaleCacheForTests(): void {
  cache.clear();
}
