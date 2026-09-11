import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import GroupsPage from './page';
import { getMyGroups } from '@/lib/api';

vi.mock('@/lib/api', () => ({
  getMyGroups: vi.fn(),
}));

vi.mock('next/navigation', () => ({
  usePathname: () => '/groups',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('GroupsPage', () => {
  it('shows a login prompt when getMyGroups returns null', async () => {
    vi.mocked(getMyGroups).mockResolvedValue(null);
    renderWithMantine(await GroupsPage());
    expect(await screen.findByText('Log in to see your groups.')).toBeInTheDocument();
  });

  it('shows an empty-state message with no groups', async () => {
    vi.mocked(getMyGroups).mockResolvedValue([]);
    renderWithMantine(await GroupsPage());
    expect(screen.getByText(/not in any groups yet/)).toBeInTheDocument();
  });

  it('lists each group with its name, member count, and role badge', async () => {
    vi.mocked(getMyGroups).mockResolvedValue([
      { id: 'grp-1', name: 'Family', role: 'owner', memberCount: 3 },
    ]);
    renderWithMantine(await GroupsPage());
    expect(screen.getByText('Family')).toBeInTheDocument();
    expect(screen.getByText('3 members')).toBeInTheDocument();
    expect(screen.getByText('owner')).toBeInTheDocument();
  });

  it('singularizes the member count for exactly one member', async () => {
    vi.mocked(getMyGroups).mockResolvedValue([
      { id: 'grp-1', name: 'Solo', role: 'owner', memberCount: 1 },
    ]);
    renderWithMantine(await GroupsPage());
    expect(screen.getByText('1 member')).toBeInTheDocument();
  });
});
