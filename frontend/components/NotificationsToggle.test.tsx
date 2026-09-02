import { describe, it, expect, vi, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { NotificationsToggle } from './NotificationsToggle';

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
    // @ts-expect-error
    delete global.window.PushManager;
    // @ts-expect-error
    delete global.Notification;
  });

  it('renders nothing when the browser has no PushManager/serviceWorker support', () => {
    // jsdom has neither by default -- this is the real, unmocked baseline.
    renderWithMantine(<NotificationsToggle />);
    expect(screen.queryByRole('button', { name: /enable notifications/i })).not.toBeInTheDocument();
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
    // @ts-expect-error
    global.window.PushManager = function () {};
    // @ts-expect-error
    global.Notification = { requestPermission: vi.fn().mockResolvedValue('granted') };
  }

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

  it('does nothing further when the permission prompt is denied', async () => {
    stubPushApiSupport();
    // @ts-expect-error
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
