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
// The seed's unresolved pin (no timetable, no report), its planned train
// (W77777 in 2099) and its journey with an untracked walk before that train
// (frontend/e2e/fixtures/seed.sql). Ids by insert order on a fresh database.
const TRACKING_ID = process.env.E2E_TRACKING_ID ?? '1';
const PLANNED_TRACKING_ID = process.env.E2E_PLANNED_TRACKING_ID ?? '3';
const JOURNEY_ID = process.env.E2E_JOURNEY_ID ?? '1';
// The seed's group, with trains and journeys shared into it by both users.
const GROUP_ID = process.env.E2E_GROUP_ID ?? 'fixture-group-1';

/** The browser's "now": midday on the seed's service date, in UK summer
 * time. Only the client's clock is fixed; the server's isn't (see VOLATILE). */
const FIXED_NOW = new Date('2026-09-17T11:00:00Z');

const PAGES: [name: string, path: string][] = [
  ['home', '/'],
  // Logged in: the seed's 2099 journey under "Upcoming journeys" at the top,
  // its pins, and its trains tracked on their own under "Tracked trains".
  // No journey of the seed's is under way at any server time, so there is
  // no "Current journey" here (home-journeys.spec.ts covers it).
  ['home-logged-in', '/'],
  ['status', '/status'],
  ['lines', '/lines'],
  ['line', `/lines/${REAL_LINE_ID}`],
  ['stations', '/stations'],
  ['station', `/stations/${REAL_STATION_CRS}`],
  ['trains', '/trains'],
  // After a search: the compact bar, with Station in its search slot.
  ['trains-searched', `/trains?station=${REAL_STATION_CRS}`],
  ['train', `/train/${TRAIN_UID}/${TRAIN_DATE}`],
  ['incidents', '/incidents'],
  // The plan form in each timing mode (2026-10-19), restored from the URL;
  // the seed's /plan timetable codes. The time and date fields are masked.
  ['plan', '/plan?origin=ZEA&destination=ZEC&date=2026-09-17&departAfter=08:00'],
  ['plan-arrive-by', '/plan?origin=ZEA&destination=ZEC&date=2026-09-17&arriveBy=09:00'],
  ['account', '/account'],
  // Tracked-train states and an untracked leg (spec
  // 2026-10-20-untracked-legs-and-tracked-train-states-design.md): a train
  // not in the timetable, a planned train, and a journey with a walk.
  ['train-no-timetable', `/train/by-id/${TRACKING_ID}`],
  ['train-planned', `/train/by-id/${PLANNED_TRACKING_ID}`],
  ['journey', `/journeys/${JOURNEY_ID}`],
  // Open to everyone: logged out it is the appearance settings and one log
  // in prompt, logged in the account sections too.
  ['preferences', '/account/preferences'],
  ['preferences-logged-in', '/account/preferences'],
  // The Track page's lists, logged in: the seed's tracked trains and its
  // planned three-leg journey, named end to end (docs/style-guide.md,
  // "Journey names").
  ['track', '/track/mine'],
  // A group's shared trains and journeys, logged in as its owner: a live
  // train, a planned one, a timetabled bus, a journey whose train is
  // planned and a walk-and-Tube journey (2026-10-20: the owner's own
  // badge words on group rows).
  ['group', `/groups/${GROUP_ID}`],
  // Tram pages (the seed's metrolink network; API_TRANSIT_NETWORKS in the
  // frontend-e2e job). Dated, so the board and timetable don't follow the
  // server's clock.
  ['transit-network', '/transit/metrolink'],
  ['transit-line', '/transit/metrolink/lines/blue'],
  ['transit-stop', '/transit/metrolink/stops/9400ZZMAPIC?date=2026-09-17'],
  ['transit-timetable', '/transit/metrolink/lines/blue/timetable?date=2026-09-17&direction=outbound&from=07:00'],
];

/** The pages shot with the seed's session cookie (E2E_SESSION_COOKIE). */
const LOGGED_IN_PAGES: ReadonlySet<string> = new Set([
  'home-logged-in',
  'account',
  'preferences-logged-in',
  'track',
  'group',
  'train-no-timetable',
  'train-planned',
  'journey',
]);

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
 *    while the browser's today is FIXED_NOW;
 *  - a group's invite link expiry ("Expires 27 Oct 2026."): the seed
 *    dates it a week after the seeding. */
