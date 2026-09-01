import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoginPrompt } from './LoginPrompt';

describe('LoginPrompt', () => {
  it('renders "Log in to {verb}" linking to /api/auth/login', () => {
    renderWithMantine(<LoginPrompt verb="pin" />);
    expect(screen.getByRole('link', { name: 'Log in to pin' })).toHaveAttribute('href', '/api/auth/login');
  });
});
