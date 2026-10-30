import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { act, fireEvent, screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import {
  ANALYTICS_OPT_IN_STORAGE_KEY,
  analyticsEnabled,
  resetAnalyticsForTests,
  setAnalyticsSession,
} from '@/lib/usageAnalytics';
import { AnalyticsOptIn, PRIVACY_SIGNAL_NOTE } from './AnalyticsOptIn';

function signals({ gpc, dnt }: { gpc?: boolean; dnt?: string }) {
  Object.defineProperty(navigator, 'globalPrivacyControl', { value: gpc, configurable: true });
  Object.defineProperty(navigator, 'doNotTrack', { value: dnt ?? null, configurable: true });
}

const toggle = () => screen.getByRole('switch', { name: 'Help improve Distant Signal' });

beforeEach(() => {
  resetAnalyticsForTests();
  localStorage.clear();
  signals({});
});

afterEach(() => {
  vi.unstubAllGlobals();
});

describe('AnalyticsOptIn', () => {
  it('logged out, is off by default and saves the choice in this browser', async () => {
    setAnalyticsSession(false, false);
    renderWithMantine(<AnalyticsOptIn session={{ signedIn: false, accountOptIn: false }} />);
    await waitFor(() => expect(toggle()).toBeEnabled());
    expect(toggle()).not.toBeChecked();
    expect(screen.getByText(/Saved in this browser\.$/)).toBeInTheDocument();

    fireEvent.click(toggle());
    expect(toggle()).toBeChecked();
    expect(localStorage.getItem(ANALYTICS_OPT_IN_STORAGE_KEY)).toBe('true');
    expect(analyticsEnabled()).toBe(true);

    fireEvent.click(toggle());
    expect(toggle()).not.toBeChecked();
    expect(localStorage.getItem(ANALYTICS_OPT_IN_STORAGE_KEY)).toBeNull();
    expect(analyticsEnabled()).toBe(false);
  });

  it('logged out, shows a choice already saved in this browser', async () => {
    localStorage.setItem(ANALYTICS_OPT_IN_STORAGE_KEY, 'true');
    renderWithMantine(<AnalyticsOptIn session={{ signedIn: false, accountOptIn: false }} />);
    await waitFor(() => expect(toggle()).toBeChecked());
  });

  it('logged in, saves the choice to the account and never to this browser', async () => {
    const fetchMock = vi.fn(async () => new Response(null, { status: 204 }));
    vi.stubGlobal('fetch', fetchMock);
    setAnalyticsSession(true, false);
    renderWithMantine(<AnalyticsOptIn session={{ signedIn: true, accountOptIn: false }} />);
    await waitFor(() => expect(toggle()).toBeEnabled());
    expect(screen.getByText(/Saved to your account\.$/)).toBeInTheDocument();
    await act(async () => {
      fireEvent.click(toggle());
    });
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/preferences/analytics',
      expect.objectContaining({ method: 'PUT', body: JSON.stringify({ optIn: true }) }),
    );
    expect(toggle()).toBeChecked();
    expect(analyticsEnabled()).toBe(true);
    expect(localStorage.getItem(ANALYTICS_OPT_IN_STORAGE_KEY)).toBeNull();
  });

  it('logged in, puts the switch back and says so when the save fails', async () => {
    vi.stubGlobal(
      'fetch',
      vi.fn(async () => new Response('', { status: 500 })),
    );
    setAnalyticsSession(true, false);
    renderWithMantine(<AnalyticsOptIn session={{ signedIn: true, accountOptIn: false }} />);
    await waitFor(() => expect(toggle()).toBeEnabled());
    await act(async () => {
      fireEvent.click(toggle());
    });
    expect(toggle()).not.toBeChecked();
    expect(screen.getByRole('alert')).toHaveTextContent("Couldn't save your choice. Try again.");
    expect(analyticsEnabled()).toBe(false);
  });

  it('without a session of its own, waits for the tracker before it can be used', async () => {
    renderWithMantine(<AnalyticsOptIn />);
    await act(async () => {});
    expect(toggle()).toBeDisabled();
    act(() => setAnalyticsSession(true, true));
    await waitFor(() => expect(toggle()).toBeEnabled());
    expect(toggle()).toBeChecked();
    expect(screen.getByText(/Saved to your account\.$/)).toBeInTheDocument();
  });

  it.each([
    ['Global Privacy Control', { gpc: true }],
    ['Do Not Track', { dnt: '1' }],
  ])('is off, disabled and says why when the browser sends %s', async (_name, signal) => {
    signals(signal);
    localStorage.setItem(ANALYTICS_OPT_IN_STORAGE_KEY, 'true');
    renderWithMantine(<AnalyticsOptIn session={{ signedIn: false, accountOptIn: false }} />);
    await waitFor(() => expect(screen.getByText(PRIVACY_SIGNAL_NOTE)).toBeInTheDocument());
    expect(toggle()).toBeDisabled();
    expect(toggle()).not.toBeChecked();
  });
});
