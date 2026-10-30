/** Opt-in usage analytics in the browser (DS-190): who has agreed, and
 * sending an event when they have.
 *
 * Off by default, and nothing is sent unless the visitor turned it on:
 *
 * - logged in, the account's own choice (`analyticsOptIn` on the session,
 *   changed with `PUT /api/preferences/analytics`); the api also drops
 *   events from a signed-in account that has not opted in;
 * - logged out, this browser's choice, saved under
 *   {@link ANALYTICS_OPT_IN_STORAGE_KEY} (listed on /cookies).
 *
 * A browser that sends Global Privacy Control or Do Not Track sends
 * nothing at all, whatever the choice ({@link privacySignalSet}); the api
 * drops such requests too.
 *
 * Events go to the app's own origin (`/api/events`, proxied to the api's
 * `POST /public/events`) with `navigator.sendBeacon`, so the CSP's
 * `connect-src 'self'` already allows them and no cookie is set. Nothing
 * here runs on the server. */

import {
  apiErrorStatusFor,
  routeTemplateFor,
  type EmptyStateKind,
  type EventName,
  type Feature,
  type FeatureStep,
  type RouteTemplate,
  type UsageEventBody,
} from '@/lib/usageEvents';

/** This browser's choice while logged out: `'true'` when on; absent (or
 * anything else) is off. */
export const ANALYTICS_OPT_IN_STORAGE_KEY = 'ds-analytics-opt-in';

/** Set by `public/offline.html`, only for an opted-in browser, when the
 * offline page was shown; the next page load sends `offline_shown` and
 * removes it. */
export const OFFLINE_PENDING_STORAGE_KEY = 'ds-analytics-offline-pending';

/** Where events are sent: same origin, through the api proxy. */
export const EVENTS_PATH = '/api/events';

interface AnalyticsState {
  /** `null` until the tracker knows whether the visitor is logged in. */
  signedIn: boolean | null;
  accountOptIn: boolean;
  route: RouteTemplate | null;
}

const state: AnalyticsState = { signedIn: null, accountOptIn: false, route: null };

/** Events from before the tracker knew the session: a page's own effects
 * can run before the nav's (streamed in behind `<Suspense>`) on a full
 * load, so an empty state or a "shown" there would otherwise be lost. Sent
 * once the session is known and opted in, else dropped. Bounded. */
const MAX_PENDING_EVENTS = 20;
let pendingEvents: UsageEventBody[] = [];
const listeners = new Set<() => void>();
let version = 0;

function notify() {
  version += 1;
  for (const listener of listeners) listener();
}

/** For `useSyncExternalStore`: re-render when the consent changes. */
export function subscribeToConsent(listener: () => void): () => void {
  listeners.add(listener);
  return () => listeners.delete(listener);
}

export function consentVersion(): number {
  return version;
}

/** Whether the browser asks sites not to track it: Global Privacy Control
 * (`navigator.globalPrivacyControl`) or Do Not Track set to `'1'`. */
export function privacySignalSet(): boolean {
  if (typeof navigator === 'undefined') return false;
  const nav = navigator as Navigator & { globalPrivacyControl?: boolean; msDoNotTrack?: string };
  if (nav.globalPrivacyControl === true) return true;
  const win = typeof window === 'undefined' ? undefined : (window as Window & { doNotTrack?: string });
  return nav.doNotTrack === '1' || nav.msDoNotTrack === '1' || win?.doNotTrack === '1';
}

/** This browser's own choice (the logged-out one). Storage can throw
 * (blocked site data, some private windows): that reads as off. */
export function readDeviceOptIn(): boolean {
  try {
    return window.localStorage.getItem(ANALYTICS_OPT_IN_STORAGE_KEY) === 'true';
  } catch {
    return false;
  }
}

/** Saves this browser's choice. Off removes the key, and any pending
 * offline mark with it. Returns false if storage refused. */
export function writeDeviceOptIn(on: boolean): boolean {
  try {
    if (on) {
      window.localStorage.setItem(ANALYTICS_OPT_IN_STORAGE_KEY, 'true');
    } else {
      window.localStorage.removeItem(ANALYTICS_OPT_IN_STORAGE_KEY);
      window.localStorage.removeItem(OFFLINE_PENDING_STORAGE_KEY);
    }
    notify();
    return true;
  } catch {
    return false;
  }
}

/** Called by the tracker with the session it was rendered with. */
export function setAnalyticsSession(signedIn: boolean, accountOptIn: boolean) {
  if (state.signedIn === signedIn && state.accountOptIn === accountOptIn) return;
  state.signedIn = signedIn;
  state.accountOptIn = accountOptIn;
  const queued = pendingEvents;
  pendingEvents = [];
  if (analyticsEnabled()) for (const body of queued) deliver(body);
  notify();
}

/** Who the tracker says is visiting: `signedIn` is `null` until it knows. */
export function analyticsSession(): { signedIn: boolean | null; accountOptIn: boolean } {
  return { signedIn: state.signedIn, accountOptIn: state.accountOptIn };
}

