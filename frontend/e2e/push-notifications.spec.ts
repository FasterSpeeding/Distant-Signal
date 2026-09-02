import { test, expect } from '@playwright/test';
import type { Page, Worker } from '@playwright/test';

// Exercises the real push/notificationclick listeners registered in
// frontend/public/sw.js (docs/superpowers/plans/
// 2026-09-02-line-status-notifications.md, Task 9) inside the actual
// browser ServiceWorkerGlobalScope -- something jsdom-based Vitest tests
// cannot do at all (no real PushEvent/NotificationEvent/ServiceWorker
// globals exist there). Real end-to-end push DELIVERY (a live push
// service round-tripping a VAPID-signed message from crates/notifier)
// still needs a real device/browser and real VAPID keys -- that is not
// reproducible in this environment, and is exactly what
// docs/superpowers/plans/2026-09-02-line-status-notifications.md's
// Task 10 (manual verification) covers instead. What IS reproducible
// here: dispatching spec-shaped events at the real, already-registered
// service worker and asserting its actual handler code (not a mock of
// it) reacts correctly -- these tests run the production sw.js source
// directly, inside a real Chromium service worker.
//
// tsconfig.json's `lib` is `["dom", "dom.iterable", "esnext"]` -- it
// deliberately has no `webworker` lib, since this is a Next.js app, not a
// worker script. That means `self.clients`/`self.registration`/
// `PushEvent` have no ambient types here even though they're real at
// runtime inside `worker.evaluate()` (a real ServiceWorkerGlobalScope).
// Each evaluate callback below routes through one local `sw = self as
// any` for that reason, rather than fighting the ambient `Window` typing
// self otherwise gets from the `dom` lib.
//
// `notificationclick`'s real event type (`NotificationEvent`) can't be
// constructed in a ServiceWorkerGlobalScope with a real `Notification`
// instance (the `Notification` constructor throws there by spec -- it's
// only exposed to Window/Worker, not ServiceWorkerGlobalScope), so that
// test dispatches a plain `Event` with a hand-attached `notification`
// property shaped like the real thing. That's a synthetic double for the
// notification object specifically, not for sw.js's own listener code,
// which still runs for real.

async function getServiceWorker(page: Page): Promise<Worker> {
  const context = page.context();
  const existing = context.serviceWorkers();
  if (existing.length > 0) {
    return existing[0];
  }
  return context.waitForEvent('serviceworker');
}

test.describe('push notification handlers (sw.js)', () => {
  test('push event with no payload is ignored without throwing', async ({ page }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));
    const worker = await getServiceWorker(page);

    const calls = await worker.evaluate(async () => {
      const sw = self as any; // eslint: see file header comment
      const calls: unknown[] = [];
      sw.registration.showNotification = (...args: unknown[]) => {
        calls.push(args);
        return Promise.resolve();
      };

      // No `data` option at all -- PushEvent.data is then `null`, matching
      // a real push service sending an empty message body.
      const event = new sw.PushEvent('push', {});
      sw.dispatchEvent(event);
      // No waitUntil is ever called on this path (the handler returns
      // before reaching it) -- a short tick is enough to prove nothing
      // async was kicked off either.
      await new Promise((resolve) => setTimeout(resolve, 50));
      return calls;
    });

    expect(calls).toEqual([]);
  });

  test('push event shows a notification when no focused tab already has the url open', async ({ page }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));
    const worker = await getServiceWorker(page);

    const calls = await worker.evaluate(async () => {
      const sw = self as any;
      const calls: { title: string; options: unknown }[] = [];
      sw.registration.showNotification = (title: string, options: unknown) => {
        calls.push({ title, options });
        return Promise.resolve();
      };
      sw.clients.matchAll = async () => [];

      const payload = { title: 'Bakerloo line', body: 'Severe delays', url: '/lines/bakerloo', tag: 'line-bakerloo' };
      const event = new sw.PushEvent('push', { data: JSON.stringify(payload) });
      let waited: Promise<unknown> | undefined;
      event.waitUntil = (p: Promise<unknown>) => {
        waited = p;
        return p;
      };
      sw.dispatchEvent(event);
      await waited;
      return calls;
    });

    expect(calls).toEqual([
      { title: 'Bakerloo line', options: { body: 'Severe delays', tag: 'line-bakerloo', data: { url: '/lines/bakerloo' } } },
    ]);
  });

  test('push event is skipped when a focused tab already has the exact url open', async ({ page }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));
    const worker = await getServiceWorker(page);

    const calls = await worker.evaluate(async () => {
      const sw = self as any;
      const calls: unknown[] = [];
      sw.registration.showNotification = (...args: unknown[]) => {
        calls.push(args);
        return Promise.resolve();
      };
      sw.clients.matchAll = async () => [{ focused: true, url: `${sw.location.origin}/lines/bakerloo` }];

      const payload = { title: 'Bakerloo line', body: 'Severe delays', url: '/lines/bakerloo', tag: 'line-bakerloo' };
      const event = new sw.PushEvent('push', { data: JSON.stringify(payload) });
      let waited: Promise<unknown> | undefined;
      event.waitUntil = (p: Promise<unknown>) => {
        waited = p;
        return p;
      };
      sw.dispatchEvent(event);
      await waited;
      return calls;
    });

    expect(calls).toEqual([]);
  });

  test('notificationclick closes the notification and opens/focuses its url', async ({ page }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));
    const worker = await getServiceWorker(page);

    const result = await worker.evaluate(async () => {
      const sw = self as any;
      let openedUrl: string | undefined;
      sw.clients.openWindow = async (url: string) => {
        openedUrl = url;
        return null;
      };

      const fakeNotification = {
        closed: false,
        close() {
          this.closed = true;
        },
        data: { url: '/lines/victoria' },
      };
      const event: any = new Event('notificationclick');
      event.notification = fakeNotification;
      let waited: Promise<unknown> | undefined;
      event.waitUntil = (p: Promise<unknown>) => {
        waited = p;
        return p;
      };
      sw.dispatchEvent(event);
      await waited;
      return { closed: fakeNotification.closed, openedUrl };
    });

    expect(result).toEqual({ closed: true, openedUrl: '/lines/victoria' });
  });
});
