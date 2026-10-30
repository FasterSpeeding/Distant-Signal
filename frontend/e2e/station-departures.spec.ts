import { test, expect } from '@playwright/test';

// The station page shows no departure board of its own: its "Departures"
// link and the call to action where the board used to be both go to the
// train search, pre-filled with the station.

const REAL_STATION_CRS = process.env.E2E_REAL_STATION_CRS ?? 'KGX';
// The field shows the bare code, then "London Kings Cross (KGX)" once the
// search has looked the name up.
const STATION_FIELD_VALUE = new RegExp(`^(${REAL_STATION_CRS}|.+ \\(${REAL_STATION_CRS}\\))$`);

test.describe('station page departures', () => {
  test('"Departures" opens the train search with the station filled in', async ({ page }) => {
    await page.goto(`/stations/${REAL_STATION_CRS}`);
    await expect(page.getByRole('button', { name: 'Scheduled departures' })).toHaveCount(0);

    await page.getByRole('navigation', { name: 'On this page' }).getByRole('link', { name: 'Departures' }).click();

    await expect(page).toHaveURL(new RegExp(`/trains\\?station=${REAL_STATION_CRS}$`));
    await expect(page.getByRole('combobox', { name: 'Station' })).toHaveValue(STATION_FIELD_VALUE);
  });

  test('the call to action goes to the same search, from the keyboard too', async ({ page }) => {
    await page.goto(`/stations/${REAL_STATION_CRS}#departures`);
    const cta = page.getByRole('link', { name: /^See departures from / });
    await expect(cta).toHaveAttribute('href', `/trains?station=${REAL_STATION_CRS}`);

    await cta.focus();
    await page.keyboard.press('Enter');

    await expect(page).toHaveURL(new RegExp(`/trains\\?station=${REAL_STATION_CRS}$`));
    await expect(page.getByRole('combobox', { name: 'Station' })).toHaveValue(STATION_FIELD_VALUE);
  });
});
