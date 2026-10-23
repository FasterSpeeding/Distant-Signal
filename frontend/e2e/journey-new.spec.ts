import { test, expect, type Page } from '@playwright/test';

// /journeys/new (`JourneyCreationFlow`): which option it opens on, and the
// "How do you want to start this journey?" choice at phone widths. Mantine
// keeps a SegmentedControl label on one line and hides the control's
// overflow, so at 390px "Tube, bus or walk" ran past the control's edge
// and was cut off; below `xs` the options now stack
// (`.ds-entry-mode-control` in globals.css). A label can be clipped by its
// own box (an ellipsis) or by the control's, so both are checked.

const QUESTION = 'How do you want to start this journey?';

function entryModes(page: Page) {
  return page.getByRole('radiogroup', { name: QUESTION });
}

test.describe('/journeys/new', () => {
  test('opens on the planner', async ({ page }) => {
    await page.goto('/journeys/new');
    await expect(entryModes(page).getByRole('radio', { name: 'Plan a route for me' })).toBeChecked();
    await expect(page.getByRole('button', { name: 'Find routes' })).toBeVisible();
  });

  test('honours ?mode=window, opening the train form on a time window search', async ({ page }) => {
    await page.goto('/journeys/new?mode=window');
    await expect(entryModes(page).getByRole('radio', { name: 'I know my route' })).toBeChecked();
    await expect(page.getByRole('radio', { name: 'Search a time window' })).toBeChecked();
  });

  test('/plan\'s "Track it directly" opens on "I know my route"', async ({ page }) => {
    await page.goto('/plan');
    await page.getByRole('link', { name: 'Track it directly' }).click();
    await expect(page).toHaveURL(/\/journeys\/new\?mode=known$/);
    await expect(entryModes(page).getByRole('radio', { name: 'I know my route' })).toBeChecked();
    await expect(page.getByRole('button', { name: 'Track this train' })).toBeVisible();
  });

  test('arrow keys move between the options', async ({ page }) => {
    await page.goto('/journeys/new');
    const plan = entryModes(page).getByRole('radio', { name: 'Plan a route for me' });
    await plan.focus();
    await page.keyboard.press('ArrowDown');
    await expect(entryModes(page).getByRole('radio', { name: 'I know my route' })).toBeChecked();
    await expect(page.getByRole('button', { name: 'Track this train' })).toBeVisible();
  });

  for (const width of [320, 360, 390, 430, 1366]) {
    test.describe(`at ${width}px`, () => {
      test.use({ viewport: { width, height: width < 600 ? 844 : 900 } });

      test('shows every option label in full', async ({ page }) => {
        await page.goto('/journeys/new');
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
