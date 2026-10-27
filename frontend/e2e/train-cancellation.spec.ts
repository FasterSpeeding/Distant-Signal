import { test, expect } from '@playwright/test';

// A train cut short (DS-181), against the seeded e2e stack
// (frontend/e2e/fixtures/seed.sql): W81181 on 2026-12-03 left King's Cross,
// reached York and was cancelled en route there with a reason, so the
// public train page says where it terminates and why, and strikes the
// calls it no longer makes through, with "Cancelled" in words beside them.

const PAGE = '/train/W81181/2026-12-03';

test('a train cut short says where it terminates, why, and which calls are cancelled', async ({ page }) => {
  await page.goto(PAGE);

  const notice = page.getByRole('alert').filter({ hasText: 'Terminates at York' });
  await expect(notice).toBeVisible();
  await expect(notice).toContainText('Cancelled between York and Edinburgh.');
  await expect(notice).toContainText('Reason: points failure (including no fault found).');
  await expect(notice).toContainText('Reason recorded by Network Rail');

  const table = page.getByRole('table', { name: 'Journey timeline' });
  const edinburgh = table.getByRole('row').filter({ hasText: 'Edinburgh' });
  await expect(edinburgh).toContainText('Cancelled');
  await expect(edinburgh.locator('[data-cancelled="true"]')).toHaveCSS('text-decoration-line', 'line-through');
  // The calls it still made are not struck through.
  const york = table.getByRole('row').filter({ hasText: 'York' });
  await expect(york).not.toContainText('Cancelled');
  await expect(york.locator('[data-cancelled]')).toHaveCount(0);

  // The page's description says the same, for a shared link.
  await expect(page.locator('meta[name="description"]')).toHaveAttribute(
    'content',
    'Terminates at York: points failure (including no fault found).',
  );
});
