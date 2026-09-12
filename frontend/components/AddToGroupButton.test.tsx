import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import type { ReactElement } from 'react';
import { screen, fireEvent, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { GroupSummariesProvider } from '@/lib/useGroupSummaries';
import { AddToGroupButton } from './AddToGroupButton';
import type { GroupSummary } from '@/lib/types';

vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: vi.fn() }),
  usePathname: () => '/train/by-id/42',
  useSearchParams: () => new URLSearchParams(''),
}));

const GROUPS_FIXTURE: GroupSummary[] = [
  { id: 'grp-1', name: 'Commuters', role: 'member', memberCount: 3 },
  { id: 'grp-2', name: 'Family', role: 'owner', memberCount: 2 },
];

// `useGroupSummaries` now reads from `GroupSummariesProvider`'s context
// instead of fetching `/api/groups` itself -- every render below supplies
// its own groups value directly, synchronously, rather than mocking that
// fetch (see `lib/useGroupSummaries.tsx`'s own doc comment for why: the
// value is present at first render, no fetch-then-setState chain to await).
function renderWithGroups(ui: ReactElement, groups: GroupSummary[] | null = []) {
  return renderWithMantine(<GroupSummariesProvider groups={groups}>{ui}</GroupSummariesProvider>);
}

describe('AddToGroupButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('renders nothing when the viewer is in zero groups', () => {
    renderWithGroups(<AddToGroupButton trainSubscriptionId={7} />, []);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  // The context's own fail-safe posture (`GroupSummariesProvider`'s doc
  // comment): an anonymous visitor and any upstream `getMyGroups()` failure
  // both reach this component as `null`, collapsed to "nothing to offer" --
  // same as the zero-groups case above.
  it('renders nothing for an anonymous visitor (null groups)', () => {
    renderWithGroups(<AddToGroupButton trainSubscriptionId={7} />, null);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  // No `GroupSummariesProvider` ancestor at all -- the context's own
  // non-throwing default must still resolve to "nothing to offer" rather
  // than crashing this component's unconditional `groups.length` check.
  it('renders nothing with no GroupSummariesProvider in the tree at all', () => {
    renderWithMantine(<AddToGroupButton trainSubscriptionId={7} />);
    expect(screen.queryByRole('button')).not.toBeInTheDocument();
  });

  it('shows the button and a group picker once the viewer has at least one group', async () => {
    renderWithGroups(<AddToGroupButton trainSubscriptionId={7} />, GROUPS_FIXTURE);

    const button = await screen.findByRole('button', { name: 'Add to group' });
    fireEvent.click(button);

    const [select] = await screen.findAllByLabelText('Group');
    fireEvent.click(select);
    expect(await screen.findByText('Commuters')).toBeInTheDocument();
    expect(screen.getByText('Family')).toBeInTheDocument();
  });

  it('POSTs the chosen groupId and shows a success confirmation without closing the modal', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response(null, { status: 204 }));

    renderWithGroups(<AddToGroupButton trainSubscriptionId={7} />, GROUPS_FIXTURE);
    fireEvent.click(await screen.findByRole('button', { name: 'Add to group' }));
    const [select] = await screen.findAllByLabelText('Group');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText('Commuters'));
    fireEvent.click(screen.getByRole('button', { name: 'Share' }));

    await waitFor(() => {
      expect(fetchMock).toHaveBeenCalledWith(
        '/api/groups/grp-1/trains',
        expect.objectContaining({ method: 'POST', body: JSON.stringify({ trainSubscriptionId: 7 }) }),
      );
    });
    expect(await screen.findByText('Added to Commuters.')).toBeInTheDocument();
    // The modal itself is still open -- the confirm button is still there,
    // allowing a second group to be picked without reopening.
    expect(screen.getByRole('button', { name: 'Share' })).toBeInTheDocument();
  });

  it('shows a real error, not silence, when the share request fails', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('Something went wrong', { status: 500 }));

    renderWithGroups(<AddToGroupButton trainSubscriptionId={7} />, GROUPS_FIXTURE);
    fireEvent.click(await screen.findByRole('button', { name: 'Add to group' }));
    const [select] = await screen.findAllByLabelText('Group');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText('Commuters'));
    fireEvent.click(screen.getByRole('button', { name: 'Share' }));

    expect(await screen.findByText('Something went wrong')).toBeInTheDocument();
    expect(screen.queryByText(/Added to/)).not.toBeInTheDocument();
  });

  it('shows a login prompt on a 401 rather than the raw rejection text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('unauthorized', { status: 401 }));

    renderWithGroups(<AddToGroupButton trainSubscriptionId={7} />, GROUPS_FIXTURE);
    fireEvent.click(await screen.findByRole('button', { name: 'Add to group' }));
    const [select] = await screen.findAllByLabelText('Group');
    fireEvent.click(select);
    fireEvent.click(await screen.findByText('Commuters'));
    fireEvent.click(screen.getByRole('button', { name: 'Share' }));

    expect(await screen.findByText('Log in to share this train')).toBeInTheDocument();
    expect(screen.queryByText('unauthorized')).not.toBeInTheDocument();
  });
});
