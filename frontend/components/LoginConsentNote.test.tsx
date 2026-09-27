import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoginConsentNote, LoginConsentProvider } from './LoginConsentNote';
import { LoginPromptModal } from './LoginPromptModal';
import { LoginButton } from './LoginButton';

vi.mock('next/navigation', () => ({
  usePathname: () => '/groups',
  useSearchParams: () => new URLSearchParams(''),
}));

// LEG-1: the "by logging in you agree" line, gated on the legal pages.
describe('LoginConsentNote', () => {
  it('renders nothing without a provider, or while the legal pages are unpublished', () => {
    renderWithMantine(
      <>
        <LoginConsentNote />
        <LoginConsentProvider published={false}>
          <LoginConsentNote />
        </LoginConsentProvider>
      </>,
    );
    expect(document.querySelector('[data-login-consent]')).toBeNull();
  });

  it('links the terms and privacy notice and states the minimum age once published', () => {
    renderWithMantine(
      <LoginConsentProvider published>
        <LoginConsentNote />
      </LoginConsentProvider>,
    );
    expect(screen.getByText(/By logging in you agree to our/)).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'terms of use' })).toHaveAttribute('href', '/terms');
    expect(screen.getByRole('link', { name: 'privacy notice' })).toHaveAttribute('href', '/privacy');
    expect(screen.getByText(/You must be 18 or over/)).toBeInTheDocument();
  });

  it('appears in the login prompt modal and beside the login button once published', () => {
    renderWithMantine(
      <LoginConsentProvider published>
        <LoginPromptModal opened onClose={() => {}}>
          Log in to continue.
        </LoginPromptModal>
        <LoginButton>Log in to join</LoginButton>
      </LoginConsentProvider>,
    );
    expect(document.querySelectorAll('[data-login-consent]')).toHaveLength(2);
  });
});
