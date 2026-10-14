import { test, expect, type Page } from '@playwright/test';

// The /lines search and filters bar (components/ListFilterBar.tsx): one
// sticky row with "Find a line" and a "Filters" button, the filters in a
// popover from `sm` up and a bottom sheet below it, and the search,
// operator and status filters kept in the URL.

const PHONE = { width: 390, height: 844 };

async function scrollDown(page: Page, by: number) {
  await page.mouse.wheel(0, by);
  await expect.poll(() => page.evaluate(() => window.scrollY)).toBeGreaterThan(by / 2);
}

for (const [label, viewport] of [
  ['phone', PHONE],
  ['desktop', { width: 1366, height: 900 }],
] as const) {
  test.describe(`/lines search and filters, ${label}`, () => {
    test.use({ viewport });

    test('the bar is one row, and search stays on screen after scrolling', async ({ page }) => {
      await page.goto('/lines');
      const search = page.getByRole('textbox', { name: 'Find a line' });
      const filters = page.getByRole('button', { name: 'Filters' });
      const searchBox = await search.boundingBox();
      const filtersBox = await filters.boundingBox();
      expect(searchBox && filtersBox).toBeTruthy();
      // Same row: the two controls' vertical centres line up.
      expect(Math.abs(searchBox!.y + searchBox!.height / 2 - (filtersBox!.y + filtersBox!.height / 2))).toBeLessThan(2);
      // The whole bar costs well under a tenth of a phone screen.
      const barHeight = await page.locator('.listFilterBar').evaluate((el) => el.getBoundingClientRect().height);
      expect(barHeight).toBeLessThan(70);

      await scrollDown(page, 1500);
      await expect(search).toBeInViewport();
      await expect(page.locator('.listFilterBar')).toHaveAttribute('data-stuck', 'true');
    });

    test('filters open in a dialog, Escape closes it and returns focus', async ({ page }) => {
      await page.goto('/lines');
      const filters = page.getByRole('button', { name: 'Filters' });
      await expect(filters).toHaveAttribute('aria-expanded', 'false');
      await filters.click();
      const dialog = page.getByRole('dialog', { name: 'Filters' });
      await expect(dialog).toBeVisible();
      await expect(filters).toHaveAttribute('aria-expanded', 'true');
      await page.keyboard.press('Escape');
      await expect(dialog).toBeHidden();
      await expect(filters).toBeFocused();
    });

    test('search and filters survive a reload through the URL', async ({ page }) => {
      await page.goto('/lines');
      await page.getByRole('textbox', { name: 'Find a line' }).fill('main');
      await page.getByRole('button', { name: 'Filters' }).click();
      await page.getByRole('dialog', { name: 'Filters' }).getByText('All statuses', { exact: true }).waitFor();
      await page.getByRole('dialog', { name: 'Filters' }).getByText('Good service', { exact: true }).click();
      await expect(page).toHaveURL(/[?&]q=main(&|$)/);
      await expect(page).toHaveURL(/[?&]statusGroup=good(&|$)/);

      await page.reload();
      await expect(page.getByRole('textbox', { name: 'Find a line' })).toHaveValue('main');
      await expect(page.getByTestId('list-filter-count')).toHaveText('1');
    });
  });
}

for (const width of [320, 360, 390]) {
  test(`at ${width}px the table fits the screen with the Pin column visible`, async ({ page }) => {
    await page.setViewportSize({ width, height: 844 });
    await page.goto('/lines');
    const fit = await page.locator('table').evaluate((table) => ({
      table: table.getBoundingClientRect().width,
      wrapper: table.parentElement!.clientWidth,
      page: document.documentElement.scrollWidth - document.documentElement.clientWidth,
    }));
    expect(fit.table).toBeLessThanOrEqual(fit.wrapper + 1);
    expect(fit.page).toBeLessThanOrEqual(1);
    await expect(page.getByRole('columnheader', { name: 'Pin' })).toBeInViewport({ ratio: 1 });
    await expect(page.getByRole('row').nth(1).getByRole('button').last()).toBeInViewport({ ratio: 1 });
  });
}

test('the column headers stick under the search bar', async ({ page }) => {
  await page.setViewportSize(PHONE);
  await page.goto('/lines');
  await scrollDown(page, 1500);
  const { barBottom, headTop } = await page.evaluate(() => ({
    barBottom: document.querySelector('.listFilterBar')!.getBoundingClientRect().bottom,
    headTop: document.querySelector('thead')!.getBoundingClientRect().top,
  }));
  expect(Math.abs(headTop - barBottom)).toBeLessThan(1);
  await expect(page.getByRole('columnheader', { name: /Name/ })).toBeInViewport();
});

test('the tab title follows the search', async ({ page }) => {
  await page.goto('/lines');
  await page.getByRole('textbox', { name: 'Find a line' }).fill('main');
  await expect(page).toHaveTitle('Lines: "main" · Distant Signal');
  await page.getByRole('button', { name: 'Clear search' }).click();
  await expect(page).toHaveTitle('Lines · Distant Signal');
});

test('on a phone the filters are a bottom sheet, not a popover', async ({ page }) => {
  await page.setViewportSize(PHONE);
  await page.goto('/lines');
  await page.getByRole('button', { name: 'Filters' }).click();
  const dialog = page.getByRole('dialog', { name: 'Filters' });
  await expect(dialog).toBeVisible();
  const box = await dialog.boundingBox();
  expect(box).toBeTruthy();
  // Full width and anchored to the bottom of the screen.
  expect(box!.width).toBeGreaterThan(PHONE.width - 2);
  expect(Math.round(box!.y + box!.height)).toBeGreaterThanOrEqual(PHONE.height - 1);
});
