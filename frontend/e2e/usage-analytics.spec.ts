import { test, expect, type Page, type Request } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// Opt-in usage counts (DS-190, frontend/lib/usageAnalytics.ts): off by
// default, so a visitor who never opted in sends nothing; one who turns the
// switch on sends a page view by route template to the app's own
// `/api/events`. Runs against the real stack like every spec here; the
// events reach the api's `POST /public/events`, but these tests only watch
// what the browser sends, never the api's answer.

const LEGAL_PAGES_RENDERED = process.env.LEGAL_PAGES_PREVIEW === 'true' || process.env.LEGAL_PAGES_PUBLISHED === 'true';

/** Where a logged-out visitor finds the switch: /cookies while the legal
 * pages render (CI's frontend-e2e job previews them), else Preferences,
 * which is always open. */
const TOGGLE_PAGE = LEGAL_PAGES_RENDERED ? '/cookies' : '/account/preferences';

const SWITCH_NAME = 'Help improve Distant Signal';

/** A line in the seed (as e2e/accessibility.spec.ts names it). */
const LINE_ID = process.env.E2E_REAL_LINE_ID ?? 'gwr-main-line';

function isEvent(request: Request): boolean {
  return new URL(request.url()).pathname === '/api/events';
}

/** Records every request to `/api/events` the page makes. */
function recordEvents(page: Page): Request[] {
  const events: Request[] = [];
  page.on('request', (request) => {
    if (isEvent(request)) events.push(request);
  });
  return events;
}

/** Keeps a copy of each beacon's body in the page: Playwright reports a
 * `sendBeacon` request but not its Blob body. Each new document starts a
 * new list. */
async function recordBeaconBodies(page: Page) {
  await page.addInitScript(() => {
    const bodies: string[] = [];
    (window as unknown as { __usageBeacons: string[] }).__usageBeacons = bodies;
    const original = navigator.sendBeacon.bind(navigator);
    navigator.sendBeacon = (url: string | URL, data?: BodyInit | null) => {
      if (data instanceof Blob) {
        void data.text().then((text) => bodies.push(text));
      } else if (typeof data === 'string') {
        bodies.push(data);
      }
      return original(url, data);
    };
  });
}

async function beaconBodies(page: Page): Promise<unknown[]> {
  const raw = await page.evaluate(() => (window as unknown as { __usageBeacons?: string[] }).__usageBeacons ?? []);
  return raw.map((text) => JSON.parse(text) as unknown);
}

/** The usage switch, once the nav's session has reached it (it stays
 * disabled until then on /cookies). */
async function usageSwitch(page: Page) {
  const toggle = page.getByRole('switch', { name: SWITCH_NAME });
  await expect(toggle).toBeVisible();
  await expect(toggle).toBeEnabled();
  return toggle;
}

