import { test, expect } from '@playwright/test';

// The compact search and filters bar (components/ListFilterBar.tsx) beyond
// /lines (see e2e/lines.spec.ts for that page): /incidents and /trains,
// which search the api live, the search-only bars on /operators and a tram
// network's stops, the issue list's filters on a line page, and the history
// pages, which replace the URL rather than push. Every page keeps its state
// in the URL, so each test reloads and checks it is still there.

const REAL_LINE_ID = process.env.E2E_REAL_LINE_ID ?? 'lner-ecml';
const REAL_STATION_CRS = process.env.E2E_REAL_STATION_CRS ?? 'KGX';
const PHONE = { width: 390, height: 844 };

test.describe('/incidents', () => {
  test('searches as you type, counts the matches and keeps it all in the URL', async ({ page }) => {
    await page.goto('/incidents?period=all');
    const search = page.getByRole('textbox', { name: 'Find an incident' });
    await expect(page.getByRole('link', { name: /Signalling problems between London Kings Cross/ })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Search' })).toHaveCount(0);

    // One request per settled search, not per keystroke, and never a server
    // re-render of the page (which refetched every line and operator).
    const searches: string[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname === '/api/incidents') searches.push(url.search);
      // A server render of the page itself; nav links' prefetches don't count.
      const headers = request.headers();
      if (url.pathname === '/incidents' && headers.rsc && !headers['next-router-prefetch']) searches.push('RSC');
    });
    await search.pressSequentially('signal', { delay: 30 });
    await expect(page.getByRole('status').filter({ hasText: '1 incident matches.' })).toHaveCount(1);
    await expect(page.getByRole('link', { name: /Planned engineering work/ })).toHaveCount(0);
    await expect(page).toHaveURL(/[?&]q=signal(&|$)/);
    expect(searches.filter((entry) => entry === 'RSC')).toHaveLength(0);
    expect(searches.filter((entry) => entry.includes('q=')).length).toBeLessThanOrEqual(2);

    await page.getByRole('button', { name: 'Filters' }).click();
    const dialog = page.getByRole('dialog', { name: 'Filters' });
    await dialog.getByText('Cleared', { exact: true }).click();
    await expect(dialog.getByRole('button', { name: 'Close' })).toBeVisible();
    await expect(dialog.getByText('No incidents match these filters.')).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
    await expect(page.getByRole('button', { name: 'Filters' })).toBeFocused();
    await expect(page).toHaveURL(/[?&]state=cleared(&|$)/);

    await page.reload();
    await expect(page.getByRole('textbox', { name: 'Find an incident' })).toHaveValue('signal');
    await expect(page.getByTestId('list-filter-count')).toHaveText('2');
    await expect(page.getByRole('status').filter({ hasText: 'No incidents match.' })).toHaveCount(1);

    await page.getByRole('button', { name: 'Show the last 30 days' }).click();
    await expect(page.getByRole('textbox', { name: 'Find an incident' })).toHaveValue('');
    await expect(page).toHaveURL(/\/incidents$/);
  });

  test('Back from an incident restores the search', async ({ page }) => {
    await page.goto('/incidents?period=all');
    await page.getByRole('textbox', { name: 'Find an incident' }).fill('engineering');
    await expect(page).toHaveURL(/[?&]q=engineering(&|$)/);
    await page.getByRole('link', { name: /Planned engineering work/ }).click();
    await expect(page).toHaveURL(/\/incidents\/FIXTURE-INCIDENT-ENDED/);
    await page.goBack();
    await expect(page.getByRole('textbox', { name: 'Find an incident' })).toHaveValue('engineering');
    await expect(page.getByRole('link', { name: /Planned engineering work/ })).toBeVisible();
  });

  // 2026-10-20: planned work whose only validity window is next week is
  // Upcoming, not Active, in the list, its Status filter and its page.
  test('places planned work that has not started under Upcoming, not Active', async ({ page }) => {
    const row = page.getByRole('link', { name: /Buses replace trains between Middlesbrough and Saltburn/ });
    await page.goto('/incidents?period=all&state=active');
    await expect(page.getByRole('link', { name: /Signalling problems between London Kings Cross/ })).toBeVisible();
    await expect(row).toHaveCount(0);

    await page.goto('/incidents?period=all&state=upcoming');
    await expect(row).toBeVisible();
    await expect(page.locator('[data-incident-state="upcoming"]')).toHaveCount(1);
    await expect(page.locator('[data-incident-state="active"]')).toHaveCount(0);

    await row.click();
    await expect(page).toHaveURL(/\/incidents\/FIXTURE-INCIDENT-UPCOMING/);
    await expect(page.locator('[data-incident-state="upcoming"]')).toBeVisible();
    await expect(page.getByText(/^Not in effect yet: starts /)).toBeVisible();
    await expect(page.getByRole('heading', { name: 'Currently affects' })).toHaveCount(0);
  });

  test('on a phone the filters are a bottom sheet', async ({ page }) => {
    await page.setViewportSize(PHONE);
    await page.goto('/incidents');
    await page.getByRole('button', { name: 'Filters' }).click();
    const dialog = page.getByRole('dialog', { name: 'Filters' });
    await expect(dialog).toBeVisible();
    const box = await dialog.boundingBox();
    expect(box!.width).toBeGreaterThan(PHONE.width - 2);
    await page.keyboard.press('Escape');
    await expect(dialog).toBeHidden();
  });
});

