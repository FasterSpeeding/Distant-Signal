import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { DeleteGroupButton } from './DeleteGroupButton';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('DeleteGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('DELETEs the group and navigates to /groups', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithMantine(<DeleteGroupButton groupId="grp-1" name="Family" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete group' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete group' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1', { method: 'DELETE' });
    });
    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups'));
  });

  it('warns that the deletion affects everyone before confirming', async () => {
    renderWithMantine(<DeleteGroupButton groupId="grp-1" name="Family" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete group' }));

    expect(await screen.findByText(/deletes the group for everyone/)).toBeInTheDocument();
    expect(vi.mocked(fetch)).not.toHaveBeenCalled();
  });

  it('a 401 shows a login prompt', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<DeleteGroupButton groupId="grp-1" name="Family" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete group' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete group' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete group' }));

    expect(await screen.findByRole('link', { name: 'Log in to delete this group' })).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });

  it("shows the backend's own error message on a non-401 failure", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(
      new Response("you don't have permission to do that in this group", { status: 403 }),
    );

    renderWithMantine(<DeleteGroupButton groupId="grp-1" name="Family" />);
    fireEvent.click(screen.getByRole('button', { name: 'Delete group' }));
    await waitFor(() => screen.getByRole('button', { name: 'Confirm delete group' }));
    fireEvent.click(screen.getByRole('button', { name: 'Confirm delete group' }));

    expect(await screen.findByText("you don't have permission to do that in this group")).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });
});
