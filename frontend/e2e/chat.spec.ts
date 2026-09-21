import { test, expect, type Page } from '@playwright/test';

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
//
// That "authenticated, allowlisted session" this file's own comment above
// requires was never actually established here (unlike every other
// logged-in spec in this directory) -- discovered running this suite for
// real in CI (.github/workflows/ci.yml's `frontend-e2e` job): without a
// session cookie, `getChatbotAccess()` returns 'unauthenticated' and
// `/chat` renders its "Sign in to ask about..." prompt instead of
// `ChatPanel`, so every test here timed out waiting for the (never
// rendered) message input. `sessionState`/`SESSION_COOKIE` below are
// copied from e2e/accessibility.spec.ts's own (see that file's copy for
// the full rationale on cookie flags), matching nav.spec.ts's own existing
// duplication of the same helper rather than introducing a new shared
// module for one function three files now use.
const SESSION_COOKIE = process.env.E2E_SESSION_COOKIE;

function sessionState(value: string) {
  const base = new URL(process.env.E2E_BASE_URL ?? 'http://localhost:3000');
  return {
    cookies: [
      {
        name: 'distant_signal_session',
        value,
        domain: base.hostname,
        path: '/',
        expires: -1,
        httpOnly: true,
        secure: base.protocol === 'https:',
        sameSite: 'Lax' as const,
      },
    ],
    origins: [],
  };
}

/** The MCP handshake `@modelcontextprotocol/sdk`'s `Client.connect()` runs
 * BEFORE `listTools()` ever fires: a JSON-RPC `initialize` call, then an
 * `notifications/initialized` notification (see `client/index.js`'s own
 * `connect()`). None of the three tests below used to mock either one --
 * only `tools/list` -- so `route.continue()`'s fallback for everything else
 * tried to reach the real (nonexistent) `NEXT_PUBLIC_RAILMCP_PUBLIC_URL`
 * this spec's own CI job points at a placeholder `.invalid` host, and every
 * case failed the same way regardless of what it was actually testing:
 * `Error: Failed to fetch` from the browser's own `fetch`, never reaching
 * the assertion each test cared about. Discovered running this suite for
 * real in CI (.github/workflows/ci.yml's `frontend-e2e` job).
 *
 * Registered in `beforeEach`, so it is the LAST-priority handler Playwright
 * checks for `**\/mcp` (routes run most-recently-registered first); each
 * test's own `page.route('**\/mcp', ...)` still wins for the method it
 * cares about and can shadow this entirely (the "reconnect" test below does
 * exactly that, on purpose, to make `initialize` itself fail). */
async function mockMcpHandshake(page: Page) {
  await page.route('**/mcp', async (route) => {
    const body = JSON.parse(route.request().postData() ?? '{}');
    if (body.method === 'initialize') {
      await route.fulfill({
        json: {
          jsonrpc: '2.0',
          id: body.id,
          result: {
            // Echo the client's own requested version back rather than
            // hardcoding one of `SUPPORTED_PROTOCOL_VERSIONS` -- keeps this
            // fixture correct across an SDK version bump without edits.
            protocolVersion: body.params?.protocolVersion ?? '2025-06-18',
            capabilities: {},
            serverInfo: { name: 'distant-signal-mcp-fixture', version: '0.0.0' },
          },
        },
      });
      return;
    }
    if (body.method === 'notifications/initialized') {
      // A notification, not a request -- no `result`/`error` body, per
      // MCP's Streamable HTTP transport spec.
      await route.fulfill({ status: 202, body: '' });
      return;
    }
    if (body.method === 'tools/list') {
      await route.fulfill({ json: { jsonrpc: '2.0', id: body.id, result: { tools: [] } } });
      return;
    }
    await route.fallback();
  });
}

