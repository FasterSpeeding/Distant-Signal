import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import {
  ANALYTICS_OPT_IN_STORAGE_KEY,
  analyticsEnabled,
  installApiErrorReporting,
  OFFLINE_PENDING_STORAGE_KEY,
  privacySignalSet,
  readDeviceOptIn,
  resetAnalyticsForTests,
  setAccountOptIn,
  setAnalyticsSession,
  setCurrentRoute,
  track,
  trackEmptyState,
  trackFeature,
  writeDeviceOptIn,
} from './usageAnalytics';

const beacon = vi.fn((_url: string, _data?: BodyInit | null) => true);

function sentBodies(): unknown[] {
  return beacon.mock.calls.map(([, blob]) => blob);
}

/** jsdom's Blob has no `text()`; FileReader reads it. */
function blobText(blob: Blob): Promise<string> {
  return new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error ?? new Error('read failed'));
    reader.readAsText(blob);
  });
}

async function sentJson(): Promise<unknown[]> {
  return Promise.all(beacon.mock.calls.map(async ([, blob]) => JSON.parse(await blobText(blob as Blob))));
}

/** Sets navigator's privacy signals for one test. */
function signals({ gpc, dnt }: { gpc?: boolean; dnt?: string }) {
  Object.defineProperty(navigator, 'globalPrivacyControl', { value: gpc, configurable: true });
  Object.defineProperty(navigator, 'doNotTrack', { value: dnt ?? null, configurable: true });
}

beforeEach(() => {
  resetAnalyticsForTests();
  localStorage.clear();
  beacon.mockClear();
  Object.defineProperty(navigator, 'sendBeacon', { value: beacon, configurable: true });
  signals({});
  setCurrentRoute('/lines/[id]');
});

afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});

describe('opt-in gating', () => {
  it('sends nothing by default, logged in or out, or before the session is known', () => {
    track('page_view');
    setAnalyticsSession(false, false);
    track('page_view');
    setAnalyticsSession(true, false);
    track('page_view');
    expect(beacon).not.toHaveBeenCalled();
    expect(analyticsEnabled()).toBe(false);
  });

  it('logged out, follows this browser’s choice, which takes effect at once', async () => {
    setAnalyticsSession(false, false);
    expect(writeDeviceOptIn(true)).toBe(true);
    expect(localStorage.getItem(ANALYTICS_OPT_IN_STORAGE_KEY)).toBe('true');
    track('page_view');
    expect(await sentJson()).toEqual([{ name: 'page_view', route_template: '/lines/[id]' }]);
    expect(beacon.mock.calls[0]![0]).toBe('/api/events');
    expect((sentBodies()[0] as Blob).type).toBe('application/json');

    beacon.mockClear();
    localStorage.setItem(OFFLINE_PENDING_STORAGE_KEY, '1');
    writeDeviceOptIn(false);
    expect(localStorage.getItem(ANALYTICS_OPT_IN_STORAGE_KEY)).toBeNull();
    expect(localStorage.getItem(OFFLINE_PENDING_STORAGE_KEY)).toBeNull();
    track('page_view');
    expect(beacon).not.toHaveBeenCalled();
  });

  it('holds events from before the session is known, and sends them only once it says opted in', async () => {
    writeDeviceOptIn(true);
    trackEmptyState('journeys_none');
    expect(beacon).not.toHaveBeenCalled();
    setAnalyticsSession(false, false);
    expect(await sentJson()).toEqual([
      { name: 'empty_state', route_template: '/lines/[id]', props: { kind: 'journeys_none' } },
    ]);

    resetAnalyticsForTests();
    beacon.mockClear();
    setCurrentRoute('/lines/[id]');
    trackEmptyState('journeys_none');
    setAnalyticsSession(true, false);
    setAccountOptIn(true);
    expect(beacon).not.toHaveBeenCalled();
  });

  it('treats any stored value but "true" as off', () => {
    setAnalyticsSession(false, false);
    localStorage.setItem(ANALYTICS_OPT_IN_STORAGE_KEY, 'yes');
    expect(readDeviceOptIn()).toBe(false);
    expect(analyticsEnabled()).toBe(false);
  });

  it('logged in, follows the account and ignores this browser’s choice', async () => {
    localStorage.setItem(ANALYTICS_OPT_IN_STORAGE_KEY, 'true');
    setAnalyticsSession(true, false);
    track('page_view');
    expect(beacon).not.toHaveBeenCalled();

    setAccountOptIn(true);
    trackFeature('plan', 'results');
    trackEmptyState('departures_none');
    expect(await sentJson()).toEqual([
      { name: 'feature', route_template: '/lines/[id]', props: { feature: 'plan', step: 'results' } },
      { name: 'empty_state', route_template: '/lines/[id]', props: { kind: 'departures_none' } },
    ]);
  });

  it.each([
    ['Global Privacy Control', { gpc: true }],
    ['Do Not Track', { dnt: '1' }],
  ])('sends nothing at all when the browser sends %s', (_name, signal) => {
    signals(signal);
    expect(privacySignalSet()).toBe(true);
    setAnalyticsSession(false, false);
    writeDeviceOptIn(true);
    track('page_view');
    setAnalyticsSession(true, true);
    track('page_view');
    expect(beacon).not.toHaveBeenCalled();
  });

  it('ignores Do Not Track set to anything but "1"', () => {
    signals({ dnt: '0', gpc: false });
    expect(privacySignalSet()).toBe(false);
  });

  it('falls back to a keepalive fetch when sendBeacon refuses', () => {
    beacon.mockReturnValueOnce(false);
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchMock);
    setAnalyticsSession(false, false);
    writeDeviceOptIn(true);
    track('offline_shown', undefined, '/offline');
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/events',
      expect.objectContaining({ method: 'POST', keepalive: true, credentials: 'same-origin' }),
    );
  });
});