test.describe('/trains', () => {
  test('shows the full form until the first search, then the compact bar', async ({ page }) => {
    await page.goto('/trains');
    await expect(page.getByRole('combobox', { name: 'Starts at (optional)' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Filters' })).toHaveCount(0);

    await page.getByRole('combobox', { name: 'Station' }).fill(REAL_STATION_CRS);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Search' }).click();

    await expect(page.getByRole('button', { name: 'Filters' })).toBeVisible();
    await expect(page.getByRole('combobox', { name: 'Starts at (optional)' })).toHaveCount(0);
    await expect(page).toHaveURL(new RegExp(`[?&]station=${REAL_STATION_CRS}(&|$)`));
  });

  test('applies a filter live and keeps it through a reload', async ({ page }) => {
    await page.goto(`/trains?station=${REAL_STATION_CRS}`);
    const searches: string[] = [];
    page.on('request', (request) => {
      const url = new URL(request.url());
      if (url.pathname === '/api/trains/search') searches.push(url.search);
      // A server render of the page itself; nav links' prefetches don't count.
      const headers = request.headers();
      if (url.pathname === '/trains' && headers.rsc && !headers['next-router-prefetch']) searches.push('RSC');
    });
    await page.getByRole('button', { name: 'Filters' }).click();
    const dialog = page.getByRole('dialog', { name: 'Filters' });
    await dialog.getByRole('combobox', { name: 'Starts at (optional)' }).fill('pbo');
    await expect(page).toHaveURL(/[?&]origin=PBO(&|$)/);
    await expect.poll(() => searches.some((entry) => entry.includes('origin=PBO'))).toBe(true);
    expect(searches).not.toContain('RSC');
    await expect(page.getByTestId('list-filter-count')).toHaveText('1');

    await page.reload();
    await expect(page.getByRole('combobox', { name: 'Station' })).toHaveValue(REAL_STATION_CRS);
    await expect(page.getByTestId('list-filter-count')).toHaveText('1');
  });

  // The seed's ZFX trains on 2099-01-05 (e2e/fixtures/seed.sql): c2c at
  // 22:30 and at 00:30 after midnight, and one with no operator at 23:00.
  test('filters by operator, across midnight into the rest of the train day', async ({ page }) => {
    await page.goto('/trains?station=ZFX&date=2099-01-05&time=22:00&until=01:30&operator=CC');
    const evening = page.getByRole('list', { name: 'Trains' });
    await expect(evening.getByRole('link', { name: /^22:30 to / })).toBeVisible();
    const afterMidnight = page.getByRole('list', { name: 'After midnight, Tue 6 Jan' });
    await expect(afterMidnight.getByRole('link', { name: /^00:30 \(next day\) to / })).toBeVisible();
    await expect(page.getByRole('link', { name: /^23:00 to / })).toHaveCount(0);

    await page.getByRole('button', { name: 'Filters' }).click();
    const dialog = page.getByRole('dialog', { name: 'Filters' });
    // The picked operator's pill (its option in the closed list is hidden).
    await expect(dialog.getByText('CC · c2c').first()).toBeVisible();
    // 01:30 is before 22:00, so it can only be the next morning.
    await expect(dialog.getByRole('checkbox', { name: 'Next day, Tue 6 Jan' }).nth(1)).toBeChecked();

    // Without the operator, the 23:00 joins them.
    await dialog.getByRole('combobox', { name: 'Operator (optional)' }).press('Backspace');
    await expect(page).not.toHaveURL(/[?&]operator=/);
    await expect(page.getByRole('link', { name: /^23:00 to / })).toBeVisible();
  });
});

test.describe('search-only lists', () => {
  test('/operators filters its cards and keeps the search through a reload', async ({ page }) => {
    await page.goto('/operators');
    const cards = page.locator('main a[href^="/operators/"]');
    // The seed's one TOC, c2c (see e2e/fixtures/seed.sql).
    await expect(page.getByRole('link', { name: 'History for c2c' })).toBeVisible();
    const before = await cards.count();
    await page.getByRole('textbox', { name: 'Find an operator' }).fill('zzzz');
    await expect(page.getByRole('status').filter({ hasText: 'No operators match.' })).toHaveCount(1);
    await expect(cards).toHaveCount(0);
    await expect(page).toHaveURL(/[?&]q=zzzz(&|$)/);
    await expect(page).toHaveTitle('Operators: "zzzz" · Distant Signal');

    await page.reload();
    await expect(page.getByRole('textbox', { name: 'Find an operator' })).toHaveValue('zzzz');
    await page.getByRole('button', { name: 'Show all operators' }).click();
    await expect(cards).toHaveCount(before);
  });

  test("a tram network's stops have Find a stop", async ({ page }) => {
    await page.goto('/transit/metrolink');
    await page.getByRole('textbox', { name: 'Find a stop' }).fill('picc');
    // The A to Z only: a disruption may link a stop too.
    const stops = page.getByRole('list', { name: 'Stops' }).locator('a[href^="/transit/metrolink/stops/"]');
    await expect(stops).toHaveCount(1);
    await expect(stops.first()).toHaveText('Piccadilly');
    await expect(page).toHaveURL(/[?&]q=picc(&|$)/);
    await page.reload();
    await expect(stops).toHaveCount(1);
  });
});

test.describe('issue list', () => {
  test("keeps the line page's tab in the URL under its own key", async ({ page }) => {
    await page.goto(`/lines/${REAL_LINE_ID}?dir=up`);
    await page
      .getByText(/^Upcoming \(/)
      .first()
      .click();
    await expect(page).toHaveURL(/[?&]issueTab=upcoming(&|$)/);
    await expect(page).toHaveURL(/[?&]dir=up(&|$)/);
    await page.reload();
    await expect(page.getByRole('radio', { name: /^Upcoming/ }).first()).toBeChecked();
  });

  test("the line page's train links keep the issue filters", async ({ page }) => {
    // c2c: the seed gives it an empty train population, so "Trains on this
    // line" draws its links. It has nothing wrong, so its issue list has no
    // filters to click: the filter set after load is written to the URL the
    // way the list writes it (useUrlFilterSync).
    await page.goto('/lines/c2c?tflIssueTab=ended');
    // Retried: Next.js only follows `replaceState` once it has hydrated.
    const byRoute = page.getByRole('navigation', { name: 'Group trains' }).getByRole('link', { name: 'By route' });
    await expect(async () => {
      await page.evaluate(() => {
        const url = new URL(window.location.href);
        url.searchParams.set('issueSeverity', '6');
        window.history.replaceState(null, '', `${url.pathname}${url.search}${url.hash}`);
      });
      await expect(byRoute).toHaveAttribute('href', /[?&]issueSeverity=6(&|#)/, { timeout: 1000 });
    }).toPass();
    await byRoute.click();
    await expect(page).toHaveURL(/[?&]view=routes(&|#|$)/);
    await expect(page).toHaveURL(/[?&]tflIssueTab=ended(&|#|$)/);
    await expect(page).toHaveURL(/[?&]issueSeverity=6(&|#|$)/);

    await page.getByRole('link', { name: 'Later trains' }).filter({ visible: true }).click();
    await expect(page).toHaveURL(/[?&]at=\d\d%3A\d\d(&|#|$)/);
    await expect(page).toHaveURL(/[?&]view=routes(&|#|$)/);
    await expect(page).toHaveURL(/[?&]issueSeverity=6(&|#|$)/);

    // The station picker is a GET form: its hidden inputs carry them too.
    await page.getByText('Pick stations').click();
    const from = page.getByRole('combobox', { name: 'From' });
    await from.selectOption({ index: 1 });
    await page.getByRole('combobox', { name: 'To' }).selectOption({ index: 2 });
    await page.getByRole('button', { name: 'Show trains' }).click();
    await expect(page).toHaveURL(/[?&]from=[A-Z]{3}(&|#|$)/);
    await expect(page).toHaveURL(/[?&]tflIssueTab=ended(&|#|$)/);
    await expect(page).toHaveURL(/[?&]issueSeverity=6(&|#|$)/);
  });
});

test.describe('history pages', () => {
  test('changing the period replaces the URL, so Back leaves the page', async ({ page }) => {
    await page.goto(`/lines/${REAL_LINE_ID}`);
    await page.goto(`/lines/${REAL_LINE_ID}/history`);
    // 7 days is the default: pick 30, then back to 7.
    await page.getByText('30 days', { exact: true }).click();
    await expect(page).toHaveURL(/[?&]range=30d(&|$)/);
    await page.getByText('7 days', { exact: true }).click();
    await expect(page).toHaveURL(/[?&]range=7d(&|$)/);
    await page.goBack();
    await expect(page).toHaveURL(new RegExp(`/lines/${REAL_LINE_ID}$`));
  });
});
