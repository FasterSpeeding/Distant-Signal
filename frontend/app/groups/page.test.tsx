import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import GroupsPage, { metadata } from './page';
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

  it('renders a server-rendered login action alongside the modal, not just the client-only prompt', async () => {
    vi.mocked(getMyGroups).mockResolvedValue(null);
    renderWithMantine(await GroupsPage());
    const link = screen.getByRole('link', { name: 'Log in to see your groups' });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Fgroups');
  });

  // Review §2.16: this used to be an underlined text link -- promoted to a
  // filled button so the anonymous visitor's one action here has the same
  // visual weight as the "Create group" action an authenticated visitor
  // sees in its place.
  it('renders the login action as a filled button, not a plain text link', async () => {
    vi.mocked(getMyGroups).mockResolvedValue(null);
    renderWithMantine(await GroupsPage());
    expect(screen.getByRole('button', { name: 'Log in to see your groups' })).toBeInTheDocument();
  });

  // Review §3.2.8, deliberately on top of Task 1.1's root-cause `<main>`
  // fix, not instead of it: a list of short name/badge rows stretching the
  // full content width reads as sparse rather than deliberate.
  it('caps the content width at 640px', async () => {
    vi.mocked(getMyGroups).mockResolvedValue([]);
    renderWithMantine(await GroupsPage());
    const stack = screen.getByRole('heading', { name: 'Groups', level: 1 }).closest('.mantine-Stack-root');
    expect(stack).toHaveStyle({ maxWidth: 'calc(40rem * var(--mantine-scale))' });
  });

  it('caps the content width at 640px for the anonymous branch too', async () => {
    vi.mocked(getMyGroups).mockResolvedValue(null);
    renderWithMantine(await GroupsPage());
    const stack = screen.getByRole('heading', { name: 'Groups', level: 1 }).closest('.mantine-Stack-root');
    expect(stack).toHaveStyle({ maxWidth: 'calc(40rem * var(--mantine-scale))' });
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

  // Review §3.2.5: the card used to be a bare `<Link>` with
  // `textDecoration: 'none'; color: 'inherit'` and no other signal that it
  // was clickable besides the cursor.
  it('signals that each group card is clickable with a hover/focus hook and a trailing chevron', async () => {
    vi.mocked(getMyGroups).mockResolvedValue([
      { id: 'grp-1', name: 'Family', role: 'owner', memberCount: 3 },
    ]);
    renderWithMantine(await GroupsPage());
    const link = screen.getByRole('link', { name: /Family/ });
    expect(link).toHaveAttribute('data-group-card-link', 'true');
    expect(link.querySelector('[data-group-card]')).not.toBeNull();
    expect(screen.getByText('›')).toBeInTheDocument();
  });
});

describe('metadata', () => {
  it('titles the page after its own heading, suffixed with the site name', () => {
    expect(metadata.title).toBe('Groups — Distant Signal');
  });

  it('explains what a group is rather than inheriting the generic site description', () => {
    expect(metadata.description).toBe(
      'Groups are how tracked trains and custom lines get shared with other people. Log in to see the ones you belong to — each with its member count and your role in it — or create a group and invite people to it.',
    );
  });

  it('hedges the listing behind logging in, which is the only branch a bot can render', () => {
    // `getMyGroups()` returns null on a 401 and this page then shows only
    // "Log in to see your groups." -- and no link-unfurler bot carries a
    // session cookie, so that IS the page they preview. Describing "your
    // groups, listed" flatly would promise a list the link's recipient
    // will not find.
    expect(metadata.description).toMatch(/Log in to see/);
  });

  it('names both kinds of thing a group shares, not just trains', () => {
    // The empty state on this page mentions only tracked trains, but
    // app/groups/[id]/page.tsx renders a shared-trains section AND a
    // "Shared custom lines" one (getGroupTrains/getGroupCustomLines), so
    // the preview card describes both.
    expect(metadata.description).toMatch(/tracked trains and custom lines/);
  });

  it('mirrors the same title and description into openGraph and twitter', () => {
    // See the equivalent case in app/incidents/page.test.tsx for why the
    // mirror is asserted against literals rather than against
    // `metadata.title`/`.description`.
    expect(metadata.openGraph).toMatchObject({
      title: 'Groups — Distant Signal',
      description:
        'Groups are how tracked trains and custom lines get shared with other people. Log in to see the ones you belong to — each with its member count and your role in it — or create a group and invite people to it.',
      type: 'website',
    });
    expect(metadata.twitter).toMatchObject({
      card: 'summary',
      title: 'Groups — Distant Signal',
      description:
        'Groups are how tracked trains and custom lines get shared with other people. Log in to see the ones you belong to — each with its member count and your role in it — or create a group and invite people to it.',
    });
  });
});
