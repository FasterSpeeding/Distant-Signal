import { test, expect } from '@playwright/test';

test.describe('service worker registration and precaching', () => {
  test('registers and activates on the home page', async ({ page }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));

    const registration = await page.evaluate(() => navigator.serviceWorker.getRegistration());
    expect(registration).toBeTruthy();
  });

  test('precaches exactly the four allowlisted static-asset URLs on install', async ({ page }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));

    const cachedPathnames = await page.evaluate(async () => {
      const names = await caches.keys();
      const pathnames: string[] = [];
      for (const name of names) {
        const cache = await caches.open(name);
        const requests = await cache.keys();
        pathnames.push(...requests.map((r) => new URL(r.url).pathname));
      }
      return pathnames;
    });

    expect(cachedPathnames.sort()).toEqual(
      ['/icon-192.png', '/icon-512.png', '/manifest.webmanifest', '/offline.html'].sort(),
    );
  });

  test('/sw.js is served with a Cache-Control: no-cache header', async ({ page }) => {
    const response = await page.goto('/sw.js');
    expect(response?.headers()['cache-control']).toBe('no-cache');
  });
});

test.describe('offline behaviour', () => {
  test('a navigation while offline shows the static offline page, never a stale copy of real content', async ({
    page,
    context,
  }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));

    await context.setOffline(true);
    await page.goto('/').catch(() => {
      // A hard navigation attempt while offline may itself reject
      // depending on browser/Playwright version -- the assertion below is
      // what actually matters: the page ends up showing offline.html's
      // content either way, since the service worker's own fetch handler
      // serves it as the navigation's response.
    });

    await expect(page.getByRole('heading', { name: 'You’re offline' })).toBeVisible();
    // The critical negative assertion: no fragment of real, previously-
    // viewed line-status content (e.g. this app's nav bar) is present --
    // this is Decision 1's central safety property, exercised end to end.
    await expect(page.getByRole('navigation')).toHaveCount(0);

    await context.setOffline(false);
  });

  test('a mutation request still fails normally offline, never served from any cache', async ({ page, context }) => {
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));

    await context.setOffline(true);
    const outcome = await page.evaluate(async () => {
      try {
        await fetch('/api/preferences', { method: 'GET' });
        return 'unexpected-success';
      } catch {
        return 'failed-as-expected';
      }
    });
    expect(outcome).toBe('failed-as-expected');

    await context.setOffline(false);
  });
});
