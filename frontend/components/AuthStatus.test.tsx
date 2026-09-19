import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { fireEvent, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AuthStatus } from './AuthStatus';
import type { SessionInfo } from '@/lib/types';

// `LoginLink` calls usePathname()/useSearchParams() and `AccountMenu`
// (via useLogout) calls useRouter(), all of which throw "invariant
// expected app router to be mounted" outside a real Next.js App Router
// tree (as in these unit tests) — same stub PinToggle.test.tsx uses.
const refresh = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh }),
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

const loggedOut: SessionInfo = { authenticated: false, id: null, email: null, name: null };

/** The account control's accessible name is where the display name lives
 * now that it is no longer a run of visible text in the bar (see
 * AuthStatus.tsx / AccountMenu.tsx for why it moved). Every "what label
 * does this session resolve to" case below therefore asserts on the
 * button's name rather than on `getByText`. */
function accountMenuName() {
  const buttons = screen.getAllByRole('button');
  const target = buttons.find((button) => button.getAttribute('aria-label')?.startsWith('Account menu for '));
  return target?.getAttribute('aria-label')?.replace('Account menu for ', '');
}

describe('AuthStatus', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refresh.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('shows a log in link when logged out', () => {
    renderWithMantine(<AuthStatus session={loggedOut} />);
    const link = screen.getByRole('link', { name: 'Log in' });
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2F');
  });

  // Review §2.16 "auth controls are inconsistently sized" (a 16px "Log in"
  // beside a ~12px "Log out"): the nav's "Log in" now renders at 14px
  // (`sm`), the size the chrome's other text-link-styled controls converge
  // on -- see TextLink.tsx's own `size` doc comment for the full reasoning.
  it('renders "Log in" at the chrome\'s converged text-link size (sm), not Text\'s own 16px default', () => {
    renderWithMantine(<AuthStatus session={loggedOut} />);
    expect(screen.getByText('Log in')).toHaveStyle({ '--text-fz': 'var(--mantine-font-size-sm)' });
  });

  it('shows no account menu at all when logged out', () => {
    renderWithMantine(<AuthStatus session={loggedOut} />);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('shows an account menu, not a log in link, when logged in', () => {
    renderWithMantine(
      <AuthStatus session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: 'Ada' }} />,
    );
    expect(accountMenuName()).toBe('Ada');
    expect(screen.queryByRole('link', { name: 'Log in' })).not.toBeInTheDocument();
  });

  /** The display name is deliberately no longer visible bar TEXT -- that
   * run of text is most of the 12px that pushed the authenticated bar
   * onto a second row at 1440px. It must still reach assistive tech,
   * though, which is what the case above pins; this one pins the other
   * half of that trade, so a future change that puts the name back as
   * bar text has to be a deliberate one. */
  it('does not render the display name as visible bar text', () => {
    renderWithMantine(
      <AuthStatus session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: 'Ada' }} />,
    );
    expect(screen.queryByText('Ada')).not.toBeInTheDocument();
  });

  it('falls back to the email when logged in with no name', () => {
    renderWithMantine(
      <AuthStatus session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: null }} />,
    );
    expect(accountMenuName()).toBe('a@b.com');
  });

  /** An identity provider with no name on file for a user sends a BLANK
   * `name` claim rather than omitting it, and it reaches the session shape
   * as `''` -- which `??` treats as a perfectly good label, leaving the
   * account control with an empty accessible name. Same defect the group
   * member list and shared-train attribution had. */
  it('falls back to the email when the name is blank rather than null', () => {
    renderWithMantine(
      <AuthStatus session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: '   ' }} />,
    );
    expect(accountMenuName()).toBe('a@b.com');
  });

  it('falls back to "Signed in" when both name and email are blank', () => {
    renderWithMantine(<AuthStatus session={{ authenticated: true, id: 'u1', email: '', name: '' }} />);
    expect(accountMenuName()).toBe('Signed in');
  });

  it('falls back to "Signed in" when both name and email are null', () => {
    renderWithMantine(<AuthStatus session={{ authenticated: true, id: 'u1', email: null, name: null }} />);
    expect(accountMenuName()).toBe('Signed in');
  });

  // Review §3.1.3: /chat was undiscoverable -- this is the account menu's
  // half of the fix (the drawer's own is in AppNavBar.test.tsx).
  it('adds a Chat entry to the account menu only when chatAllowed is true', async () => {
    renderWithMantine(
      <AuthStatus
        session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: 'Ada' }}
        chatAllowed
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Account menu for Ada' }));
    // `hidden: true` -- same as AccountMenu.test.tsx's own `menuItem`
    // helper: Mantine's dropdown content can be aria-hidden mid-transition.
    expect(await screen.findByRole('menuitem', { name: 'Chat', hidden: true })).toHaveAttribute(
      'href',
      '/chat',
    );
  });

  it('omits Chat from the account menu by default', () => {
    renderWithMantine(
      <AuthStatus session={{ authenticated: true, id: 'u1', email: 'a@b.com', name: 'Ada' }} />,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Account menu for Ada' }));
    expect(screen.queryByRole('menuitem', { name: 'Chat' })).not.toBeInTheDocument();
  });
});
