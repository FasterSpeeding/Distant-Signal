import { test, expect, type Page } from '@playwright/test';
import { openNavDrawer } from './navDrawer';

// Repeater Signal L7: pages are served with a per-request nonce CSP
// (`script-src 'self' 'nonce-…' 'strict-dynamic'`, built by proxy.ts and
// lib/csp.ts) instead of `'unsafe-inline'`. A script Next or Mantine emits
// without the nonce would now be blocked silently, with the page looking
// fine in the server HTML and broken after hydration. So this drives real
// pages and fails on any CSP violation, through the flows that run the most
// client script: first load and hydration, the phone nav drawer and a
// client-side navigation out of it, AutoRefresh's `router.refresh()`, and
// service worker registration.
//
// Like every other spec here, this drives the real app (`webServer`, or a
// deployed `E2E_BASE_URL`).

/** Records every CSP violation the page reports. The init script is
 * injected by Playwright itself, so the page's own CSP does not apply to it. */
async function watchCsp(page: Page): Promise<() => Promise<string[]>> {
  const consoleViolations: string[] = [];
  page.on('console', (msg) => {
    const text = msg.text();
    // See the eval note in the init script below.
    if (text.includes('unsafe-eval')) return;
    if (/Content Security Policy|Refused to (execute|load|apply|connect)/i.test(text)) {
      consoleViolations.push(text);
    }
  });
  await page.addInitScript(() => {
    const w = window as unknown as { __cspViolations: string[] };
    w.__cspViolations = [];
    document.addEventListener('securitypolicyviolation', (e) => {
      // zod v4 (pulled in by @modelcontextprotocol/sdk on the chat pages)
      // probes for eval support with `try { Function('') } catch {}` and
      // falls back to its non-JIT path. The probe raises a violation but
      // is expected: no policy this app has shipped allowed eval.
      if (e.blockedURI === 'eval') return;
      w.__cspViolations.push(`${e.effectiveDirective} blocked ${e.blockedURI || '(inline)'}`);
    });
  });
  return async () => {
    const events = await page.evaluate(() => (window as unknown as { __cspViolations: string[] }).__cspViolations);
    return [...events, ...consoleViolations];
  };
}

/** Waits for React to hydrate: the nav's burger/links only respond once it has. */
async function waitForHydration(page: Page) {
  await page.waitForFunction(() => {
    const el = document.querySelector('nav[aria-label="Main"]');
    return !!el && Object.keys(el).some((k) => k.startsWith('__react'));
  });
}

const ROUTES = ['/', '/lines', '/stations', '/incidents', '/trains', '/track', '/chat', '/chat/callback', '/lines/new'];

test.describe('Content-Security-Policy', () => {
  for (const path of ROUTES) {
    test(`${path} loads and hydrates with no CSP violations`, async ({ page }) => {
      const violations = await watchCsp(page);
      const response = await page.goto(path);
      const csp = response?.headers()['content-security-policy'] ?? '';
      expect(csp).toMatch(/script-src 'self' 'nonce-[A-Za-z0-9+/=]+' 'strict-dynamic'/);
      expect(csp.split('; ').find((d) => d.startsWith('script-src'))).not.toContain('unsafe-inline');
      await waitForHydration(page);
      // Mantine's ColorSchemeScript ran (it carries the nonce).
      await expect(page.locator('html')).toHaveAttribute('data-mantine-color-scheme', /light|dark/);
      expect(await violations()).toEqual([]);
    });
  }

  test('each page load gets a fresh nonce', async ({ page }) => {
    const nonce = async () =>
      (await page.goto('/stations'))?.headers()['content-security-policy']!.match(/'nonce-([^']+)'/)?.[1];
    const [a, b] = [await nonce(), await nonce()];
    expect(a).toBeTruthy();
    expect(a).not.toBe(b);
  });

  test('an injected inline event handler does not run', async ({ page }) => {
    const violations = await watchCsp(page);
    await page.goto('/stations');
    await waitForHydration(page);
    // The classic sanitizer-bypass payload: markup with an inline handler.
    await page.evaluate(() => {
      const div = document.createElement('div');
      div.innerHTML = '<img src="/does-not-exist.png" onerror="window.__pwned = true">';
      document.body.appendChild(div);
    });
    await expect.poll(violations).toContainEqual(expect.stringContaining('script-src'));
    expect(await page.evaluate(() => (window as unknown as { __pwned?: boolean }).__pwned)).toBeUndefined();
  });

  test('the phone nav drawer opens and navigates with no CSP violations', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 900 });
    const violations = await watchCsp(page);
    await page.goto('/');
    const drawer = await openNavDrawer(page);
    await drawer.getByRole('link', { name: 'Stations' }).click();
    await expect(page).toHaveURL(/\/stations$/);
    await expect(drawer).toBeHidden();
    expect(await violations()).toEqual([]);
  });

  test("AutoRefresh's router.refresh() still works with no CSP violations", async ({ page }) => {
    const violations = await watchCsp(page);
    await page.goto('/stations');
    await waitForHydration(page);
    // AutoRefresh refreshes immediately when a tab becomes visible again;
    // fake a hide/show rather than waiting out the 30s interval.
    const refresh = page.waitForResponse(
      (res) => new URL(res.url()).pathname === '/stations' && res.request().headers().rsc === '1',
    );
    await page.evaluate(() => {
      const setVisibility = (state: DocumentVisibilityState) => {
        Object.defineProperty(document, 'visibilityState', { value: state, configurable: true });
        document.dispatchEvent(new Event('visibilitychange'));
      };
      setVisibility('hidden');
      setTimeout(() => setVisibility('visible'), 50);
    });
    const res = await refresh;
    expect(res.status()).toBe(200);
    expect(await violations()).toEqual([]);
  });

  test('the service worker registers under the page CSP', async ({ page }) => {
    const violations = await watchCsp(page);
    await page.goto('/');
    await page.waitForFunction(() => navigator.serviceWorker.ready.then(() => true));
    expect(await page.evaluate(() => navigator.serviceWorker.getRegistration())).toBeTruthy();
    expect(await violations()).toEqual([]);
  });
});
