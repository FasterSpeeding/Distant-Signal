import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { render } from '@testing-library/react';
import {
  ANALYTICS_OPT_IN_STORAGE_KEY,
  OFFLINE_PENDING_STORAGE_KEY,
  resetAnalyticsForTests,
} from '@/lib/usageAnalytics';
import { UsageTracker } from './UsageTracker';

let pathname = '/lines/northern';
vi.mock('next/navigation', () => ({ usePathname: () => pathname }));

const beacon = vi.fn((_url: string, _data?: BodyInit | null) => true);

function blobText(blob: Blob): Promise<string> {
  return new Promise((resolve) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.readAsText(blob);
  });
}

async function sent(): Promise<unknown[]> {
  return Promise.all(beacon.mock.calls.map(async ([, blob]) => JSON.parse(await blobText(blob as Blob)) as unknown));
}

beforeEach(() => {
  resetAnalyticsForTests();
  localStorage.clear();
  beacon.mockClear();
  pathname = '/lines/northern';
  Object.defineProperty(navigator, 'sendBeacon', { value: beacon, configurable: true });
  Object.defineProperty(navigator, 'globalPrivacyControl', { value: undefined, configurable: true });
  Object.defineProperty(navigator, 'doNotTrack', { value: null, configurable: true });
  vi.stubGlobal(
    'matchMedia',
    vi.fn(() => ({ matches: false })),
  );
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('UsageTracker', () => {
  it('sends nothing for a visitor who has not opted in', () => {
    localStorage.setItem(OFFLINE_PENDING_STORAGE_KEY, '1');
    const { rerender } = render(<UsageTracker signedIn={false} accountOptIn={false} />);
    pathname = '/stations/KGX';
    rerender(<UsageTracker signedIn={false} accountOptIn={false} />);
    render(<UsageTracker signedIn={true} accountOptIn={false} />);
    expect(beacon).not.toHaveBeenCalled();
  });

  it('sends page views by route template, the display mode once, and a pending offline page', async () => {
    localStorage.setItem(ANALYTICS_OPT_IN_STORAGE_KEY, 'true');
    localStorage.setItem(OFFLINE_PENDING_STORAGE_KEY, '1');
    const { rerender } = render(<UsageTracker signedIn={false} accountOptIn={false} />);
    pathname = '/stations/KGX';
    rerender(<UsageTracker signedIn={false} accountOptIn={false} />);
    expect(await sent()).toEqual([
      { name: 'page_view', route_template: '/lines/[id]' },
      { name: 'display_mode', route_template: '/lines/[id]', props: { mode: 'browser' } },
      { name: 'offline_shown', route_template: '/offline' },
      { name: 'page_view', route_template: '/stations/[crs]' },
    ]);
    expect(localStorage.getItem(OFFLINE_PENDING_STORAGE_KEY)).toBeNull();
  });

  it('follows the account for a logged-in visitor', async () => {
    render(<UsageTracker signedIn={true} accountOptIn={true} />);
    const events = await sent();
    expect(events[0]).toEqual({ name: 'page_view', route_template: '/lines/[id]' });
  });
});
