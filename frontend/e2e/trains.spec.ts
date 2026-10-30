import { test, expect, type Page } from '@playwright/test';

// The Trains section (`/trains`, DS-192): the timetable, with no tracking
// or journey steps in the way. Its views run against the seed's W62001
// (e2e/fixtures/seed.sql): headcode 2Z42 on 2099-01-05, ZFA 10:00, ZFB
// (arrives 10:28, leaves 10:30), terminating at ZFC at 11:00.
const DATE = '2099-01-05';

/** Nothing on the page scrolls sideways. */
async function hasHorizontalOverflow(page: Page): Promise<boolean> {
  return page.evaluate(() => document.documentElement.scrollWidth > document.documentElement.clientWidth + 1);
}

test.describe('/trains views', () => {
  test("a station's departures open the train's page, where tracking is a secondary action", async ({ page }) => {
    await page.goto(`/trains?station=ZFA&date=${DATE}`);
    await expect(page.getByRole('heading', { level: 1, name: 'Trains' })).toBeVisible();
    await expect(page.getByRole('radio', { name: 'Departures' })).toBeChecked();
    const row = page.getByRole('link', { name: /^10:00 to ZFC\b/ });
    await expect(row).toBeVisible();
    // A timetable row is its link and nothing else: no tracking button.
    await expect(page.getByRole('button', { name: /^Track this/ })).toHaveCount(0);

    await row.click();
    await expect(page).toHaveURL(new RegExp(`/train/W62001/${DATE}$`));
    const track = page.getByRole('button', { name: 'Track this train' });
    await expect(track).toBeVisible();
    // Light, not the filled primary: the page is the train's timetable first.
    await expect(track).toHaveAttribute('data-variant', 'light');
    await expect(page.getByRole('link', { name: 'Look up another train' })).toHaveAttribute('href', '/trains');
  });

  test("a station's arrivals, terminating trains included, in the URL as board=arrivals", async ({ page }) => {
    await page.goto(`/trains?station=ZFB&date=${DATE}&board=arrivals`);
    const calling = page.getByRole('link', { name: /^10:28 from ZFA\b/ });
    await expect(calling).toBeVisible();
    await expect(page.locator('li').filter({ has: calling })).toContainText('To ZFC');

    await page.getByRole('combobox', { name: 'Station' }).fill('ZFC');
    await page.keyboard.press('Escape');
    const terminating = page.getByRole('link', { name: /^11:00 from ZFA\b/ });
    await expect(terminating).toBeVisible();
    await expect(page.locator('li').filter({ has: terminating })).toContainText('Terminates here');
    await expect(page).toHaveURL(/[?&]station=ZFC(&|$)/);
    await expect(page).toHaveURL(/[?&]board=arrivals(&|$)/);

    // The radio itself is visually hidden behind its label.
    await page.getByText('Departures', { exact: true }).click();
    await expect(page).not.toHaveURL(/[?&]board=/);
  });

  test('the trains between two stations, with the arrival at the second', async ({ page }) => {
    await page.goto(`/trains?from=ZFA&to=ZFC&date=${DATE}`);
    await expect(page.getByRole('radio', { name: 'Between stations' })).toBeChecked();
    const row = page.getByRole('link', { name: /^10:00 to ZFC\b/ });
    await expect(row).toBeVisible();
    await expect(page.locator('li').filter({ has: row })).toContainText(/arr.*11:00/);
  });

  test('picking a view, then a station pair, by keyboard alone', async ({ page }) => {
    await page.goto('/trains');
    const station = page.getByRole('radio', { name: 'Station', exact: true });
    await station.focus();
    await page.keyboard.press('ArrowRight');
    await expect(page.getByRole('radio', { name: 'Between stations' })).toBeChecked();
    await page.getByRole('combobox', { name: 'From', exact: true }).fill('ZFA');
    await page.keyboard.press('Tab');
    await page.getByRole('combobox', { name: 'To', exact: true }).fill('ZFC');
    await page.keyboard.press('Tab');
    await page.getByRole('button', { name: 'Search' }).press('Enter');
    await expect(page).toHaveURL(/[?&]from=ZFA&to=ZFC(&|$)/);
  });

  test('a headcode lists its trains; a UID opens its train', async ({ page }) => {
    await page.goto(`/trains?headcode=2z42&date=${DATE}`);
    await expect(page.getByRole('radio', { name: 'Headcode or UID' })).toBeChecked();
    await expect(page.getByText(/^1 train runs as 2Z42\./)).toBeVisible();
    await expect(page.getByRole('link', { name: /^10:00 to ZFC\b/ })).toHaveAttribute('href', `/train/W62001/${DATE}`);

    await page.goto(`/trains?uid=w62001&date=${DATE}`);
    await expect(page).toHaveURL(new RegExp(`/train/W62001/${DATE}$`));
  });

  test('fits a phone: the view switch stacks and nothing scrolls sideways', async ({ page }) => {
    await page.setViewportSize({ width: 390, height: 844 });
    await page.goto(`/trains?station=ZFA&date=${DATE}`);
    await expect(page.getByRole('link', { name: /^10:00 to ZFC\b/ })).toBeVisible();
    for (const view of ['Station', 'Between stations', 'Headcode or UID']) {
      await expect(page.getByText(view, { exact: true }).first()).toBeVisible();
    }
    expect(await hasHorizontalOverflow(page)).toBe(false);
  });
});

