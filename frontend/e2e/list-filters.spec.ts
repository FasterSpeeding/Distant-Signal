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
    await expect(page.getByRole('combobox', { name: 'Departing from (optional)' })).toBeVisible();
    await expect(page.getByRole('button', { name: 'Filters' })).toHaveCount(0);

    await page.getByRole('combobox', { name: 'Station' }).fill(REAL_STATION_CRS);
    await page.keyboard.press('Escape');
    await page.getByRole('button', { name: 'Search' }).click();

    await expect(page.getByRole('button', { name: 'Filters' })).toBeVisible();
    await expect(page.getByRole('combobox', { name: 'Departing from (optional)' })).toHaveCount(0);
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
    await dialog.getByRole('combobox', { name: 'Departing from (optional)' }).fill('pbo');
    await expect(page).toHaveURL(/[?&]origin=PBO(&|$)/);
    await expect.poll(() => searches.some((entry) => entry.includes('origin=PBO'))).toBe(true);
    expect(searches).not.toContain('RSC');
    await expect(page.getByTestId('list-filter-count')).toHaveText('1');

    await page.reload();
    await expect(page.getByRole('combobox', { name: 'Station' })).toHaveValue(REAL_STATION_CRS);
    await expect(page.getByTestId('list-filter-count')).toHaveText('1');
  });
});

test.describe('search-only lists', () => {
  test('/operators filters its cards and keeps the search through a reload', async ({ page }) => {
    await page.goto('/operators');
    const cards = page.locator('main a[href^="/operators/"]');
    const before = await cards.count();
    // The e2e seed has no `tocs` rows (adding them would rename operators
    // on most of the visual baselines), so there are no cards and no bar
    // to search; SearchableList.test.tsx covers the search itself.
    test.skip(before === 0, 'the e2e seed has no operators');
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
    const stops = page.locator('a[href^="/transit/metrolink/stops/"]');
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
