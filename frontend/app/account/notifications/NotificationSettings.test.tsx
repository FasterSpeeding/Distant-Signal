import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, screen, waitFor, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import type { NotificationPreferences, PushDevice } from '@/lib/types';
import { NotificationSettings } from './NotificationSettings';

// NotificationsToggle's login prompt reads the router hooks.
vi.mock('next/navigation', () => ({
  usePathname: () => '/account/notifications',
  useSearchParams: () => new URLSearchParams(''),
}));

const SETTINGS: NotificationPreferences = {
  journeySuggestions: true,
  lineStatus: true,
  trainDelay: true,
  trainCancel: true,
  predictedStart: true,
  stationSkip: true,
  templateAttention: true,
  delayThresholdMinutes: null,
  quietHours: null,
};

const DEVICES: PushDevice[] = [
  {
    id: 4,
    label: 'Chrome on Android',
    createdAt: '2026-10-01T08:00:00Z',
    lastUsedAt: '2026-10-20T08:00:00Z',
    endpointSha256: 'a'.repeat(64),
  },
  {
    id: 5,
    label: null,
    createdAt: '2026-09-01T08:00:00Z',
    lastUsedAt: '2026-09-02T08:00:00Z',
    endpointSha256: 'b'.repeat(64),
  },
];

type Handler = (init: RequestInit | undefined) => Response;

/** A fetch mock answering each `METHOD url` from `routes`; the settings
 * PUT answers with the stored settings merged with the body by default. */
