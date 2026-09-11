import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RemoveGroupTrainButton } from './RemoveGroupTrainButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('RemoveGroupTrainButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('DELETEs the shared train and refreshes on confirm', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<RemoveGroupTrainButton groupId="grp-1" trainSubscriptionId={42} />);
    fireEvent.click(screen.getByRole('button', { name: 'Remove from group' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm remove train from group' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm remove train from group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/trains/42', { method: 'DELETE' });
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });
});
