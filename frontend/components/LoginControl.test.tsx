import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoginControl } from './LoginControl';

const mockUsePathname = vi.fn();
const mockUseSearchParams = vi.fn();
vi.mock('next/navigation', () => ({
  usePathname: () => mockUsePathname(),
  useSearchParams: () => mockUseSearchParams(),
}));

// See TextLink.test.tsx's own comment on this mock shape -- surfaces
// `prefetch`, which real next/link never renders as a DOM attribute, so
// this is the only way to assert it landed on the underlying `<Link>`.
vi.mock('next/link', () => ({
  default: ({
    href,
    children,
    prefetch,
    ...rest
  }: {
    href: string;
    children: React.ReactNode;
    prefetch?: boolean;
    [key: string]: unknown;
  }) => (
    <a href={href} data-prefetch={String(prefetch)} {...rest}>
      {children}
    </a>
  ),
}));

describe('LoginControl (link)', () => {
  it('disables next/link prefetch -- this href is a side-effecting backend redirect (crates/api/src/routes/auth.rs::login), not a real page, so Next must never fire it on mere viewport visibility with no click', () => {
    mockUsePathname.mockReturnValue('/stations');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginControl>Log in</LoginControl>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute('data-prefetch', 'false');
  });

  it('appends the current pathname as return_to when there is no query string', () => {
    mockUsePathname.mockReturnValue('/lines/some-line');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginControl>Log in</LoginControl>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines%2Fsome-line',
    );
  });

  it('appends pathname + query string, URL-encoded, when a query string is present', () => {
    mockUsePathname.mockReturnValue('/lines/some-line');
    mockUseSearchParams.mockReturnValue(new URLSearchParams('tab=history'));
    renderWithMantine(<LoginControl>Log in</LoginControl>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines%2Fsome-line%3Ftab%3Dhistory',
    );
  });

  it('passes through the underline prop to TextLink', () => {
    mockUsePathname.mockReturnValue('/');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginControl underline="always">Log in to pin</LoginControl>);
    expect(screen.getByRole('link', { name: 'Log in to pin' })).toHaveAttribute('data-text-link', 'always');
  });

  // Review §2.16: AuthStatus's nav-bar use of this component now sets
  // `size="sm"` explicitly, converging on the size the rest of the chrome's
  // text-link-styled controls already use.
  it('passes through the size prop to TextLink', () => {
    mockUsePathname.mockReturnValue('/');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginControl size="sm">Log in</LoginControl>);
    expect(screen.getByText('Log in')).toHaveStyle({ '--text-fz': 'var(--mantine-font-size-sm)' });
  });

  it('renders the root path correctly', () => {
    mockUsePathname.mockReturnValue('/');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginControl>Log in</LoginControl>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute('href', '/api/auth/login?return_to=%2F');
  });
});

describe('LoginControl (button)', () => {
  it('is a filled button-styled link to the same login href, not a <button> inside an <a>', () => {
    mockUsePathname.mockReturnValue('/groups/join/tok123');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginControl variant="button">Log in to join Family</LoginControl>);
    const link = screen.getByRole('link', { name: 'Log in to join Family' });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Fgroups%2Fjoin%2Ftok123');
    expect(link).toHaveClass('mantine-Button-root');
    expect(link.querySelector('button')).toBeNull();
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('disables next/link prefetch, like the link variant', () => {
    mockUsePathname.mockReturnValue('/groups');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginControl variant="button">Log in</LoginControl>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute('data-prefetch', 'false');
  });

  it('carries an optional title hint, and none when not given', () => {
    mockUsePathname.mockReturnValue('/groups');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(
      <>
        <LoginControl variant="button" title="Log in (needs a Distant Signal account)">
          Log in
        </LoginControl>
        <LoginControl variant="button">Log in again</LoginControl>
      </>,
    );
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'title',
      'Log in (needs a Distant Signal account)',
    );
    expect(screen.getByRole('link', { name: 'Log in again' })).not.toHaveAttribute('title');
  });
});
