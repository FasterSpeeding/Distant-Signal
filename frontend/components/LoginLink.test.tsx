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

describe('LoginLink', () => {
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
