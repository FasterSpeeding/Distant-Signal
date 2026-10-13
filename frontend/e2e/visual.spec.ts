import { test, expect, type Page } from '@playwright/test';

// Visual regression screenshots of the main pages, at phone (390px) and
// desktop (1366px) widths, in the light and dark schemes.
//
// Runs only in the `visual` Playwright project (playwright.config.ts), and
// only inside the pinned Playwright container started by
// `scripts/visual-tests.py`, so the fonts and the renderer match between a
// developer's machine and CI. Run outside that container, every screenshot
// differs by font hinting alone. See docs/style-guide.md, "Visual
// regression", for how to run it and how to update the baselines.
//
// Deterministic by construction:
//  - the data is frontend/e2e/fixtures/seed.sql, the same seed the
//    frontend-e2e job uses;
//  - the browser's clock is fixed (FIXED_NOW), and every element whose
//    text depends on the server's wall clock is masked (VOLATILE below);
//  - animations and transitions are disabled, the caret is hidden and
//    reduced motion is emulated;
//  - the colour scheme is forced through `colorScheme` emulation. A fresh
//    context has no saved Mantine scheme in localStorage, so the app's
//    `defaultColorScheme="auto"` follows the emulated media query;
//  - service workers are blocked (playwright.config.ts) so a cached shell
//    can't stand in for the page.

const REAL_LINE_ID = process.env.E2E_REAL_LINE_ID ?? 'lner-ecml';
const REAL_STATION_CRS = process.env.E2E_REAL_STATION_CRS ?? 'KGX';
const TRAIN_UID = process.env.E2E_TRAIN_UID ?? 'W12345';
const TRAIN_DATE = process.env.E2E_TRAIN_DATE ?? '2026-09-17';
const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

/** The browser's "now": midday on the seed's service date, in UK summer
 * time. Only the client's clock is fixed; the server's isn't (see VOLATILE). */
const FIXED_NOW = new Date('2026-09-17T11:00:00Z');

const PAGES: [name: string, path: string][] = [
  ['home', '/'],
  ['status', '/status'],
  ['lines', '/lines'],
  ['line', `/lines/${REAL_LINE_ID}`],
  ['stations', '/stations'],
  ['station', `/stations/${REAL_STATION_CRS}`],
  ['trains', '/trains'],
  ['train', `/train/${TRAIN_UID}/${TRAIN_DATE}`],
  ['incidents', '/incidents'],
  ['account', '/account'],
];

const WIDTHS = [390, 1366] as const;
const SCHEMES = ['light', 'dark'] as const;

/** Regions whose content depends on the server's wall clock or the time the
 * stack was seeded, not on the code under test. Each is painted over with a
 * solid box in the screenshot, so a layout change around it still shows.
 *  - relative and absolute "Updated ..." / "... ago" freshness text
 *    (LastUpdated, DataFreshnessInfo): rendered from fetch times;
 *  - date-and-time stamps ("12 Oct 2026, 17:32"): the seed dates incident
 *    and status rows relative to NOW(), so they move with every seeding.
 *    A railway time on its own ("10:00") or a date on its own is fixture
 *    data and stays visible;
 *  - date and time inputs: the trains and incidents search forms default
 *    to the server's today;
 *  - the trains date field's hint ("Search a day from 6 Oct 2026 to 20 Oct
 *    2026."): the searchable range is the server's, around its own today,
 *    while the browser's today is FIXED_NOW. */
function volatile(page: Page) {
  return [
    page.getByText(/\b(updated|last updated|fetched)\b.*\d/i),
    page.getByText(/\b\d+ (min|hr|days?) ago\b|\bjust now\b/),
    page.getByText(/\b\d{1,2} [A-Z][a-z]{2,3} \d{4}, \d{1,2}:\d{2}\b/),
    page.getByText(/^(Search (a|a different|a later|an earlier) day\b|Only .* can be searched\.)/),
    page.locator('.mantine-DatePickerInput-input, .mantine-TimeInput-input, input[type="date"], input[type="time"]'),
  ];
}

/** Disable Mantine's transitions and every CSS animation, on top of the
 * screenshot's own `animations: 'disabled'`, which only finishes animations
 * already running at capture time. */
const NO_MOTION_CSS = `*, *::before, *::after {
  transition: none !important;
  animation: none !important;
  caret-color: transparent !important;
}`;

function sessionCookie(value: string) {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  return {
    name: 'distant_signal_session',
    value,
    domain: base.hostname,
    path: '/',
    expires: -1,
    httpOnly: true,
    secure: base.protocol === 'https:',
    sameSite: 'Lax' as const,
  };
}

for (const scheme of SCHEMES) {
  for (const width of WIDTHS) {
    test.describe(`${scheme} ${width}px`, () => {
      test.use({
        colorScheme: scheme,
        contextOptions: { reducedMotion: 'reduce' },
        viewport: { width, height: width < 600 ? 844 : 900 },
      });

      for (const [name, path] of PAGES) {
        test(name, async ({ page, context }) => {
          if (name === 'account') {
            test.skip(!SESSION_COOKIE, 'E2E_SESSION_COOKIE is unset');
            await context.addCookies([sessionCookie(SESSION_COOKIE!)]);
          }
          await page.clock.setFixedTime(FIXED_NOW);
          await page.goto(path, { waitUntil: 'networkidle' });
          await page.addStyleTag({ content: NO_MOTION_CSS });
          await page.evaluate(() => document.fonts.ready);
          await expect(page.locator('h1').first()).toBeVisible();

          await expect(page).toHaveScreenshot(`${name}-${scheme}-${width}.png`, {
            fullPage: true,
            animations: 'disabled',
            caret: 'hide',
            mask: volatile(page),
          });
        });
      }
    });
  }
}