// The advanced options (DS-210) run against the seed's W62003 on the same
// date: ZGA 10:00, a call at ZGB, through ZGJ without stopping, to ZGC.
test.describe('/trains advanced options', () => {
  const NONE = 'No scheduled trains match those filters right now.';

  test('on the departures board, "Avoid completely" drops a train that runs through, and the URL keeps it', async ({
    page,
  }) => {
    await page.goto(`/trains?station=ZGA&date=${DATE}`);
    await expect(page.getByRole('link', { name: /^10:00 to ZGC\b/ })).toBeVisible();
    await page.getByRole('button', { name: /^Filters/ }).click();
    await page.getByRole('button', { name: /^Advanced options/ }).click();
    await expect(page.getByText("These look at the train's route after ZGA.")).toBeVisible();
    const avoid = page.getByRole('combobox', { name: 'Avoid completely' });
    await avoid.fill('ZGJ');
    await avoid.press('Enter');
    await expect(page.getByText(NONE)).toBeVisible();
    await expect(page).toHaveURL(/[?&]avoid=ZGJ(&|$)/);
  });

  test('on the arrivals board, "Don\'t stop at" looks at the calls before the station', async ({ page }) => {
    await page.goto(`/trains?station=ZGC&board=arrivals&date=${DATE}&avoid_stop=ZGJ`);
    // Running through ZGJ is not stopping there.
    await expect(page.getByRole('link', { name: /^10:50 from ZGA\b/ })).toBeVisible();
    await page.goto(`/trains?station=ZGC&board=arrivals&date=${DATE}&avoid_stop=ZGB`);
    await expect(page.getByText(NONE)).toBeVisible();
  });

  // ZGA is where W62003 starts; ZGB is a stop on its way.
  test('"Previously calls at" counts where the train starts and "Previously stops at" does not', async ({ page }) => {
    await page.goto(`/trains?station=ZGB&date=${DATE}`);
    await expect(page.getByRole('link', { name: /^10:22 to ZGC\b/ })).toBeVisible();
    await page.getByRole('button', { name: /^Filters/ }).click();
    await page.getByRole('button', { name: /^Advanced options/ }).click();
    await expect(
      page.getByText('To find a particular train, these look at where it has been before ZGB.'),
    ).toBeVisible();
    const calls = page.getByRole('combobox', { name: 'Previously calls at (in order)' });
    await calls.fill('ZGA');
    await calls.press('Enter');
    await expect(page).toHaveURL(/[?&]previously_calls_at=ZGA(&|$)/);
    await expect(page.getByRole('link', { name: /^10:22 to ZGC\b/ })).toBeVisible();
    await page.goto(`/trains?station=ZGB&date=${DATE}&previously_stops_at=ZGA`);
    await expect(page.getByText(NONE)).toBeVisible();
    // On the arrivals board, a stop on the way counts.
    await page.goto(`/trains?station=ZGC&board=arrivals&date=${DATE}&previously_stops_at=ZGB`);
    await expect(page.getByRole('link', { name: /^10:50 from ZGA\b/ })).toBeVisible();
  });

  test('an old "Starts at" link opens as "Previously calls at"', async ({ page }) => {
    await page.goto(`/trains?station=ZGB&date=${DATE}&origin=ZGA`);
    await expect(page.getByRole('link', { name: /^10:22 to ZGC\b/ })).toBeVisible();
    await page.getByRole('button', { name: /^Filters/ }).click();
    await expect(page.getByRole('button', { name: /^Advanced options ?\(1 set\)$/ })).toBeVisible();
    await expect(page.getByText(/^Previously calls at (ZGA|Fixture Lane)$/)).toBeVisible();
  });

  test('between two stations, "Pass through" counts a pass and "Call at" does not', async ({ page }) => {
    await page.goto(`/trains?from=ZGA&to=ZGC&date=${DATE}&via=ZGJ`);
    await expect(page.getByRole('link', { name: /^10:00 to ZGC\b/ })).toBeVisible();
    await page.goto(`/trains?from=ZGA&to=ZGC&date=${DATE}&calls_at=ZGJ`);
    await expect(page.getByText(NONE)).toBeVisible();
  });
});

test.describe('/trains links from before the section', () => {
  test('an old-shape search redirects permanently to the same search, keeping every param', async ({ request }) => {
    const response = await request.get('/trains?station=ZFX&date=2099-01-05&from=22:00&to=01:30&operator=CC', {
      maxRedirects: 0,
    });
    expect(response.status()).toBe(308);
    const location = new URL(response.headers().location!, 'http://localhost');
    expect(location.pathname).toBe('/trains');
    expect(location.search).toBe('?station=ZFX&date=2099-01-05&time=22%3A00&until=01%3A30&operator=CC');
  });

  test('station and stops_at redirect to the between-stations view', async ({ request }) => {
    const response = await request.get('/trains?station=ZFA&stops_at=ZFC&ticketId=7', { maxRedirects: 0 });
    expect(response.status()).toBe(308);
    const location = new URL(response.headers().location!, 'http://localhost');
    expect(Object.fromEntries(location.searchParams)).toEqual({ ticketId: '7', from: 'ZFA', to: 'ZFC' });
  });

  test('an old link lands on the same results', async ({ page }) => {
    await page.goto('/trains?station=ZFA&stops_at=ZFC&date=2099-01-05');
    await expect(page).toHaveURL(/\/trains\?date=2099-01-05&from=ZFA&to=ZFC$/);
    await expect(page.getByRole('link', { name: /^10:00 to ZFC\b/ })).toBeVisible();
  });

  test('a current search is not redirected', async ({ request }) => {
    const response = await request.get('/trains?station=ZFA&time=09:00', { maxRedirects: 0 });
    expect(response.status()).toBe(200);
  });
});
