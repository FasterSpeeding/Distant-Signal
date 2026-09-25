import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { NotificationsToggle, urlBase64ToUint8Array } from './NotificationsToggle';

describe('urlBase64ToUint8Array', () => {
  it('decodes a plain base64url string with no padding characters needed', () => {
    // "AAECAw" (no '=' in the source) is the base64url encoding of the
    // four raw bytes [0, 1, 2, 3].
    expect(Array.from(urlBase64ToUint8Array('AAECAw'))).toEqual([0, 1, 2, 3]);
  });

  it('translates "-" and "_" to the standard base64 "+" and "/" alphabet', () => {
    // "_-8" is the base64url form of standard base64 "/+8=" -- both decode
    // to the same two bytes, [255, 239].
    expect(Array.from(urlBase64ToUint8Array('_-8'))).toEqual([255, 239]);
  });

  it('decodes a real-shaped 65-byte uncompressed P-256 VAPID public key', () => {
    // A well-known example VAPID public key (from the Push API reference
    // examples) -- a real key is always 65 bytes and starts with 0x04,
    // the uncompressed-point marker.
    const key = 'BEl62iUYgUivxIkv69yViEuiBIa40HI0DLLuxazjBk9j4H_hMVU2fV4kX_pMcFOxIRXVFrCzYqfE_ArNVjpJUBg';
    const decoded = urlBase64ToUint8Array(key);
    expect(decoded).toHaveLength(65);
    expect(decoded[0]).toBe(4);
  });
});

