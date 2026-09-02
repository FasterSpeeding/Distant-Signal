import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoginLink } from './LoginLink';

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

describe('LoginLink', () => {
  it('disables next/link prefetch -- this href is a side-effecting backend redirect (crates/api/src/routes/auth.rs::login), not a real page, so Next must never fire it on mere viewport visibility with no click', () => {
    mockUsePathname.mockReturnValue('/stations');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginLink>Log in</LoginLink>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute('data-prefetch', 'false');
  });

  it('appends the current pathname as return_to when there is no query string', () => {
    mockUsePathname.mockReturnValue('/lines/some-line');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginLink>Log in</LoginLink>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines%2Fsome-line',
    );
  });

  it('appends pathname + query string, URL-encoded, when a query string is present', () => {
    mockUsePathname.mockReturnValue('/lines/some-line');
    mockUseSearchParams.mockReturnValue(new URLSearchParams('tab=history'));
    renderWithMantine(<LoginLink>Log in</LoginLink>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines%2Fsome-line%3Ftab%3Dhistory',
    );
  });

  it('passes through the underline prop to TextLink', () => {
    mockUsePathname.mockReturnValue('/');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginLink underline="always">Log in to pin</LoginLink>);
    expect(screen.getByRole('link', { name: 'Log in to pin' })).toHaveAttribute(
      'data-text-link',
      'always',
    );
  });

  it('renders the root path correctly', () => {
    mockUsePathname.mockReturnValue('/');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    renderWithMantine(<LoginLink>Log in</LoginLink>);
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2F',
    );
  });
});