test.describe('opt-in usage counts', () => {
  test('a visitor who has not opted in sends nothing', async ({ page }) => {
    const events = recordEvents(page);
    await page.goto('/lines', { waitUntil: 'networkidle' });
    await page.goto('/stations', { waitUntil: 'networkidle' });
    await page.goto(TOGGLE_PAGE, { waitUntil: 'networkidle' });
    await expect(await usageSwitch(page)).not.toBeChecked();
    // Client-side navigation too, not only full loads.
    await page.getByRole('link', { name: 'Lines', exact: true }).filter({ visible: true }).first().click();
    await page.waitForURL('**/lines');
    await page.waitForLoadState('networkidle');
    expect(events.map((request) => request.url())).toEqual([]);
  });

  test('a visitor who opts in sends a page view by route template', async ({ page }) => {
    await recordBeaconBodies(page);
    const events = recordEvents(page);
    await page.goto(TOGGLE_PAGE, { waitUntil: 'networkidle' });
    const toggle = await usageSwitch(page);
    await toggle.click();
    await expect(toggle).toBeChecked();
    expect(await page.evaluate(() => window.localStorage.getItem('ds-analytics-opt-in'))).toBe('true');

    await page.goto(`/lines/${LINE_ID}`, { waitUntil: 'networkidle' });
    await expect.poll(() => beaconBodies(page)).toContainEqual({ name: 'page_view', route_template: '/lines/[id]' });
    // Never the real line, a cookie or anything else about the visitor.
    expect(JSON.stringify(await beaconBodies(page))).not.toContain(LINE_ID);
    expect(events.some((request) => request.method() === 'POST')).toBe(true);

    // Turning it off stops any more counts.
    await page.goto(TOGGLE_PAGE, { waitUntil: 'networkidle' });
    await (await usageSwitch(page)).click();
    const before = events.length;
    await page.goto('/stations', { waitUntil: 'networkidle' });
    expect(events.length).toBe(before);
  });

  test('a Trains search counts its view and outcome, never the train', async ({ page }) => {
    // The seed's W62001 runs as 2Z42 on 2099-01-05 (e2e/trains.spec.ts).
    await recordBeaconBodies(page);
    await page.addInitScript(() => window.localStorage.setItem('ds-analytics-opt-in', 'true'));
    await page.goto('/trains?headcode=2Z42&date=2099-01-05', { waitUntil: 'networkidle' });
    await expect(page.getByRole('link', { name: /^10:00 to ZFC\b/ })).toBeVisible();
    await expect
      .poll(() => beaconBodies(page))
      .toEqual(
        expect.arrayContaining([
          { name: 'feature', route_template: '/trains', props: { feature: 'trains', step: 'headcode' } },
          { name: 'feature', route_template: '/trains', props: { feature: 'trains', step: 'results' } },
        ]),
      );

    await page.goto('/trains?headcode=9Z99&date=2099-01-05', { waitUntil: 'networkidle' });
    await expect(page.getByText('No train runs as 9Z99 on that day.')).toBeVisible();
    await expect
      .poll(() => beaconBodies(page))
      .toEqual(
        expect.arrayContaining([
          { name: 'feature', route_template: '/trains', props: { feature: 'trains', step: 'no_results' } },
          { name: 'empty_state', route_template: '/trains', props: { kind: 'trains_headcode_none' } },
        ]),
      );
    const sent = JSON.stringify(await beaconBodies(page));
    expect(sent).not.toContain('9Z99');
    expect(sent).not.toContain('2099');
  });

  test('a browser that sends Global Privacy Control sends nothing, even when opted in', async ({ page }) => {
    await page.addInitScript(() => {
      Object.defineProperty(Navigator.prototype, 'globalPrivacyControl', { get: () => true, configurable: true });
      window.localStorage.setItem('ds-analytics-opt-in', 'true');
    });
    const events = recordEvents(page);
    await page.goto(TOGGLE_PAGE, { waitUntil: 'networkidle' });
    const toggle = page.getByRole('switch', { name: SWITCH_NAME });
    await expect(toggle).toBeDisabled();
    await expect(toggle).not.toBeChecked();
    await expect(page.getByText('Your browser asks sites not to track you, so nothing is sent.')).toBeVisible();
    await page.goto('/lines', { waitUntil: 'networkidle' });
    expect(events).toEqual([]);
  });
});

test.describe('usage switch accessibility', () => {
  for (const colorScheme of ['light', 'dark'] as const) {
    test.describe(colorScheme, () => {
      test.use({ colorScheme });

      for (const path of [...(LEGAL_PAGES_RENDERED ? ['/cookies'] : []), '/account/preferences']) {
        test(`${path}: the switch off and on`, async ({ page }) => {
          await page.goto(path, { waitUntil: 'networkidle' });
          const toggle = await usageSwitch(page);
          for (const state of ['off', 'on'] as const) {
            if (state === 'on') {
              await toggle.click();
              await expect(toggle).toBeChecked();
            }
            const results = await new AxeBuilder({ page }).include('[data-analytics-opt-in]').analyze();
            expect(results.violations, `${state}: ${JSON.stringify(results.violations, null, 2)}`).toEqual([]);
          }
        });
      }

      test('the switch disabled by Global Privacy Control', async ({ page }) => {
        await page.addInitScript(() => {
          Object.defineProperty(Navigator.prototype, 'globalPrivacyControl', { get: () => true, configurable: true });
        });
        await page.goto('/account/preferences', { waitUntil: 'networkidle' });
        await expect(page.getByRole('switch', { name: SWITCH_NAME })).toBeDisabled();
        const results = await new AxeBuilder({ page }).include('[data-analytics-opt-in]').analyze();
        expect(results.violations, JSON.stringify(results.violations, null, 2)).toEqual([]);
      });
    });
  }
});