function stubFetch(routes: Record<string, Handler> = {}) {
  let stored = { ...SETTINGS };
  const fetchMock = vi.fn(async (input: RequestInfo | URL, init?: RequestInit) => {
    const key = `${init?.method ?? 'GET'} ${String(input)}`;
    const handler = routes[key];
    if (handler) return handler(init);
    if (key === 'GET /api/preferences/notifications') return Response.json(stored);
    if (key === 'PUT /api/preferences/notifications') {
      stored = { ...stored, ...(JSON.parse(String(init?.body)) as Partial<NotificationPreferences>) };
      return Response.json(stored);
    }
    if (key === 'GET /api/notifications/devices') return Response.json(DEVICES);
    return new Response(null, { status: 204 });
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

function putBodies(fetchMock: ReturnType<typeof stubFetch>, url: string): unknown[] {
  return fetchMock.mock.calls
    .filter(([input, init]) => String(input) === url && init?.method === 'PUT')
    .map(([, init]) => JSON.parse(String(init?.body)) as unknown);
}

/** The delay threshold's input (its listbox shares the label). */
function thresholdInput(): HTMLElement {
  const input = screen.getAllByLabelText('Delay threshold').find((element) => element.tagName === 'INPUT');
  if (!input) throw new Error('no delay threshold input');
  return input;
}

const LINES = [
  { id: 'lner-ecml', name: 'East Coast Main Line', alerts: true },
  { id: 'metrolink:blue', name: 'Blue line', alerts: false },
];

describe('NotificationSettings', () => {
  beforeEach(() => {
    vi.spyOn(Intl.DateTimeFormat.prototype, 'resolvedOptions').mockReturnValue({
      ...new Intl.DateTimeFormat().resolvedOptions(),
      timeZone: 'Europe/London',
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it('shows the saved settings and saves a switch on its own', async () => {
    const fetchMock = stubFetch();
    renderWithMantine(<NotificationSettings pinnedLines={LINES} />);
    const delays = screen.getByRole('switch', { name: /^Train delays/ });
    await waitFor(() => expect(delays).toBeEnabled());
    expect(delays).toBeChecked();
    expect(screen.getByRole('switch', { name: /^Cancellations/ })).toBeChecked();
    expect(thresholdInput()).toHaveValue('15 minutes');

    fireEvent.click(delays);
    await waitFor(() => expect(delays).not.toBeChecked());
    expect(putBodies(fetchMock, '/api/preferences/notifications')).toEqual([{ trainDelay: false }]);
    // The threshold only matters for delays.
    expect(thresholdInput()).toBeDisabled();
  });

  it('explains that suggestions replace the plain train notification', () => {
    stubFetch();
    renderWithMantine(<NotificationSettings pinnedLines={[]} />);
    expect(screen.getByText(/in place of that train's delay or cancellation notification/)).toBeInTheDocument();
    expect(screen.getByText(/These come through even in quiet hours/)).toBeInTheDocument();
  });

  it('puts a switch back and says so when the save fails', async () => {
    stubFetch({ 'PUT /api/preferences/notifications': () => new Response('', { status: 500 }) });
    renderWithMantine(<NotificationSettings pinnedLines={[]} />);
    const skips = screen.getByRole('switch', { name: /^Skipped stops/ });
    await waitFor(() => expect(skips).toBeEnabled());
    fireEvent.click(skips);
    expect(await screen.findByText("Couldn't save this setting. Try again.")).toBeInTheDocument();
    expect(skips).toBeChecked();
  });

  it('turns quiet hours on in this browser time zone', async () => {
    const fetchMock = stubFetch();
    renderWithMantine(<NotificationSettings pinnedLines={[]} />);
    const quiet = screen.getByRole('switch', { name: 'Quiet hours' });
    await waitFor(() => expect(quiet).toBeEnabled());
    expect(screen.getByText('Times are in Europe/London.')).toBeInTheDocument();
    fireEvent.click(quiet);
    await waitFor(() => expect(quiet).toBeChecked());
    expect(putBodies(fetchMock, '/api/preferences/notifications')).toEqual([
      { quietHours: { start: '22:00', end: '07:00', timezone: 'Europe/London' } },
    ]);
    fireEvent.click(quiet);
    await waitFor(() => expect(quiet).not.toBeChecked());
    expect(putBodies(fetchMock, '/api/preferences/notifications').at(-1)).toEqual({ quietHours: null });
  });

  it('switches a pinned line alerts on its own', async () => {
    const fetchMock = stubFetch();
    renderWithMantine(<NotificationSettings pinnedLines={LINES} />);
    const blue = screen.getByRole('switch', { name: 'Blue line' });
    expect(blue).not.toBeChecked();
    expect(screen.getByRole('switch', { name: 'East Coast Main Line' })).toBeChecked();
    await waitFor(() => expect(blue).toBeEnabled());
    fireEvent.click(blue);
    await waitFor(() => expect(blue).toBeChecked());
    expect(putBodies(fetchMock, '/api/preferences/pinned-lines/metrolink%3Ablue/alerts')).toEqual([{ notify: true }]);
  });

  it('says how to get line alerts with no line pinned', () => {
    stubFetch();
    renderWithMantine(<NotificationSettings pinnedLines={[]} />);
    expect(screen.getByText('Pin a line from its page to get alerts about it.')).toBeInTheDocument();
  });

  it('lists the devices and removes one', async () => {
    const fetchMock = stubFetch();
    renderWithMantine(<NotificationSettings pinnedLines={[]} />);
    const devices = within(screen.getByRole('region', { name: 'Devices' }));
    expect(await devices.findByText('Chrome on Android')).toBeInTheDocument();
    expect(devices.getByText('Unknown browser')).toBeInTheDocument();
    fireEvent.click(devices.getByRole('button', { name: 'Remove unknown browser' }));
    await waitFor(() => expect(devices.queryByText('Unknown browser')).not.toBeInTheDocument());
    expect(fetchMock).toHaveBeenCalledWith('/api/notifications/devices/5', { method: 'DELETE' });
    expect(devices.getByText('Chrome on Android')).toBeInTheDocument();
  });

  it('marks this device and offers no remove for it', async () => {
    const endpoint = 'https://push.example/this-one';
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(endpoint));
    const hash = Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
    Object.defineProperty(navigator, 'serviceWorker', {
      configurable: true,
      value: {
        getRegistration: vi.fn().mockResolvedValue({
          pushManager: { getSubscription: vi.fn().mockResolvedValue({ endpoint }) },
        }),
      },
    });
    try {
      stubFetch({
        'GET /api/notifications/devices': () => Response.json([{ ...DEVICES[0], endpointSha256: hash }]),
      });
      renderWithMantine(<NotificationSettings pinnedLines={[]} />);
      const devices = within(screen.getByRole('region', { name: 'Devices' }));
      expect(await devices.findByText('This device')).toBeInTheDocument();
      expect(devices.queryByRole('button', { name: /^Remove/ })).not.toBeInTheDocument();
    } finally {
      // @ts-expect-error -- undo the test-only stub (jsdom has no serviceWorker).
      delete navigator.serviceWorker;
    }
  });
});
