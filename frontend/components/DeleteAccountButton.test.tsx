import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DeleteAccountButton, DELETE_ACCOUNT_CONFIRMATION } from './DeleteAccountButton';

const pushMock = vi.fn();
const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock, refresh: refreshMock }),
  usePathname: () => '/account',
  useSearchParams: () => new URLSearchParams(''),
}));

async function openAndType(text: string) {
  fireEvent.click(screen.getByRole('button', { name: 'Delete my account' }));
  const input = await screen.findByLabelText(`Type "${DELETE_ACCOUNT_CONFIRMATION}" to confirm`);
  fireEvent.change(input, { target: { value: text } });
}

function confirmButton() {
  return screen.getByRole('button', { name: 'Delete my account permanently' });
}

describe('DeleteAccountButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
    refreshMock.mockClear();
    localStorage.clear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('explains what is deleted, what happens to groups, and the 14-day backup retention before anything is sent', async () => {
    renderWithMantine(<DeleteAccountButton />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete my account' }));

    expect(await screen.findByText(/tracked trains, tickets, journeys and journey templates/)).toBeInTheDocument();
    expect(screen.getByText(/Groups carry on for their other members/)).toBeInTheDocument();
    expect(screen.getByText(/your data can stay in them for up to 14 days/)).toBeInTheDocument();
    expect(screen.getByText(/single sign-on account/)).toBeInTheDocument();
    expect(vi.mocked(fetch)).not.toHaveBeenCalled();
  });

  it('keeps the delete button disabled until the confirmation phrase is typed', async () => {
    renderWithMantine(<DeleteAccountButton />);
    await openAndType('delete');
    expect(confirmButton()).toBeDisabled();

    fireEvent.change(screen.getByLabelText(`Type "${DELETE_ACCOUNT_CONFIRMATION}" to confirm`), {
      target: { value: '  Delete My Account ' },
    });
    expect(confirmButton()).toBeEnabled();
  });

  it('sends DELETE /api/account with the confirmation body, clears browser-only data and leaves the page', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response(null, { status: 204 }));
    localStorage.setItem('ds-anthropic-api-key', 'sk-test');
    localStorage.setItem('ds-mcp-oauth:tokens', '{}');
    localStorage.setItem('ds-color-scheme', 'dark');

    renderWithMantine(<DeleteAccountButton />);
    await openAndType(DELETE_ACCOUNT_CONFIRMATION);
    fireEvent.click(confirmButton());

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/account/deleted'));
    expect(fetch).toHaveBeenCalledWith('/api/account', {
      method: 'DELETE',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ confirm: DELETE_ACCOUNT_CONFIRMATION }),
    });
    expect(localStorage.getItem('ds-anthropic-api-key')).toBeNull();
    expect(localStorage.getItem('ds-mcp-oauth:tokens')).toBeNull();
    expect(localStorage.getItem('ds-color-scheme')).toBe('dark');
  });

  it("shows the backend's error and stays put when deletion fails", async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('account deletion failed', { status: 500 }));

    renderWithMantine(<DeleteAccountButton />);
    await openAndType(DELETE_ACCOUNT_CONFIRMATION);
    fireEvent.click(confirmButton());

    expect(await screen.findByText("Couldn't delete your account. Try again.")).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it('asks the visitor to log in again on a 401', async () => {
    vi.mocked(fetch).mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<DeleteAccountButton />);
    await openAndType(DELETE_ACCOUNT_CONFIRMATION);
    fireEvent.click(confirmButton());

    expect(await screen.findByText(/session has expired/)).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });
});
