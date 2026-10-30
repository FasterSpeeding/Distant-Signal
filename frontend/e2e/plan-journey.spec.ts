import { test, expect, type Page } from '@playwright/test';
import AxeBuilder from '@axe-core/playwright';

// Plan a journey (`/plan`, `JourneyCreationFlow`), which DS-194 merged
// "New journey" (`/journeys/new`) into: which option it opens on, the old
// URLs' redirects, the ways in that have a page of their own, deep links,
// and the "How do you want to start this journey?" choice at phone widths.
//
// Mantine keeps a SegmentedControl label on one line and hides the
// control's overflow, so at 390px "Tube, bus or walk" ran past the
// control's edge and was cut off; below `xs` the options now stack
// (`.ds-entry-mode-control` in globals.css). A label can be clipped by its
// own box (an ellipsis) or by the control's, so both are checked.
//
// The plan request here is answered by the test (`/Trips/plan` is rate
// limited per client, and plan.spec.ts spends that budget on real plans).

const QUESTION = 'How do you want to start this journey?';

function entryModes(page: Page) {
  return page.getByRole('radiogroup', { name: QUESTION });
}

test.describe('Plan a journey', () => {
  test('opens on the planner, with every other way to start on the same page', async ({ page }) => {
    await page.goto('/plan');
    await expect(page).toHaveTitle(/^Plan a journey/);
    await expect(page.getByRole('heading', { level: 1, name: 'Plan a journey' })).toBeVisible();
    await expect(entryModes(page).getByRole('radio', { name: 'Plan a route for me' })).toBeChecked();
    await expect(page.getByRole('button', { name: 'Find routes' })).toBeVisible();
    const main = page.getByRole('main');
    await expect(main.getByRole('link', { name: 'Find a train' })).toHaveAttribute('href', '/trains');
    await expect(main.getByRole('link', { name: 'Add a ticket' })).toHaveAttribute('href', '/track/mine/add-ticket');
    await expect(main.getByRole('link', { name: 'Journey templates' })).toHaveAttribute('href', '/journeys/templates');
    await expect(main.getByText('New journey')).toHaveCount(0);
  });

  test('names /plan as its canonical URL, whatever the query', async ({ page }) => {
    await page.goto('/plan?mode=known&origin=EUS');
    await expect(page.locator('link[rel="canonical"]')).toHaveAttribute('href', /\/plan$/);
  });

  test('the ticket and template links open their pages', async ({ page }) => {
    await page.goto('/plan');
    await page.getByRole('main').getByRole('link', { name: 'Add a ticket' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Add a ticket' })).toBeVisible();
    await page.goto('/plan');
    await page.getByRole('main').getByRole('link', { name: 'Journey templates' }).click();
    await expect(page.getByRole('heading', { level: 1, name: 'Your journey templates' })).toBeVisible();
    await page.goto('/plan');
    await page.getByRole('main').getByRole('link', { name: 'Find a train' }).click();
    await expect(page).toHaveURL(/\/trains$/);
  });

  test.describe('/journeys/new, which it replaced', () => {
    test('redirects to /plan for good, in one hop, keeping the query', async ({ request }) => {
      for (const [path, expected] of [
        ['/journeys/new', '/plan'],
        ['/journeys/new?mode=known', '/plan?mode=known'],
        ['/journeys/new?mode=window', '/plan?mode=window'],
        ['/journeys/new?fromTflStop=940GZZLUKSX', '/plan?fromTflStop=940GZZLUKSX'],
      ]) {
        const response = await request.get(path!, { maxRedirects: 0 });
        expect(response.status(), path).toBe(308);
        const location = new URL(response.headers().location!, 'http://x');
        expect(`${location.pathname}${location.search}`, path).toBe(expected);
      }
    });

    test('opens the same option it did, under the new name', async ({ page }) => {
      await page.goto('/journeys/new?mode=window');
      await expect(page).toHaveURL(/\/plan\?mode=window$/);
      await expect(page.getByRole('heading', { level: 1, name: 'Plan a journey' })).toBeVisible();
      await expect(entryModes(page).getByRole('radio', { name: 'I know my route' })).toBeChecked();
      await expect(page.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
    });

    test('keeps a TfL stop, opening Tube, bus or walk from it', async ({ page }) => {
      await page.goto('/journeys/new?fromTflStop=940GZZLUKSX');
      await expect(page).toHaveURL(/\/plan\?fromTflStop=940GZZLUKSX$/);
      await expect(entryModes(page).getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
      await expect(page.getByRole('combobox', { name: 'From (optional)' })).toHaveValue("King's Cross St. Pancras");
    });

    test('keeps a planner search', async ({ page }) => {
      await page.clock.setFixedTime(new Date('2026-09-17T06:00:00Z'));
      await page.goto('/journeys/new?origin=ZEA&destination=ZEC&date=2026-09-17&arriveBy=09:00');
      await expect(page).toHaveURL(/\/plan\?origin=ZEA&destination=ZEC&date=2026-09-17&arriveBy=09(:|%3A)00$/);
      await expect(page.getByRole('radio', { name: 'Arrive by' })).toBeChecked();
    });
  });

  test.describe('?mode=', () => {
    test('opens on the planner for ?mode=plan', async ({ page }) => {
      await page.goto('/plan?mode=plan');
      await expect(entryModes(page).getByRole('radio', { name: 'Plan a route for me' })).toBeChecked();
      await expect(page.getByRole('button', { name: 'Find routes' })).toBeVisible();
    });

    test('opens on the train form for ?mode=known, ready to track a known train', async ({ page }) => {
      await page.goto('/plan?mode=known');
      await expect(entryModes(page).getByRole('radio', { name: 'I know my route' })).toBeChecked();
      await expect(page.getByRole('radio', { name: 'I know the train' })).toBeChecked();
      await expect(page.getByRole('button', { name: 'Track this train' })).toBeVisible();
    });

    test('honours ?mode=window, opening the train form on a time window search', async ({ page }) => {
      await page.goto('/plan?mode=window');
      await expect(entryModes(page).getByRole('radio', { name: 'I know my route' })).toBeChecked();
      await expect(page.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
    });

    test('opens on Tube, bus or walk for ?mode=untracked', async ({ page }) => {
      await page.goto('/plan?mode=untracked');
      await expect(entryModes(page).getByRole('radio', { name: 'Tube, bus or walk' })).toBeChecked();
      await expect(page.getByRole('button', { name: 'Start journey' })).toBeVisible();
    });

    test('switching option writes it into the address bar, so a shared link opens on it', async ({ page }) => {
      await page.goto('/plan');
      await page.getByText('I know my route', { exact: true }).click();
      await expect(page).toHaveURL(/\/plan\?mode=known$/);
      await page.reload();
      await expect(entryModes(page).getByRole('radio', { name: 'I know my route' })).toBeChecked();
      await page.getByText('Plan a route for me', { exact: true }).click();
      await expect(page).toHaveURL(/\/plan$/);
    });
  });

  // Waypoints, vias, avoids and max changes, restored from a shared link
  // and sent to the planner; the search goes back into the address bar.
  test('restores a shared search with every option, sends it, and keeps it shareable', async ({ page }) => {
    const sent: URLSearchParams[] = [];
    await page.route('**/api/Trips/plan?*', async (route) => {
      sent.push(new URL(route.request().url()).searchParams);
      await route.fulfill({
        json: {
          results: 'options',
          segments: [
            { originCrs: 'ZEA', destinationCrs: 'ZEB', cappedByMaxChanges: false, itineraries: [] },
            { originCrs: 'ZEB', destinationCrs: 'ZEC', cappedByMaxChanges: false, itineraries: [] },
          ],
        },
      });
    });
    const query =
      'origin=ZEA&destination=ZEC&date=2026-09-17&departAfter=08:00&results=options' +
      '&waypoints=ZEB&via=ZTA&avoid=ZTB&avoidStop=ZTE&avoidChange=MAN&maxChanges=1';
    // The date picker's first day is today: keep the query's date in range.
    await page.clock.setFixedTime(new Date('2026-09-17T06:00:00Z'));
    await page.goto(`/plan?${query}`);
    await expect(page.getByRole('radio', { name: 'Compare options' })).toBeChecked();
    await expect(page.getByRole('button', { name: /^Advanced options ?\(5 set\)$/ })).toBeVisible();
    await page.getByRole('button', { name: 'Find routes' }).click();

    await expect.poll(() => sent.length).toBe(1);
    const params = sent[0]!;
    expect(params.get('origin')).toBe('ZEA');
    expect(params.get('destination')).toBe('ZEC');
    expect(params.get('waypoints')).toBe('ZEB');
    expect(params.get('via')).toBe('ZTA');
    expect(params.get('avoid')).toBe('ZTB');
    expect(params.get('avoidStop')).toBe('ZTE');
    expect(params.get('avoidChange')).toBe('MAN');
    expect(params.get('maxChanges')).toBe('1');
    const url = new URL(page.url());
    expect(url.pathname).toBe('/plan');
    for (const name of ['waypoints', 'via', 'avoid', 'avoidStop', 'avoidChange', 'maxChanges']) {
      expect(url.searchParams.get(name), name).toBe(params.get(name));
    }
  });

  test('arrow keys move between the options', async ({ page }) => {
    await page.goto('/plan');
    const plan = entryModes(page).getByRole('radio', { name: 'Plan a route for me' });
    await plan.focus();
    await page.keyboard.press('ArrowDown');
    await expect(entryModes(page).getByRole('radio', { name: 'I know my route' })).toBeChecked();
    await expect(page.getByRole('button', { name: 'Track this train' })).toBeVisible();
  });

  for (const mode of ['plan', 'known', 'untracked']) {
    test(`has no axe violations on ?mode=${mode}`, async ({ page }) => {
      await page.goto(`/plan?mode=${mode}`);
      await expect(page.getByRole('heading', { level: 1, name: 'Plan a journey' })).toBeVisible();
      const results = await new AxeBuilder({ page }).analyze();
      expect(results.violations).toEqual([]);
    });
  }

  for (const width of [320, 360, 390, 430, 1366]) {
    test.describe(`at ${width}px`, () => {
      test.use({ viewport: { width, height: width < 600 ? 844 : 900 } });

      test('shows every option label in full', async ({ page }) => {
        await page.goto('/plan');
        const group = entryModes(page);
        await expect(group).toBeVisible();

        const box = await group.boundingBox();
        expect(box).not.toBeNull();
        const labels = group.locator('.mantine-SegmentedControl-label');
        await expect(labels).toHaveText(['Plan a route for me', 'I know my route', 'Tube, bus or walk']);
        for (const label of await labels.all()) {
          const labelBox = await label.boundingBox();
          expect(labelBox).not.toBeNull();
          expect(labelBox!.x, `"${await label.textContent()}" starts outside the control`).toBeGreaterThanOrEqual(
            box!.x,
          );
          expect(
            labelBox!.x + labelBox!.width,
            `"${await label.textContent()}" runs past the control's edge`,
          ).toBeLessThanOrEqual(box!.x + box!.width);
          const { scrollWidth, clientWidth, scrollHeight, clientHeight } = await label.evaluate((el) => ({
            scrollWidth: el.scrollWidth,
            clientWidth: el.clientWidth,
            scrollHeight: el.scrollHeight,
            clientHeight: el.clientHeight,
          }));
          expect(scrollWidth, `"${await label.textContent()}" is cut short`).toBeLessThanOrEqual(clientWidth);
          expect(scrollHeight, `"${await label.textContent()}" is cut short`).toBeLessThanOrEqual(clientHeight);
        }

        // The control itself fits the page, so nothing is clipped by a
        // sideways scroll either.
        expect(box!.x + box!.width).toBeLessThanOrEqual(width);
        expect(await page.evaluate(() => document.documentElement.scrollWidth)).toBeLessThanOrEqual(width);
      });
    });
  }
});
