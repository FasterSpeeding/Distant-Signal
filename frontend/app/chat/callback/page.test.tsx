import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import ChatCallbackPage from './page';

const mockAuth = vi.fn();
const mockReplace = vi.fn();

vi.mock('@modelcontextprotocol/sdk/client/auth.js', () => ({
  auth: (...args: unknown[]) => mockAuth(...args),
}));
vi.mock('next/navigation', () => ({
  useRouter: () => ({ replace: mockReplace }),
}));

function renderAt(search: string) {
  window.history.pushState({}, '', `/chat/callback${search}`);
  return renderWithMantine(<ChatCallbackPage />);
}

describe('ChatCallbackPage', () => {
  beforeEach(() => {
    localStorage.clear();
    mockAuth.mockReset();
    mockReplace.mockReset();
    vi.stubEnv('NEXT_PUBLIC_RAILMCP_PUBLIC_URL', 'https://mcp.example.com');
  });

  it('exchanges the code and redirects to /chat on success', async () => {
    mockAuth.mockResolvedValue('AUTHORIZED');
    renderAt('?code=abc123');
    await waitFor(() => expect(mockReplace).toHaveBeenCalledWith('/chat'));
    expect(mockAuth).toHaveBeenCalledWith(
      expect.anything(),
      expect.objectContaining({ serverUrl: 'https://mcp.example.com', authorizationCode: 'abc123' }),
    );
  });

  it('shows an error and does not redirect when no code is present', async () => {
    renderAt('');
    expect(await screen.findByText(/no authorization code/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('shows an error when the exchange itself fails', async () => {
    mockAuth.mockRejectedValue(new Error('token exchange failed'));
    renderAt('?code=abc123');
    expect(await screen.findByText(/token exchange failed/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });

  it('shows an error when auth() returns REDIRECT instead of AUTHORIZED', async () => {
    mockAuth.mockResolvedValue('REDIRECT');
    renderAt('?code=abc123');
    expect(await screen.findByText(/did not complete/i)).toBeInTheDocument();
    expect(mockReplace).not.toHaveBeenCalled();
  });
});