test.describe('/chat, mocked network', () => {
  test.skip(!SESSION_COOKIE, 'set E2E_SESSION_COOKIE to a raw distant_signal_session value for a chatbot-allowlisted user');
  test.use({ storageState: SESSION_COOKIE ? sessionState(SESSION_COOKIE) : undefined });

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
    await mockMcpHandshake(page);
  });

  test('renders a streamed text reply from a mocked Anthropic response', async ({ page }) => {
    await page.route('**/v1/messages*', async (route) => {
      // `@anthropic-ai/sdk`'s `BetaMessageStream` (lib/BetaMessageStream.ts
      // `#accumulateMessage`) enforces real event ordering: `message_start`
      // (carrying a full `message` object) must come first, then a
      // `content_block_start` for the index a later `content_block_delta`
      // targets, then `content_block_stop`/`message_delta`/`message_stop`
      // to close it out -- a bare `content_block_delta` first (this test's
      // previous body) throws `Unexpected event order, got
      // content_block_delta before "message_start"` before the mocked text
      // ever reaches the page.
      const messageStart = {
        type: 'message_start',
        message: {
          id: 'msg_e2e_test',
          type: 'message',
          role: 'assistant',
          model: 'claude-e2e-test',
          content: [],
          stop_reason: null,
          stop_sequence: null,
          usage: { input_tokens: 1, output_tokens: 0 },
        },
      };
      const events = [
        messageStart,
        { type: 'content_block_start', index: 0, content_block: { type: 'text', text: '' } },
        {
          type: 'content_block_delta',
          index: 0,
          delta: { type: 'text_delta', text: 'Next train is at 10:15.' },
        },
        { type: 'content_block_stop', index: 0 },
        {
          type: 'message_delta',
          delta: { stop_reason: 'end_turn', stop_sequence: null },
          usage: { output_tokens: 6 },
        },
        { type: 'message_stop' },
      ];
      await route.fulfill({
        status: 200,
        contentType: 'text/event-stream',
        body: events.map((event) => `event: ${event.type}\ndata: ${JSON.stringify(event)}\n\n`).join(''),
      });
    });

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('when is the next train');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/next train is at 10:15/i)).toBeVisible();
  });

  test('surfaces a distinct error when Anthropic rejects the key with a 401', async ({ page }) => {
    // No test-specific `**/mcp` route needed here -- `mockMcpHandshake`
    // (beforeEach) already gives the MCP side a normal, successful
    // handshake + empty tool list, exactly like the happy-path test above;
    // only the Anthropic response should fail in this one.
    await page.route('**/v1/messages*', (route) =>
      route.fulfill({ status: 401, contentType: 'application/json', body: JSON.stringify({ error: { message: 'invalid x-api-key' } }) }),
    );

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('hi');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/anthropic api key was rejected/i)).toBeVisible();
  });

  test('surfaces a distinct "reconnect" error on a 401/403 from /mcp', async ({ page }) => {
    // Registered AFTER `mockMcpHandshake` (beforeEach), so this shadows it
    // entirely for `**/mcp` (Playwright checks the most-recently-registered
    // matching route first) -- `initialize` itself gets the 401 this test
    // is about, rather than the successful handshake every other case gets.
    await page.route('**/mcp', (route) => route.fulfill({ status: 401, contentType: 'application/json', body: '{}' }));
    // On that 401, the SDK's `StreamableHTTPClientTransport` automatically
    // attempts OAuth reauth (`client/auth.js`'s `auth()`), which starts by
    // fetching RFC 9728 protected-resource metadata from a `.well-known`
    // URL on the same origin -- a real request `**/mcp` above does not
    // match. Left unmocked, that fetch fails at the network layer
    // (`Failed to fetch`, since `NEXT_PUBLIC_RAILMCP_PUBLIC_URL` is a
    // placeholder host in CI) instead of the clean, message-bearing HTTP
    // error this test actually wants: `discoverOAuthProtectedResourceMetadata`
    // (client/auth.js) throws `HTTP 401 trying to load well-known OAuth
    // protected resource metadata` for any non-404 response, which
    // `classifyChatError` (ChatPanel.tsx) matches on the literal "401" and
    // maps to the same 'mcp-reconnect' state this test asserts on.
    // Left unmocked, `discoverOAuthProtectedResourceMetadata`'s 401 above
    // doesn't stop the flow: with no cached/discovered authorization-server
    // metadata, `client/auth.js`'s `registerClient` falls back to a default
    // Dynamic Client Registration endpoint (`/register` on the same
    // origin) -- a real request neither `**/mcp` nor `**/.well-known/**`
    // matches, which then fails at the network layer exactly like the
    // unmocked `initialize` call originally did. `parseErrorResponse`
    // (client/auth.js) falls back to `HTTP ${status}: Invalid OAuth error
    // response...` for any non-`{error, error_description}` 401 body,
    // which still carries the literal "401" `classifyChatError` matches
    // on.
    await page.route('**/.well-known/**', (route) =>
      route.fulfill({ status: 401, contentType: 'application/json', body: '{}' }),
    );
    await page.route('**/register', (route) =>
      route.fulfill({ status: 401, contentType: 'application/json', body: '{}' }),
    );

    await page.goto('/chat');
    await page.getByPlaceholder(/ask about/i).fill('hi');
    await page.getByRole('button', { name: /send/i }).click();
    await expect(page.getByText(/reconnect/i)).toBeVisible();
  });
});
