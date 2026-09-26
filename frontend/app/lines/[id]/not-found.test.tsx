import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import LineNotFound from './not-found';
import * as api from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  // `getSessionOrLoggedOut`, not `getSession`: `not-found.tsx` calls the
  // former (it never writes its own `.catch()` around a raw `getSession()`
  // any more -- see `lib/api.ts`'s own doc comment on why every such
  // call site was centralized there). Overriding `getSession` alone
  // wouldn't reach it: `getSessionOrLoggedOut`'s internal `await
  // getSession()` call is a lexical reference to THIS module's own
  // real implementation, not a dynamic lookup through the exports object
  // this factory returns, so it would silently ignore a `getSession`
  // override and hit the real (unmocked) network call instead.
  return { ...actual, getSessionOrLoggedOut: vi.fn() };
});
// LoginLink calls usePathname()/useSearchParams() -- same workaround every
// other test rendering it uses.
vi.mock('next/navigation', () => ({
  usePathname: () => '/lines/some-id/edit',
  useSearchParams: () => new URLSearchParams(''),
}));

function session(authenticated: boolean) {
  return { authenticated, id: authenticated ? 'u1' : null, email: authenticated ? 'a@b.com' : null, name: null };
}

describe('LineNotFound', () => {
  // Task 3.4.10: "Back to your dashboard" assumed a home page an anonymous
  // visitor doesn't have -- neutral wording works for both.
  it('offers a neutral "Go to the home page" link, not "Back to your dashboard"', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(session(true));
    renderWithMantine(await LineNotFound());

    expect(screen.getByRole('link', { name: 'Go to the home page' })).toHaveAttribute('href', '/');
    expect(screen.queryByText('Back to your dashboard')).not.toBeInTheDocument();
  });

  it('offers a "Log in" link to an anonymous visitor', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(session(false));
    renderWithMantine(await LineNotFound());

    expect(screen.getByRole('link', { name: 'Log in' })).toBeInTheDocument();
  });

  it('does not offer a "Log in" link to a signed-in visitor', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(session(true));
    renderWithMantine(await LineNotFound());

    expect(screen.queryByRole('link', { name: 'Log in' })).not.toBeInTheDocument();
  });

  // Failing closed: an unreachable session check must not hide the login
  // link from a visitor who may well be genuinely logged out. The actual
  // fail-closed *and* logged-failure behaviour now lives inside
  // `getSessionOrLoggedOut()` itself (`lib/api.ts` -- see its own tests in
  // `lib/api.test.ts`), which never rejects; this only pins that
  // `LineNotFound` still renders the "Log in" link given the logged-out
  // `SessionInfo` that fallback resolves to.
  it('shows the "Log in" link when getSessionOrLoggedOut() resolves logged-out after a failed check', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue({ authenticated: false, id: null, email: null, name: null });
    renderWithMantine(await LineNotFound());

    expect(screen.getByRole('link', { name: 'Log in' })).toBeInTheDocument();
  });

  it('still links to "Browse all lines"', async () => {
    vi.mocked(api.getSessionOrLoggedOut).mockResolvedValue(session(true));
    renderWithMantine(await LineNotFound());

    expect(screen.getByRole('link', { name: 'Browse all lines' })).toHaveAttribute('href', '/lines');
  });
});
