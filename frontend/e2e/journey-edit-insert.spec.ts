import { test, expect } from '@playwright/test';

// Editing an untracked leg in place and adding a leg at the start of a
// journey (spec docs/superpowers/specs/2026-10-20-untracked-legs-and-tracked-train-states-design.md
// §2.3/§2.4, follow-up). The journey page renders on the server, so this runs
// against the seeded e2e stack, logged in as the seed's owner, on a journey
// it creates for itself (so the seeded journey the screenshots use is left
// alone).

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

test.describe('edit and insert journey legs', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to the seed session');

  test('edits a Tube leg in place and adds a walk before it', async ({ page, context, request }) => {
    const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
    await context.addCookies([
      {
        name: 'distant_signal_session',
        value: SESSION_COOKIE ?? '',
        domain: base.hostname,
        path: '/',
        expires: -1,
        httpOnly: true,
        secure: base.protocol === 'https:',
        sameSite: 'Lax',
      },
    ]);
    // Tomorrow, 09:05 to 09:20 UTC: well inside the horizon.
    const tomorrow = new Date(Date.now() + 24 * 3600 * 1000).toISOString().slice(0, 10);
    const created = await request.post('/api/Journeys', {
      headers: {
        cookie: `distant_signal_session=${SESSION_COOKIE}`,
        origin: base.origin,
        'content-type': 'application/json',
      },
      data: {
        customName: 'E2E edit and insert',
        leg: {
          mode: 'untracked',
          transitMode: 'tube',
          lineName: 'Victoria line',
          originName: 'Victoria',
          destinationName: 'Euston',
          plannedDeparture: `${tomorrow}T09:05:00Z`,
          plannedArrival: `${tomorrow}T09:20:00Z`,
        },
      },
    });
    expect(created.ok(), await created.text()).toBe(true);
    const { journeyId } = (await created.json()) as { journeyId: number };

    await page.goto(`/journeys/${journeyId}`);
    const main = page.locator('#main-content');
    await expect(main.getByText('Tube: Victoria line')).toBeVisible();

    // Edit in place: another line.
    await main.getByRole('button', { name: 'Edit leg' }).click();
    const edit = page.getByRole('dialog', { name: 'Edit this leg' });
    await edit.getByLabel('Line (optional)').fill('Northern line');
    await edit.getByRole('button', { name: 'Save leg' }).click();
    await expect(main.getByText('Tube: Northern line')).toBeVisible();

    // Add a walk at the start, ending where the Tube leg starts.
    await main.getByRole('button', { name: 'Add a leg at the start' }).click();
    const add = page.getByRole('dialog', { name: 'Add a leg at the start' });
    await expect(add.getByRole('combobox', { name: 'To (optional)' })).toHaveValue('Victoria');
    const londonTime = new Intl.DateTimeFormat('en-GB', {
      timeZone: 'Europe/London',
      hour: '2-digit',
      minute: '2-digit',
    }).format(new Date(`${tomorrow}T09:05:00Z`));
    await expect(add.getByRole('textbox', { name: 'Arrives', exact: true })).toHaveValue(londonTime);
    await add.getByRole('combobox', { name: 'How' }).click();
    await page.getByRole('option', { name: 'Walk' }).click();
    await add.getByLabel('From (optional)').fill('Pimlico');
    await add.getByRole('textbox', { name: 'Leaves', exact: true }).fill('07:30');
    await add.getByRole('button', { name: 'Add leg' }).click();

    await expect(main.getByText('Leg 1 of 2')).toBeVisible();
    const cards = main.locator('[data-untracked-leg]');
    await expect(cards.nth(0)).toContainText('Walk');
    await expect(cards.nth(0)).toContainText('Pimlico to Victoria');
    await expect(cards.nth(1)).toContainText('Tube: Northern line');

    await request.delete(`/api/Journeys/${journeyId}`, {
      headers: { cookie: `distant_signal_session=${SESSION_COOKIE}`, origin: base.origin },
    });
  });
});
