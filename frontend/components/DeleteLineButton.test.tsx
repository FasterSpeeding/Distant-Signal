import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DeleteLineButton } from './DeleteLineButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
}));

describe('DeleteLineButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('does not call DELETE until the confirmation modal is confirmed', () => {
    const fetchMock = vi.mocked(fetch);
    renderWithMantine(<DeleteLineButton id="custom-my-commute" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    expect(fetchMock).not.toHaveBeenCalled();
  });

  it('DELETEs the line and redirects to /lines on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<DeleteLineButton id="custom-my-commute" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/lines/custom-my-commute', { method: 'DELETE' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/lines'));
  });

  it('shows an error and does not redirect on a failed delete', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('custom line not found', { status: 404 }));

    renderWithMantine(<DeleteLineButton id="custom-my-commute" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete' }));

    await waitFor(() => {
      expect(screen.getByText('custom line not found')).toBeInTheDocument();
    });
    expect(pushMock).not.toHaveBeenCalled();
  });
});
