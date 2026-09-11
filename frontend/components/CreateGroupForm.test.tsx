import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { CreateGroupForm } from './CreateGroupForm';

const pushMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: pushMock }),
  usePathname: () => '/groups/new',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('CreateGroupForm', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    pushMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('Create group is disabled with an empty name', () => {
    renderWithMantine(<CreateGroupForm />);
    expect(screen.getByRole('button', { name: 'Create group' })).toBeDisabled();
  });

  it('creates the group, rotates its invite link, and navigates to it', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/groups') {
        return Promise.resolve(new Response(JSON.stringify({ id: 'grp-1', name: 'Family' }), { status: 200 }));
      }
      return Promise.resolve(new Response(null, { status: 204 }));
    });

    renderWithMantine(<CreateGroupForm />);
    fireEvent.change(screen.getByLabelText('Group name', { exact: false }), { target: { value: 'Family' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create group' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups/grp-1'));
    expect(fetchMock).toHaveBeenCalledWith(
      '/api/groups',
      expect.objectContaining({ method: 'POST', body: JSON.stringify({ name: 'Family' }) }),
    );
    expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/invite-link', { method: 'POST' });
  });

  it('navigates even if the follow-up invite-link call fails', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockImplementation((input) => {
      const url = typeof input === 'string' ? input : (input as Request).url;
      if (url === '/api/groups') {
        return Promise.resolve(new Response(JSON.stringify({ id: 'grp-2', name: 'Crew' }), { status: 200 }));
      }
      return Promise.reject(new Error('network blip'));
    });

    renderWithMantine(<CreateGroupForm />);
    fireEvent.change(screen.getByLabelText('Group name', { exact: false }), { target: { value: 'Crew' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create group' }));

    await waitFor(() => expect(pushMock).toHaveBeenCalledWith('/groups/grp-2'));
  });

  it('a 401 on group creation shows a login prompt', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('no session', { status: 401 }));

    renderWithMantine(<CreateGroupForm />);
    fireEvent.change(screen.getByLabelText('Group name', { exact: false }), { target: { value: 'Family' } });
    fireEvent.click(screen.getByRole('button', { name: 'Create group' }));

    expect(await screen.findByText('Log in to create a group.')).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in' })).toBeInTheDocument();
    expect(pushMock).not.toHaveBeenCalled();
  });
});
