import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoginButton } from './LoginButton';

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

describe('LoginButton', () => {
  it('renders as a filled Button, not a text link (review §2.16 CTA promotion)', () => {
    mockUsePathname.mockReturnValue('/groups/join/tok123');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginButton>Log in to join Family</LoginButton>);
    expect(screen.getByRole('button', { name: 'Log in to join Family' })).toBeInTheDocument();
  });

  it('still exposes the login href on an ancestor anchor, same as LoginLink', () => {
    mockUsePathname.mockReturnValue('/groups/join/tok123');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginButton>Log in to join Family</LoginButton>);
    expect(screen.getByRole('link', { name: 'Log in to join Family' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Fgroups%2Fjoin%2Ftok123',
    );
  });

  it('disables next/link prefetch -- same side-effecting-backend-endpoint reasoning as LoginLink', () => {
    mockUsePathname.mockReturnValue('/connect-claude');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginButton>Log in</LoginButton>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute('data-prefetch', 'false');
  });

  it('carries an optional title hint that the action needs an account', () => {
    mockUsePathname.mockReturnValue('/connect-claude');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginButton title="Log in — needs a Distant Signal account">Log in</LoginButton>);
    expect(screen.getByRole('button', { name: 'Log in' })).toHaveAttribute(
      'title',
      'Log in — needs a Distant Signal account',
    );
  });

  it('renders with no title at all when none is given', () => {
    mockUsePathname.mockReturnValue('/connect-claude');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginButton>Log in</LoginButton>);
    expect(screen.getByRole('button', { name: 'Log in' })).not.toHaveAttribute('title');
  });
});
