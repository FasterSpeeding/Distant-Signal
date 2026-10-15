import { describe, it, expect, vi, afterEach, beforeEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { NotificationsToggle } from './NotificationsToggle';
import {
  BLOCKED_FIREFOX_MESSAGE,
  BLOCKED_MESSAGE,
  GENERIC_ENABLE_ERROR,
  UNSUPPORTED_IOS_MESSAGE,
  UNSUPPORTED_MESSAGE,
} from './enableNotifications';

// LoginPromptModal's own LoginButtonLink calls useLoginHref(), which calls
// usePathname()/useSearchParams() -- same stub PinToggle.test.tsx uses for
// the same reason (these throw outside a real Next.js App Router tree).
vi.mock('next/navigation', () => ({
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

const FIREFOX_ANDROID_UA = 'Mozilla/5.0 (Android 14; Mobile; rv:131.0) Gecko/131.0 Firefox/131.0';
const CHROME_ANDROID_UA =
  'Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/129.0.0.0 Mobile Safari/537.36';
const IPHONE_UA =
  'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1';

const SUBSCRIPTION = {
  endpoint: 'https://updates.push.services.mozilla.com/wpush/v2/secret-endpoint',
  toJSON: () => ({
    endpoint: 'https://updates.push.services.mozilla.com/wpush/v2/secret-endpoint',
    keys: { p256dh: 'p', auth: 'a' },
  }),
};

// A real-shaped VAPID key, so `urlBase64ToUint8Array` decodes it.
const VAPID_KEY = 'BEl62iUYgUivxIkv69yViEuiBIa40HI0DLLuxazjBk9j4H_hMVU2fV4kX_pMcFOxIRXVFrCzYqfE_ArNVjpJUBg';

/** One event in the order the browser APIs were called, to check the
 * gesture ordering. */
let calls: string[] = [];

interface Stubs {
  permission?: NotificationPermission;
  /** What `requestPermission()` answers; defaults to `permission`. */
  answer?: NotificationPermission;
  subscribe?: ReturnType<typeof vi.fn>;
  getSubscription?: ReturnType<typeof vi.fn>;
  /** Whether `requestPermission()` only takes a callback (older Safari). */
  callbackOnly?: boolean;
}

function stubPushApi(stubs: Stubs = {}) {
  const permissionState = { value: stubs.permission ?? 'default' };
  const answer = stubs.answer ?? stubs.permission ?? 'granted';
  const registration = {
    pushManager: {
      subscribe:
        stubs.subscribe ??
        vi.fn(() => {
          calls.push('subscribe');
          return Promise.resolve(SUBSCRIPTION);
        }),
      getSubscription: stubs.getSubscription ?? vi.fn().mockResolvedValue(null),
    },
  };
  const requestPermission = vi.fn((callback?: (permission: NotificationPermission) => void) => {
    calls.push('requestPermission');
    permissionState.value = answer;
    if (stubs.callbackOnly) {
      queueMicrotask(() => callback?.(answer));
      return undefined;
    }
    return Promise.resolve(answer);
  });
  Object.defineProperty(navigator, 'serviceWorker', {
    configurable: true,
    value: {
      ready: Promise.resolve(registration),
      getRegistration: vi.fn().mockResolvedValue(registration),
    },
  });
  // @ts-expect-error -- test-only global stub for a Web API jsdom doesn't implement.
  window.PushManager = function () {};
  vi.stubGlobal('Notification', {
    requestPermission,
    get permission() {
      return permissionState.value;
    },
  });
  return { registration, requestPermission };
}

function stubUserAgent(userAgent: string, maxTouchPoints = 0) {
  vi.spyOn(navigator, 'userAgent', 'get').mockReturnValue(userAgent);
  Object.defineProperty(navigator, 'maxTouchPoints', { configurable: true, value: maxTouchPoints });
}

/** A fetch mock for the two endpoints, recording each call in `calls`. */
function stubFetch({ key = () => new Response(VAPID_KEY), save = () => new Response(null, { status: 204 }) } = {}) {
  const fetchMock = vi.fn((input: string) => {
    calls.push(`fetch ${input}`);
    if (input.endsWith('/vapid-public-key')) return Promise.resolve(key());
    return Promise.resolve(save());
  });
  vi.stubGlobal('fetch', fetchMock);
  return fetchMock;
}

async function clickEnable() {
  const button = await screen.findByRole('button', { name: /enable notifications/i });
  await waitFor(() => expect(button).not.toBeDisabled());
  // Let the mount effect's key prefetch settle, as it has by the time a
  // person taps.
  await waitFor(() => expect(calls).toContain('fetch /api/notifications/vapid-public-key'));
  fireEvent.click(button);
  return button;
}

describe('NotificationsToggle', () => {
  beforeEach(() => {
    calls = [];
    vi.spyOn(console, 'warn').mockImplementation(() => {});
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
    // Undo the stubs so "unsupported browser" stays the real jsdom
    // baseline for every other test (jsdom has neither serviceWorker nor
    // PushManager).
    // @ts-expect-error -- see above.
    delete navigator.serviceWorker;
    // @ts-expect-error -- see above.
    delete window.PushManager;
    // @ts-expect-error -- see above (`stubUserAgent`).
    delete navigator.maxTouchPoints;
  });

  describe('unsupported browser', () => {
    it('renders a disabled button and says the browser cannot show notifications', async () => {
      // jsdom has no serviceWorker or PushManager: the real baseline.
      renderWithMantine(<NotificationsToggle />);
      const button = await screen.findByRole('button', { name: /enable notifications/i });
      expect(button).toBeDisabled();
      expect(await screen.findByText(UNSUPPORTED_MESSAGE)).toBeInTheDocument();
    });

    it('tells iPhone users to add the site to the home screen', async () => {
      stubUserAgent(IPHONE_UA, 5);
      renderWithMantine(<NotificationsToggle />);
      expect(await screen.findByText(UNSUPPORTED_IOS_MESSAGE)).toBeInTheDocument();
      expect(screen.getByRole('button', { name: /enable notifications/i })).toBeDisabled();
    });
  });

  it('reserves the button slot and enables it once support is confirmed', async () => {
    stubPushApi();
    stubFetch();
    renderWithMantine(<NotificationsToggle />);
    // `getByRole`, not `findByRole`: the button must be in the DOM on the
    // very first render, disabled until the mount effect runs.
    const button = screen.getByRole('button', { name: /enable notifications/i });
    await waitFor(() => expect(button).not.toBeDisabled());
    expect(screen.queryByText(UNSUPPORTED_MESSAGE)).not.toBeInTheDocument();
  });

  describe('gesture ordering', () => {
    it('calls requestPermission synchronously in the click, before any fetch or await', async () => {
      const { requestPermission } = stubPushApi();
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      // Synchronously: no await has run since the click event.
      expect(requestPermission).toHaveBeenCalledTimes(1);
      expect(calls.at(-1)).toBe('requestPermission');
      await screen.findByRole('button', { name: /notifications enabled/i });
      const afterClick = calls.slice(calls.indexOf('requestPermission'));
      // The key was fetched before the click, so nothing but subscribe
      // and the save sit between the permission answer and the end.
      expect(afterClick).toEqual(['requestPermission', 'subscribe', 'fetch /api/notifications/subscribe']);
    });

    // Firefox for Android: subscribe() rejects with NotAllowedError once
    // the tap's user activation has expired. Here it expires at the next
    // macrotask, and the network answers only after a macrotask, so any
    // fetch between the tap and subscribe() loses it, as the old flow did.
    it('subscribes while the user activation is still active (Firefox for Android)', async () => {
      stubUserAgent(FIREFOX_ANDROID_UA);
      let activationActive = false;
      const subscribe = vi.fn(() => {
        calls.push('subscribe');
        return activationActive
          ? Promise.resolve(SUBSCRIPTION)
          : Promise.reject(new DOMException('User denied permission to use the Push API.', 'NotAllowedError'));
      });
      stubPushApi({ permission: 'granted', subscribe });
      vi.stubGlobal(
        'fetch',
        vi.fn(
          (input: string) =>
            new Promise((resolve) =>
              setTimeout(
                () =>
                  resolve(
                    input.endsWith('/vapid-public-key') ? new Response(VAPID_KEY) : new Response(null, { status: 204 }),
                  ),
                5,
              ),
            ),
        ),
      );
      renderWithMantine(<NotificationsToggle />);
      const button = await screen.findByRole('button', { name: /enable notifications/i });
      await waitFor(() => expect(button).not.toBeDisabled());
      // Let the prefetch land.
      await new Promise((resolve) => setTimeout(resolve, 20));

      activationActive = true;
      fireEvent.click(button);
      setTimeout(() => {
        activationActive = false;
      }, 0);

      expect(await screen.findByRole('button', { name: /notifications enabled/i })).toBeInTheDocument();
      expect(subscribe).toHaveBeenCalledTimes(1);
    });
  });

  describe('permission', () => {
    it('shows the Firefox steps when permission is denied in Firefox', async () => {
      stubUserAgent(FIREFOX_ANDROID_UA);
      stubPushApi({ permission: 'denied' });
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByText(BLOCKED_FIREFOX_MESSAGE)).toBeInTheDocument();
      expect(calls).not.toContain('subscribe');
    });

    it('shows the general steps when permission is denied in another browser', async () => {
      stubUserAgent(CHROME_ANDROID_UA);
      stubPushApi({ permission: 'denied' });
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByText(BLOCKED_MESSAGE)).toBeInTheDocument();
    });

    it('shows nothing when the prompt is dismissed', async () => {
      stubPushApi({ answer: 'default' });
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      const button = await clickEnable();
      await waitFor(() => expect(button).not.toBeDisabled());
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
      expect(calls).not.toContain('subscribe');
    });

    it('works with the callback-only requestPermission of older Safari', async () => {
      stubPushApi({ callbackOnly: true });
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByRole('button', { name: /notifications enabled/i })).toBeInTheDocument();
    });
  });

  describe('subscribe', () => {
    it('passes the VAPID key as a Uint8Array', async () => {
      const { registration } = stubPushApi();
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      await screen.findByRole('button', { name: /notifications enabled/i });
      const options = registration.pushManager.subscribe.mock.calls[0]![0] as PushSubscriptionOptionsInit;
      expect(options.userVisibleOnly).toBe(true);
      expect(options.applicationServerKey).toBeInstanceOf(Uint8Array);
      expect((options.applicationServerKey as Uint8Array).length).toBe(65);
    });

    it('shows "try again" with a reference, not a permissions message, when subscribe fails', async () => {
      stubUserAgent(FIREFOX_ANDROID_UA);
      stubPushApi({
        permission: 'granted',
        subscribe: vi.fn().mockRejectedValue(new DOMException('Error retrieving push subscription.', 'AbortError')),
      });
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      const button = await clickEnable();
      expect(await screen.findByText(GENERIC_ENABLE_ERROR)).toBeInTheDocument();
      expect(screen.getByText('Reference: subscribe-AbortError')).toBeInTheDocument();
      expect(screen.queryByText(BLOCKED_FIREFOX_MESSAGE)).not.toBeInTheDocument();
      // The button recovers.
      await waitFor(() => expect(button).not.toBeDisabled());
    });

    it('shows "try again" for a NotAllowedError while permission is granted (lost activation)', async () => {
      stubUserAgent(FIREFOX_ANDROID_UA);
      stubPushApi({
        permission: 'granted',
        subscribe: vi.fn().mockRejectedValue(new DOMException('User denied permission.', 'NotAllowedError')),
      });
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByText(GENERIC_ENABLE_ERROR)).toBeInTheDocument();
      expect(screen.getByText('Reference: subscribe-NotAllowedError')).toBeInTheDocument();
    });

    it('shows the blocked steps for a NotAllowedError when permission is denied', async () => {
      stubUserAgent(FIREFOX_ANDROID_UA);
      const subscribe = vi.fn(() => {
        vi.stubGlobal('Notification', { requestPermission: vi.fn(), permission: 'denied' });
        return Promise.reject(new DOMException('User denied permission.', 'NotAllowedError'));
      });
      stubPushApi({ permission: 'granted', subscribe });
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByText(BLOCKED_FIREFOX_MESSAGE)).toBeInTheDocument();
    });

    it('replaces a subscription made with an old key (InvalidStateError) and retries once', async () => {
      const stale = { unsubscribe: vi.fn().mockResolvedValue(true) };
      const subscribe = vi
        .fn()
        .mockRejectedValueOnce(new DOMException('different applicationServerKey', 'InvalidStateError'))
        .mockResolvedValueOnce(SUBSCRIPTION);
      // None on mount (else the button seeds as enabled), the stale one
      // after the InvalidStateError.
      const getSubscription = vi.fn().mockResolvedValueOnce(null).mockResolvedValue(stale);
      stubPushApi({ subscribe, getSubscription });
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByRole('button', { name: /notifications enabled/i })).toBeInTheDocument();
      expect(stale.unsubscribe).toHaveBeenCalledTimes(1);
      expect(subscribe).toHaveBeenCalledTimes(2);
    });
  });

  describe('VAPID key', () => {
    it('refetches the key on click when the prefetch failed', async () => {
      stubPushApi();
      let keyCalls = 0;
      stubFetch({
        key: () => (++keyCalls === 1 ? new Response('boom', { status: 500 }) : new Response(VAPID_KEY)),
      });
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByRole('button', { name: /notifications enabled/i })).toBeInTheDocument();
      expect(keyCalls).toBe(2);
    });

    it('shows "try again" with a key reference when the key cannot be fetched', async () => {
      stubPushApi();
      stubFetch({ key: () => new Response('boom', { status: 500 }) });
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByText(GENERIC_ENABLE_ERROR)).toBeInTheDocument();
      expect(screen.getByText('Reference: key-Error')).toBeInTheDocument();
    });
  });

  describe('saving the subscription', () => {
    it('subscribes successfully on a 204', async () => {
      stubPushApi();
      stubFetch();
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByRole('button', { name: /notifications enabled/i })).toBeInTheDocument();
    });

    it('shows LoginPromptModal on a 401', async () => {
      stubPushApi();
      stubFetch({ save: () => new Response(null, { status: 401 }) });
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByText(/log in to enable notifications/i)).toBeInTheDocument();
      expect(screen.queryByRole('alert')).not.toBeInTheDocument();
    });

    it("shows the API's own sentence on a 409", async () => {
      stubPushApi();
      stubFetch({
        save: () => new Response('that push endpoint is already registered to a different account', { status: 409 }),
      });
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(
        await screen.findByText('That push endpoint is already registered to a different account.'),
      ).toBeInTheDocument();
    });

    it('shows "try again" on a 500', async () => {
      stubPushApi();
      stubFetch({ save: () => new Response('boom', { status: 500 }) });
      renderWithMantine(<NotificationsToggle />);
      await clickEnable();
      expect(await screen.findByText(GENERIC_ENABLE_ERROR)).toBeInTheDocument();
      expect(screen.getByText('Reference: save-Error')).toBeInTheDocument();
    });

    it('shows "try again" on a network failure', async () => {
      stubPushApi();
      const fetchMock = stubFetch();
      renderWithMantine(<NotificationsToggle />);
      const button = await screen.findByRole('button', { name: /enable notifications/i });
      await waitFor(() => expect(fetchMock).toHaveBeenCalled());
      fetchMock.mockImplementation(() =>
        Promise.reject(new TypeError('NetworkError when attempting to fetch resource.')),
      );
      fireEvent.click(button);
      expect(await screen.findByText(GENERIC_ENABLE_ERROR)).toBeInTheDocument();
      expect(screen.getByText('Reference: save-TypeError')).toBeInTheDocument();
    });
  });

  it('logs the failure detail without the push endpoint or keys', async () => {
    stubUserAgent(FIREFOX_ANDROID_UA);
    stubPushApi({ permission: 'granted' });
    stubFetch({ save: () => new Response('boom', { status: 500 }) });
    renderWithMantine(<NotificationsToggle />);
    await clickEnable();
    await screen.findByText(GENERIC_ENABLE_ERROR);
    const warn = vi.mocked(console.warn);
    expect(warn).toHaveBeenCalledWith(
      'Enabling notifications failed',
      expect.objectContaining({
        step: 'save',
        errorName: 'Error',
        errorMessage: 'subscribe returned HTTP 500',
        permission: 'granted',
        userAgent: FIREFOX_ANDROID_UA,
      }),
    );
    const logged = JSON.stringify(warn.mock.calls);
    expect(logged).not.toContain('secret-endpoint');
    expect(logged).not.toContain(VAPID_KEY);
  });

  // Bug: `enabled` was never seeded from the real subscription state.
  it('seeds "Notifications enabled" on mount when a subscription already exists', async () => {
    stubPushApi({ getSubscription: vi.fn().mockResolvedValue(SUBSCRIPTION) });
    stubFetch();
    renderWithMantine(<NotificationsToggle />);
    expect(await screen.findByRole('button', { name: /notifications enabled/i })).toBeInTheDocument();
  });

  it('leaves "Enable notifications" on mount when there is no subscription', async () => {
    stubPushApi();
    stubFetch();
    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    await waitFor(() => expect(button).not.toBeDisabled());
    expect(screen.queryByRole('button', { name: /notifications enabled/i })).not.toBeInTheDocument();
  });
});
