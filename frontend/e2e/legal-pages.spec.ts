import { test, expect } from '@playwright/test';

test('/attribution renders every required data-source credit', async ({ page }) => {
  await page.goto('/attribution');
  await expect(page.getByRole('heading', { level: 1, name: 'Data sources and licences' })).toBeVisible();
  const main = page.locator('main');
  await expect(main.getByText('Powered by TfL Open Data')).toBeVisible();
  await expect(
    main.getByRole('link', {
      name: 'Contains Information of Network Rail Infrastructure Limited licensed under the following licence',
    }),
  ).toBeVisible();
  await expect(main.getByText(/^Source: RSP/)).toBeVisible();
  await expect(main.getByRole('link', { name: 'CC BY 4.0' })).toBeVisible();
  await expect(main.getByText(/Contains public sector information licensed under the/)).toBeVisible();
});

test('the footer links to /attribution', async ({ page }) => {
  await page.goto('/');
  const footerNav = page.getByRole('navigation', { name: 'Site information' });
  await footerNav.getByRole('link', { name: 'Data sources and licences' }).click();
  await expect(page).toHaveURL(/\/attribution$/);
});

const LEGAL_PAGES_RENDERED =
  process.env.LEGAL_PAGES_PREVIEW === 'true' || process.env.LEGAL_PAGES_PUBLISHED === 'true';

test.describe('draft legal pages', () => {
  for (const path of ['/privacy', '/terms', '/cookies', '/contact']) {
    test(`${path} ${LEGAL_PAGES_RENDERED ? 'renders' : '404s while unpublished'}`, async ({ page }) => {
      const response = await page.goto(path);
      if (LEGAL_PAGES_RENDERED) {
        expect(response?.status()).toBe(200);
        await expect(page.getByRole('heading', { level: 1 })).toBeVisible();
      } else {
        expect(response?.status()).toBe(404);
        await expect(page.getByRole('navigation', { name: 'Site information' }).getByRole('link', { name: 'Privacy' })).toHaveCount(0);
      }
    });
  }
});
