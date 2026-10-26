import { test, expect, type BrowserContext } from '@playwright/test';

// The journey template editor (components/EditJourneyTemplateForm.tsx):
// each leg's From and To fields are the site's standard station search, so
// a station can be found by its full name, and the template is still saved
// with CRS codes.
//
// Drives the real app and api: the template page is server-rendered from
// `GET /JourneyTemplates/{id}`, which only answers its owner, so the spec
// logs in as the seed's session user (see e2e/nav.spec.ts's comment on
// `E2E_SESSION_COOKIE`), creates its own template through the api, and
// deletes it again afterwards.

const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

async function logIn(context: BrowserContext): Promise<void> {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  await context.addCookies([
    {
      name: 'distant_signal_session',
      value: SESSION_COOKIE!,
      domain: base.hostname,
      path: '/',
      httpOnly: true,
      secure: false,
      sameSite: 'Lax',
    },
  ]);
}

test.describe('edit a journey template', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to a raw distant_signal_session value');

  test('finds a station by its full name and saves its code', async ({ page, context }) => {
    await logIn(context);
    const created = await page.request.post('/api/JourneyTemplates', {
      data: {
        mode: 'manual',
        customName: 'Station search e2e',
        legs: [{ originCrs: 'KGX', destinationCrs: 'EDB' }],
      },
    });
    expect(created.ok()).toBe(true);
    const { templateId } = (await created.json()) as { templateId: number };

    try {
      await page.goto(`/journeys/templates/${templateId}`);
      const to = page.getByRole('combobox', { name: 'To station' });
      // The seed has no name for EDB, so it shows bare.
      await expect(to).toHaveValue('EDB');

      await to.fill('London Euston');
      await expect(page.getByRole('option', { name: 'EUS · London Euston' })).toBeVisible();
      // Leaving the field picks the station the typed name matches exactly.
      await to.press('Tab');
      await expect(to).toHaveValue('London Euston (EUS)');
      await expect(page.getByText('Must be a 3-letter station code')).toHaveCount(0);

      // Waited on directly so a rejected save fails here with its status,
      // not later as a missing "Saved.".
      const put = page.waitForResponse(
        (response) =>
          response.request().method() === 'PUT' && response.url().endsWith(`/api/JourneyTemplates/${templateId}`),
      );
      await page.getByRole('button', { name: 'Save changes' }).click();
      const putResponse = await put;
      // A 204 has no body to read, so the body is only fetched for a failure.
      if (!putResponse.ok()) {
        throw new Error(`PUT answered ${putResponse.status()}: ${await putResponse.text()}`);
      }
      await expect(page.getByText('Saved.')).toBeVisible();

      const saved = await page.request.get(`/api/JourneyTemplates/${templateId}`);
      expect(saved.ok()).toBe(true);
      expect(await saved.json()).toMatchObject({ legs: [{ originCrs: 'KGX', destinationCrs: 'EUS' }] });
    } finally {
      await page.request.delete(`/api/JourneyTemplates/${templateId}`);
    }
  });
});