/** Called once the account's choice is saved, so it applies at once. */
export function setAccountOptIn(on: boolean) {
  if (state.accountOptIn === on) return;
  state.accountOptIn = on;
  notify();
}

/** The page the visitor is on, as a route template. */
export function setCurrentRoute(route: RouteTemplate) {
  state.route = route;
}

export function currentRoute(): RouteTemplate | null {
  return state.route;
}

/** Whether events may be sent right now: the visitor opted in (the
 * account's choice when logged in, this browser's when not) and the
 * browser sends no privacy signal. */
export function analyticsEnabled(): boolean {
  if (typeof window === 'undefined') return false;
  if (privacySignalSet()) return false;
  if (state.signedIn === true) return state.accountOptIn;
  if (state.signedIn === false) return readDeviceOptIn();
  return false;
}

/** The `fetch` from before `installApiErrorReporting` wrapped it, so an
 * event is never itself reported as a failed call. */
let originalFetch: typeof fetch | null = null;

function deliver(body: UsageEventBody) {
  const json = JSON.stringify(body);
  try {
    if (typeof navigator.sendBeacon === 'function') {
      const blob = new Blob([json], { type: 'application/json' });
      if (navigator.sendBeacon(EVENTS_PATH, blob)) return;
    }
  } catch {
    // Fall through to fetch.
  }
  const send = originalFetch ?? (typeof fetch === 'function' ? fetch : null);
  if (!send) return;
  void send(EVENTS_PATH, {
    method: 'POST',
    keepalive: true,
    credentials: 'same-origin',
    headers: { 'Content-Type': 'application/json' },
    body: json,
  }).catch(() => undefined);
}

/** Sends one event if the visitor has opted in, on `route` (by default the
 * current page). Does nothing otherwise. Before the session is known, the
 * event waits for it (`pendingEvents`). */
export function track(name: EventName, props?: Record<string, string>, route?: RouteTemplate) {
  if (typeof window === 'undefined' || privacySignalSet()) return;
  const routeTemplate = route ?? state.route ?? routeTemplateFor(window.location.pathname);
  const body: UsageEventBody = props
    ? { name, route_template: routeTemplate, props }
    : { name, route_template: routeTemplate };
  if (state.signedIn === null) {
    if (pendingEvents.length < MAX_PENDING_EVENTS) pendingEvents.push(body);
    return;
  }
  if (analyticsEnabled()) deliver(body);
}

/** A feature step (`feature` event), e.g. `trackFeature('plan', 'results')`. */
export function trackFeature<F extends Feature>(feature: F, step: FeatureStep<F>) {
  track('feature', { feature, step });
}

/** An empty or no-result state the visitor saw. */
export function trackEmptyState(kind: EmptyStateKind) {
  track('empty_state', { kind });
}

/** Whether `input` is a call to the app's own api proxy, other than the
 * events endpoint itself. */
function isApiCall(input: RequestInfo | URL): boolean {
  try {
    const raw = typeof input === 'string' ? input : input instanceof URL ? input.href : input.url;
    const url = new URL(raw, window.location.origin);
    return url.origin === window.location.origin && url.pathname.startsWith('/api/') && url.pathname !== EVENTS_PATH;
  } catch {
    return false;
  }
}

function isAbort(err: unknown): boolean {
  const name = typeof err === 'object' && err !== null ? (err as { name?: unknown }).name : undefined;
  return name === 'AbortError';
}

/** Reports every failed call the page makes to `/api/*` as `api_error`
 * (the page's route template and the status class, nothing about the
 * call itself), by wrapping `window.fetch` once. The one place that sees
 * every call: the app's components each call `fetch('/api/...')`
 * directly. Returns the undo, for tests. */
export function installApiErrorReporting(): () => void {
  if (typeof window === 'undefined' || originalFetch !== null) return () => undefined;
  // eslint-disable-next-line @typescript-eslint/unbound-method -- kept only to put the exact original back on undo; every call goes through the bound copy below
  const original = window.fetch;
  const wrapped = original.bind(window);
  originalFetch = wrapped;
  const reporting: typeof fetch = async (input, init) => {
    const watched = isApiCall(input);
    try {
      const response = await wrapped(input, init);
      if (watched && !response.ok) {
        const status = apiErrorStatusFor(response.status);
        if (status) track('api_error', { status });
      }
      return response;
    } catch (err) {
      if (watched && !isAbort(err)) track('api_error', { status: 'network' });
      throw err;
    }
  };
  window.fetch = reporting;
  return () => {
    window.fetch = original;
    originalFetch = null;
  };
}

/** Test-only: back to the initial state. */
export function resetAnalyticsForTests() {
  state.signedIn = null;
  state.accountOptIn = false;
  state.route = null;
  pendingEvents = [];
  listeners.clear();
  version = 0;
}
