import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, fireEvent } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { LoginPromptModal } from './LoginPromptModal';

const mockUsePathname = vi.fn();
const mockUseSearchParams = vi.fn();
vi.mock('next/navigation', () => ({
  usePathname: () => mockUsePathname(),
  useSearchParams: () => mockUseSearchParams(),
}));

describe('LoginPromptModal', () => {
  beforeEach(() => {
    mockUsePathname.mockReturnValue('/lines');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
  });

  it('renders nothing interactive when opened is false', () => {
    renderWithMantine(
      <LoginPromptModal opened={false} onClose={vi.fn()}>
        Log in to pin this line.
      </LoginPromptModal>,
    );
    expect(screen.queryByText('Log in to pin this line.')).not.toBeInTheDocument();
    expect(screen.queryByRole('link', { name: 'Log in' })).not.toBeInTheDocument();
  });

  it('renders the fixed title, the body children, and a Log in link with the correct return_to href when opened', () => {
    renderWithMantine(
      <LoginPromptModal opened={true} onClose={vi.fn()}>
        Log in to pin this line.
      </LoginPromptModal>,
    );
    expect(screen.getByText('Log in required')).toBeInTheDocument();
    expect(screen.getByText('Log in to pin this line.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toHaveAttribute(
      'href',
      '/api/auth/login?return_to=%2Flines',
    );
  });

  it('calls onClose when the close button fires', () => {
    const onClose = vi.fn();
    renderWithMantine(
      <LoginPromptModal opened={true} onClose={onClose}>
        Log in to pin this line.
      </LoginPromptModal>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalled();
  });
});
