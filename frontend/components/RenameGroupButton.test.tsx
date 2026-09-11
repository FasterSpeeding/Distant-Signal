import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RenameGroupButton } from './RenameGroupButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

/** Opens the modal and waits for its form to be live. */
async function openModal() {
  fireEvent.click(screen.getByRole('button', { name: 'Rename' }));
  await waitFor(() => screen.getByRole('button', { name: 'Confirm rename group' }));
}

describe('RenameGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('prefills the current name and disables Save when it is cleared', async () => {
    renderWithMantine(<RenameGroupButton groupId="grp-1" currentName="Family" />);
    await openModal();

    expect(screen.getByLabelText('Group name', { exact: false })).toHaveValue('Family');
    fireEvent.change(screen.getByLabelText('Group name', { exact: false }), { target: { value: '   ' } });
    expect(screen.getByRole('button', { name: 'Confirm rename group' })).toBeDisabled();
  });

  it('PUTs the new name and refreshes', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(JSON.stringify({ id: 'grp-1', name: 'Commute crew' }), { status: 200 }));

    renderWithMantine(<RenameGroupButton groupId="grp-1" currentName="Family" />);
    await openModal();
    fireEvent.change(screen.getByLabelText('Group name', { exact: false }), { target: { value: 'Commute crew' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm rename group' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/groups/grp-1',
        expect.objectContaining({ method: 'PUT', body: JSON.stringify({ name: 'Commute crew' }) }),
      );
    });
    await waitFor(() => expect(refreshMock).toHaveBeenCalled());
  });

  it('a 401 shows a login prompt', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<RenameGroupButton groupId="grp-1" currentName="Family" />);
    await openModal();
    fireEvent.change(screen.getByLabelText('Group name', { exact: false }), { target: { value: 'Crew' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm rename group' }));

    expect(await screen.findByRole('link', { name: 'Log in to rename this group' })).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it("shows the backend's own rejection message verbatim", async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('Enter a name for this group.', { status: 400 }));

    renderWithMantine(<RenameGroupButton groupId="grp-1" currentName="Family" />);
    await openModal();
    fireEvent.change(screen.getByLabelText('Group name', { exact: false }), { target: { value: 'Crew' } });
    fireEvent.click(screen.getByRole('button', { name: 'Confirm rename group' }));

    expect(await screen.findByText('Enter a name for this group.')).toBeInTheDocument();
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
