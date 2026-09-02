import { test, expect } from '@playwright/test';

// Like every other spec in this directory, this drives the REAL app
// through `playwright.config.ts`'s own `webServer` (a real `next dev`) or
// a real deployed target (`E2E_BASE_URL`) -- it does not stand up its own
// mock backend. `/chat`'s own server-side gate (`getChatbotAccess()`,
// `frontend/app/chat/page.tsx`) still needs a real, reachable `api`
// service with an authenticated, allowlisted session to ever mount
// `ChatPanel` at all -- exactly the same constraint every other page this
// suite drives already has (`app/page.tsx`'s own `getPreferences()` call,
// for one). What THIS spec adds on top of a working `api`/session is
// mocking the two calls that used to have no local equivalent to run
// against at all: Anthropic's own `api.anthropic.com` and
// `distant-signal-mcp`'s `/mcp` -- both real, external, per-user-keyed
// services this repo cannot stand up a fixture for. See the
// client-side-tokens design doc's Decision 7 for the honest framing: this
// is narrower coverage than the deleted `orchestrator/test/chat.test.ts`
// suite, not a like-for-like replacement.
test.describe('/chat, mocked network', () => {
  test.beforeEach(async ({ page }) => {
    // Seed localStorage with a fake Anthropic key + MCP tokens before any
    // app JS runs, via an init script -- avoids re-driving the full OAuth
    // redirect chain for every case below, which is Decision 6's UX
    // trade-off (Open questions/risks #4), not this spec's own concern.
    await page.addInitScript(() => {
      window.localStorage.setItem('ds-anthropic-api-key', 'sk-ant-e2e-test');
      window.localStorage.setItem(
        'ds-mcp-oauth:tokens',
        JSON.stringify({ access_token: 'e2e-test-token', token_type: 'Bearer' }),
      );
    });
  });

  test('renders a streamed text reply from a mocked Anthropic response', async ({ page }) => {
    await page.route('**/mcp', async (route) => {
      const body = JSON.parse(route.request().postData() ?? '{}');
      if (body.method === 'tools/list') {
        await route.fulfill({ json: { jsonrpc: '2.0', id: body.id, result: { tools: [] } } });
        return;
      }
      await route.continue();
    });
    await page.route('**/v1/messages*', async (route) => {
      await route.fulfill({
        status: 200,
        contentType: 'text/event-stream',
        body:
          'event: content_block_delta\ndata: {"type":"content_block_delta","delta":{"type":"text_delta","text":"Next train is at 10:15."}}\n\n' +
          'event: message_stop\ndata: {"type":"message_stop"}\n\n',
      });
    });

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('when is the next train');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/next train is at 10:15/i)).toBeVisible();
  });

  test('surfaces a distinct error when Anthropic rejects the key with a 401', async ({ page }) => {
    await page.route('**/mcp', (route) => route.continue());
    await page.route('**/v1/messages*', (route) =>
      route.fulfill({ status: 401, contentType: 'application/json', body: JSON.stringify({ error: { message: 'invalid x-api-key' } }) }),
    );

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('hi');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/anthropic api key was rejected/i)).toBeVisible();
  });

  test('surfaces a distinct "reconnect" error on a 401/403 from /mcp', async ({ page }) => {
    await page.route('**/mcp', (route) => route.fulfill({ status: 401, contentType: 'application/json', body: '{}' }));

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('hi');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/reconnect/i)).toBeVisible();
  });
});
