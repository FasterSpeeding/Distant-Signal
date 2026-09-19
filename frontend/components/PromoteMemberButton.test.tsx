import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, act } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { PromoteMemberButton } from './PromoteMemberButton';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
  usePathname: () => '/groups/grp-1',
  useSearchParams: () => new URLSearchParams(''),
}));

describe('PromoteMemberButton', () => {
  beforeEach(() => {
    vi.stubGlobal('fetch', vi.fn());
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('POSTs the promote request, shows a confirmation, then refreshes', async () => {
    // Only `setTimeout`/`clearTimeout` are faked -- Mantine's `Button`
    // loading-state `Transition` schedules its own animation-frame work
    // when this component's `loading` prop flips, and faking that too
    // (vitest's default `toFake` list includes it) desyncs it from real
    // paint timing, which is what produced this test's own React "not
    // wrapped in act(...)" warning before this was narrowed.
    vi.useFakeTimers({ shouldAdvanceTime: true, toFake: ['setTimeout', 'clearTimeout'] });
    try {
      const fetchMock = vi.mocked(fetch);
      fetchMock.mockResolvedValue(new Response(JSON.stringify({ userId: 'user-2', role: 'admin' }), { status: 200 }));

      renderWithMantine(<PromoteMemberButton groupId="grp-1" userId="user-2" />);
      fireEvent.click(screen.getByRole('button', { name: 'Promote to admin' }));

      // Testing Library's own `waitFor` here, not `vi.waitFor` -- it wraps
      // each poll in `act(...)`, which the fetch mock's async continuation
      // (the `setJustPromoted(true)` state update after `await fetch(...)`
      // resolves) needs; `vi.waitFor` doesn't.
      await waitFor(() => {
        expect(fetchMock).toHaveBeenCalledWith('/api/groups/grp-1/members/user-2/promote', { method: 'POST' });
      });

      // Review §3.2.4: a brief inline confirmation replaces the button
      // before the delayed `router.refresh()` swaps this row for
      // `DemoteMemberButton` -- silence-on-success was the actual gap, not
      // the missing confirm step (that reasoning lives on the component).
      await waitFor(() => expect(screen.getByText('Promoted to admin')).toBeInTheDocument());
      expect(screen.queryByRole('button', { name: 'Promote to admin' })).not.toBeInTheDocument();
      expect(refreshMock).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(900);
      });
      expect(refreshMock).toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('a 409 shows the backend error text', async () => {
    const fetchMock = vi.mocked(fetch);
    fetchMock.mockResolvedValue(new Response('that member is already an admin or the owner', { status: 409 }));

    renderWithMantine(<PromoteMemberButton groupId="grp-1" userId="user-2" />);
    fireEvent.click(screen.getByRole('button', { name: 'Promote to admin' }));

    await waitFor(() => {
      expect(screen.getByText('that member is already an admin or the owner')).toBeInTheDocument();
    });
  });
});