function volatile(page: Page) {
  return [
    page.getByText(/\b(updated|last updated|fetched)\b.*\d/i),
    page.getByText(/\b\d+ (min|hr|days?) ago\b|\bjust now\b/),
    page.getByText(/\b\d{1,2} [A-Z][a-z]{2,3} \d{4}, \d{1,2}:\d{2}\b/),
    page.getByText(/^(Search (a|a different|a later|an earlier) day\b|Only .* can be searched\.)/),
    // The group's invite link expiry: the seed sets it a week after seeding.
    page.getByText(/^Expires \d{1,2} [A-Z][a-z]{2,3} \d{4}\.$/),
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
          if (LOGGED_IN_PAGES.has(name)) {
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

      // Preferences with a pride flag saved: the page and nav stripes, the
      // gradient site name, the sparkles around the wordmark and the
      // picker's preview. Sparkles float, so this relies on the reduced
      // motion above holding them still and visible.
      test('preferences-pride', async ({ page }) => {
        await page.addInitScript(() => {
          localStorage.setItem('pride-mode', 'trans');
        });
        await page.clock.setFixedTime(FIXED_NOW);
        await page.goto('/account/preferences', { waitUntil: 'networkidle' });
        await page.addStyleTag({ content: NO_MOTION_CSS });
        await page.evaluate(() => document.fonts.ready);
        await expect(page.locator('main .prideSparkle')).toHaveCount(3);

        await expect(page).toHaveScreenshot(`preferences-pride-${scheme}-${width}.png`, {
          fullPage: true,
          animations: 'disabled',
          caret: 'hide',
          mask: volatile(page),
        });
      });

      // The /lines search and filters bar (components/ListFilterBar.tsx) in
      // the two states the full-page shot above can't show: stuck to the
      // top after scrolling, and with the filters open (a popover from
      // `sm` up, a bottom sheet below). Viewport shots, not full page: both
      // states are about what is on screen.
      for (const [state, open] of [
        ['lines-scrolled', false],
        ['lines-filters-open', true],
      ] as const) {
        test(state, async ({ page }) => {
          await page.clock.setFixedTime(FIXED_NOW);
          await page.goto('/lines', { waitUntil: 'networkidle' });
          await page.addStyleTag({ content: NO_MOTION_CSS });
          await page.evaluate(() => document.fonts.ready);
          if (open) {
            await page.getByRole('button', { name: 'Filters' }).click();
            await expect(page.getByRole('dialog', { name: 'Filters' })).toBeVisible();
          } else {
            await page.evaluate(() => window.scrollTo(0, 1200));
            await expect(page.locator('.listFilterBar')).toHaveAttribute('data-stuck', 'true');
          }

          await expect(page).toHaveScreenshot(`${state}-${scheme}-${width}.png`, {
            animations: 'disabled',
            caret: 'hide',
            mask: volatile(page),
          });
        });
      }

      // The /incidents filters panel open (a popover from `sm` up, a bottom
      // sheet below): the tallest panel, with its chip rows and the priority
      // range. Viewport shot, as for /lines.
      test('incidents-filters-open', async ({ page }) => {
        await page.clock.setFixedTime(FIXED_NOW);
        await page.goto('/incidents', { waitUntil: 'networkidle' });
        await page.addStyleTag({ content: NO_MOTION_CSS });
        await page.evaluate(() => document.fonts.ready);
        await page.getByRole('button', { name: 'Filters' }).click();
        await expect(page.getByRole('dialog', { name: 'Filters' })).toBeVisible();

        // No mask: the only clock-dependent text, the incidents' timestamps,
        // is under the panel at both widths, and masks are painted on top.
        await expect(page).toHaveScreenshot(`incidents-filters-open-${scheme}-${width}.png`, {
          animations: 'disabled',
          caret: 'hide',
        });
      });

      // The /stations "Use my location" results
      // (app/stations/NearbyStationList.tsx), which only exist after the
      // click. The location is mocked and so is the lookup, with five
      // stations (the seed has two with coordinates) so a long name wraps
      // at 390px. A shot of the results block alone.
      test('stations-nearby', async ({ page, context }) => {
        await context.grantPermissions(['geolocation']);
        await context.setGeolocation({ latitude: 51.53, longitude: -0.13 });
        await page.route('**/api/stations/nearby?*', (route) =>
          route.fulfill({
            json: [
              { code: 'EUS', name: 'London Euston', distanceKm: 0.32 },
              { code: 'KGX', name: 'London Kings Cross', distanceKm: 0.44 },
              { code: 'STP', name: 'London St Pancras International', distanceKm: 0.51 },
              { code: 'ZFD', name: 'Farringdon', distanceKm: 1.62 },
              { code: 'MYB', name: 'London Marylebone', distanceKm: 12.04 },
            ],
          }),
        );
        await page.clock.setFixedTime(FIXED_NOW);
        await page.goto('/stations', { waitUntil: 'networkidle' });
        await page.addStyleTag({ content: NO_MOTION_CSS });
        await page.evaluate(() => document.fonts.ready);
        await page.getByRole('button', { name: 'Use my location' }).click();
        const list = page.getByRole('list', { name: 'Nearest stations' });
        await expect(list.getByRole('link')).toHaveCount(5);
        // The mouse rests on nothing, so no row is in its hover state.
        await page.mouse.move(0, 0);

        await expect(list.locator('xpath=..')).toHaveScreenshot(`stations-nearby-${scheme}-${width}.png`, {
          animations: 'disabled',
          caret: 'hide',
        });
      });
    });
  }
}