describe('installApiErrorReporting', () => {
  it('reports a failed /api call by status class, and nothing else', async () => {
    const responses = [
      new Response('nope', { status: 503 }),
      new Response('ok', { status: 200 }),
      new Response('gone', { status: 404 }),
      new Response('elsewhere', { status: 500 }),
    ];
    const underlying = vi.fn(async () => responses.shift()!);
    vi.stubGlobal('fetch', underlying);
    const undo = installApiErrorReporting();
    try {
      setAnalyticsSession(false, false);
      writeDeviceOptIn(true);
      await fetch('/api/Train/12');
      await fetch('/api/preferences');
      await fetch(new URL('/api/lines/northern', window.location.origin));
      await fetch('https://api.anthropic.com/v1/messages');
      underlying.mockRejectedValueOnce(new TypeError('Failed to fetch'));
      await expect(fetch('/api/groups')).rejects.toThrow('Failed to fetch');
      underlying.mockRejectedValueOnce(Object.assign(new Error('aborted'), { name: 'AbortError' }));
      await expect(fetch('/api/groups')).rejects.toThrow('aborted');
    } finally {
      undo();
    }
    const sent = await sentJson();
    expect(sent).toEqual([
      { name: 'api_error', route_template: '/lines/[id]', props: { status: '5xx' } },
      { name: 'api_error', route_template: '/lines/[id]', props: { status: '4xx' } },
      { name: 'api_error', route_template: '/lines/[id]', props: { status: 'network' } },
    ]);
    // Nothing about the call itself.
    expect(JSON.stringify(sent)).not.toContain('Train');
  });

  it('reports nothing for a visitor who has not opted in', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('nope', { status: 500 })),
    );
    const undo = installApiErrorReporting();
    try {
      setAnalyticsSession(false, false);
      await fetch('/api/Train/12');
    } finally {
      undo();
    }
    expect(beacon).not.toHaveBeenCalled();
  });
});
