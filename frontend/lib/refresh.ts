/** The app's one auto-refresh cadence, shared so nothing states it twice
 * and risks drifting: `components/AutoRefresh.tsx` uses it as the actual
 * `router.refresh()` interval, and anywhere that tells a visitor how often
 * a page's data goes stale (e.g. the train pages' "refreshes every Ns"
 * caption next to `LastUpdated`) reads it from here rather than repeating
 * the literal `30_000`/`30`.
 *
 * Plain, non-`'use client'` module deliberately: `AutoRefresh.tsx` is a
 * Client Component, and a Server Component (every train detail page) needs
 * this same number too. Re-exporting a value from a `'use client'` module
 * turns every one of that module's exports into an opaque client
 * reference for the RSC bundler, which is fine for the `AutoRefresh`
 * component itself but not guaranteed for a plain constant read from a
 * Server Component — so the number lives here, in a module with no
 * directive, and `AutoRefresh.tsx` imports it rather than defining it. */
export const REFRESH_INTERVAL_MS = 30_000;