// LoginPromptModal's own LoginButtonLink calls useLoginHref(), which calls
// usePathname()/useSearchParams() -- same stub PinToggle.test.tsx uses for
// the same reason (these throw outside a real Next.js App Router tree).
vi.mock('next/navigation', () => ({
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('NotificationsToggle', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
    // @ts-expect-error -- undo the test-only global stubs below, so
    // "unsupported browser" stays the real jsdom baseline for every other
    // test file (jsdom has neither serviceWorker nor PushManager by
    // default).
    delete global.navigator.serviceWorker;
    // @ts-expect-error -- see above.
    delete global.window.PushManager;
    // @ts-expect-error -- see above.
    delete global.Notification;
  });

  it('renders a permanently disabled button when the browser has no PushManager/serviceWorker support', async () => {
    // jsdom has neither by default -- this is the real, unmocked baseline.
    // Review §2.11: the button is reserved in the DOM either way (never
    // `null`), so an unsupported browser still shows it, just disabled,
    // rather than the old "renders nothing" -- which was itself half of
    // the bug, since a *supported* browser also rendered nothing until its
    // mount effect resolved, popping the button in afterwards.
    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    expect(button).toBeDisabled();
  });

  function stubPushApiSupport() {
    const fakeRegistration = {
      pushManager: {
        subscribe: vi.fn().mockResolvedValue({
          endpoint: 'https://push.example/ep1',
          toJSON: () => ({ endpoint: 'https://push.example/ep1', keys: { p256dh: 'p', auth: 'a' } }),
        }),
      },
    };
    // @ts-expect-error -- test-only global stubs for Web APIs jsdom doesn't implement.
    global.navigator.serviceWorker = { ready: Promise.resolve(fakeRegistration) };
    // @ts-expect-error -- see above.
    global.window.PushManager = function () {};
    // @ts-expect-error -- see above.
    global.Notification = { requestPermission: vi.fn().mockResolvedValue('granted') };
  }

  it('reserves the button slot and enables it once support is confirmed', async () => {
    stubPushApiSupport();
    renderWithMantine(<NotificationsToggle />);
    // `getByRole`, not `findByRole`: the button must already be in the DOM
    // on the very first render (server-rendered slot reserved, before the
    // mount effect that confirms `supported` has even run) -- that's the
    // whole point of the fix. It starts disabled, per the surrounding
    // `!checked` guard.
    const button = screen.getByRole('button', { name: /enable notifications/i });
    expect(button).toBeInTheDocument();
    await waitFor(() => expect(button).not.toBeDisabled());
  });

  it('subscribes and shows LoginPromptModal on a 401 from the subscribe POST', async () => {
    stubPushApiSupport();
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValueOnce(new Response('test-vapid-key', { status: 200 })) // GET vapid-public-key
        .mockResolvedValueOnce(new Response(null, { status: 401 })), // POST subscribe
    );

    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByText(/log in to enable notifications/i)).toBeInTheDocument());
  });

  it('subscribes successfully on a 204 from the subscribe POST', async () => {
    stubPushApiSupport();
    vi.stubGlobal(
      'fetch',
      vi
        .fn()
        .mockResolvedValueOnce(new Response('test-vapid-key', { status: 200 }))
        .mockResolvedValueOnce(new Response(null, { status: 204 })),
    );

    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    fireEvent.click(button);

    await waitFor(() => expect(screen.getByRole('button', { name: /notifications enabled/i })).toBeInTheDocument());
  });

  // Bug: `enable()` was a bare `try`/`finally` with no `catch` and no error
  // state -- a rejected `pushManager.subscribe()` (common on Firefox/Brave
  // with push disabled, or a malformed VAPID key) threw as an unhandled
  // promise rejection, with the button just silently re-enabling and
  // nothing shown to the user.
  it('shows a clear error, not a raw exception, when pushManager.subscribe() rejects', async () => {
    const fakeRegistration = {
      pushManager: {
        subscribe: vi.fn().mockRejectedValue(new DOMException('push service error', 'AbortError')),
      },
    };
    // @ts-expect-error -- test-only global stub for a Web API jsdom doesn't implement.
    global.navigator.serviceWorker = { ready: Promise.resolve(fakeRegistration) };
    // @ts-expect-error -- see above.
    global.window.PushManager = function () {};
    // @ts-expect-error -- see above.
    global.Notification = { requestPermission: vi.fn().mockResolvedValue('granted') };
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('test-vapid-key', { status: 200 })));

    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    fireEvent.click(button);

    expect(
      await screen.findByText("Couldn't enable notifications. Check your browser's notification permissions."),
    ).toBeInTheDocument();
    // The button itself recovers rather than staying stuck disabled/loading
    // forever -- the `finally` block already reset `busy`; what was missing
    // was only the visible error.
    await waitFor(() => expect(button).not.toBeDisabled());
    expect(screen.getByRole('button', { name: /enable notifications/i })).toBeInTheDocument();
  });

  it('shows the same clear error on a non-ok vapid-key fetch', async () => {
    stubPushApiSupport();
    vi.stubGlobal('fetch', vi.fn().mockResolvedValue(new Response('boom', { status: 500 })));

    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    fireEvent.click(button);

    expect(
      await screen.findByText("Couldn't enable notifications. Check your browser's notification permissions."),
    ).toBeInTheDocument();
  });

  // Bug: `enabled` was never seeded from the real subscription state
  // (`pushManager.getSubscription()`), so a returning, already-subscribed
  // visitor always saw "Enable notifications" and would call `subscribe()`
  // again on every click instead of the button reflecting they're already
  // subscribed.
  it('seeds "Notifications enabled" on mount when a subscription already exists', async () => {
    const fakeSubscription = { endpoint: 'https://push.example/existing' };
    const fakeRegistration = {
      pushManager: { getSubscription: vi.fn().mockResolvedValue(fakeSubscription) },
    };
    // @ts-expect-error -- test-only global stub for a Web API jsdom doesn't implement.
    global.navigator.serviceWorker = { getRegistration: vi.fn().mockResolvedValue(fakeRegistration) };
    // @ts-expect-error -- see above.
    global.window.PushManager = function () {};

    renderWithMantine(<NotificationsToggle />);

    expect(await screen.findByRole('button', { name: /notifications enabled/i })).toBeInTheDocument();
  });

  it('leaves "Enable notifications" on mount when getRegistration resolves with no subscription', async () => {
    const fakeRegistration = {
      pushManager: { getSubscription: vi.fn().mockResolvedValue(null) },
    };
    // @ts-expect-error -- test-only global stub for a Web API jsdom doesn't implement.
    global.navigator.serviceWorker = { getRegistration: vi.fn().mockResolvedValue(fakeRegistration) };
    // @ts-expect-error -- see above.
    global.window.PushManager = function () {};

    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    await waitFor(() => expect(button).not.toBeDisabled());
    expect(screen.queryByRole('button', { name: /notifications enabled/i })).not.toBeInTheDocument();
  });

  it('does nothing further when the permission prompt is denied', async () => {
    stubPushApiSupport();
    // @ts-expect-error -- test-only global stub for a Web API jsdom doesn't implement.
    global.Notification = { requestPermission: vi.fn().mockResolvedValue('denied') };
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<NotificationsToggle />);
    const button = await screen.findByRole('button', { name: /enable notifications/i });
    fireEvent.click(button);

    await waitFor(() => expect(button).not.toBeDisabled());
    expect(fetchMock).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: /enable notifications/i })).toBeInTheDocument();
  });
});
