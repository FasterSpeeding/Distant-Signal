import { describe, it, expect } from 'vitest';
import { render, renderHook, screen, act } from '@testing-library/react';
import { useState, type ReactNode } from 'react';
import { GroupSummariesProvider, useGroupSummaries } from './useGroupSummaries';
import type { GroupSummary } from './types';

const FIXTURE: GroupSummary[] = [
  { id: 'grp-1', name: 'Family', role: 'owner', memberCount: 3 },
  { id: 'grp-2', name: 'Commuters', role: 'member', memberCount: 5 },
];

function wrapperWithGroups(groups: GroupSummary[] | null) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return <GroupSummariesProvider groups={groups}>{children}</GroupSummariesProvider>;
  };
}

describe('useGroupSummaries', () => {
  // The whole point of moving this off a `useEffect` fetch: the value is
  // there on the very first render, no `waitFor`/`act` needed to let a
  // fetch-then-setState chain settle -- this is the regression test for the
  // cold-start race itself. If this hook were still doing its own fetch,
  // `result.current.groups` here would be `[]` on this synchronous read
  // (mount-time state) rather than the populated fixture.
  it('reflects a populated groups list immediately, with no async wait', () => {
    const { result } = renderHook(() => useGroupSummaries(), { wrapper: wrapperWithGroups(FIXTURE) });
    expect(result.current.groups).toEqual(FIXTURE);
  });

  it('reflects an authenticated-but-group-less caller as an empty array', () => {
    const { result } = renderHook(() => useGroupSummaries(), { wrapper: wrapperWithGroups([]) });
    expect(result.current.groups).toEqual([]);
  });

  // `null` (anonymous, per `getMyGroups()`'s own null-on-401 convention)
  // collapses to the same `[]` an empty list produces -- this hook's
  // existing callers (`TrackThisTrainButton`/`TrackTrainForm`/
  // `AddToGroupButton`) all just check `.length`/`.map`, and treating both
  // identically is the exact contract this hook had before this context
  // existed.
  it('collapses an anonymous visitor (null) to an empty array, same as a real empty list', () => {
    const { result } = renderHook(() => useGroupSummaries(), { wrapper: wrapperWithGroups(null) });
    expect(result.current.groups).toEqual([]);
  });

  // Fail-safe posture: a component rendered with no `GroupSummariesProvider`
  // ancestor at all (every test that doesn't explicitly wrap itself; in
  // production this would mean something is structurally broken in
  // RootLayout) must still resolve to "nothing to offer", not throw or
  // crash rendering -- same non-throwing-default reasoning as
  // `ConnectivityMonitor.tsx`'s own `ConnectivityContext`.
  it('defaults to an empty array with no provider in the tree at all', () => {
    const { result } = renderHook(() => useGroupSummaries());
    expect(result.current.groups).toEqual([]);
  });
});

describe('GroupSummariesProvider', () => {
  // The provider itself must pass a populated list, an empty list, and
  // `null` through to the context UNCHANGED -- it is not supposed to be the
  // place that collapses `null` to `[]` (that collapse is `useGroupSummaries`'s
  // job, so a future consumer that needs the real anonymous/empty
  // distinction can still get it straight from the context).
  it('passes a populated groups list through unchanged', () => {
    const { result } = renderHook(() => useGroupSummaries(), { wrapper: wrapperWithGroups(FIXTURE) });
    expect(result.current.groups).toEqual(FIXTURE);
  });

  // Deliberately NOT `renderHook`'s own `rerender`/`initialProps`: those
  // only feed new arguments to the *render callback* (`useGroupSummaries`
  // takes none), not to a custom `wrapper`'s own props -- there is no
  // supported way to hand a wrapper a changing prop through that API. This
  // instead drives a parent's state, the same tactic
  // `ConnectivityMonitor.test.tsx` uses for its own "server re-renders with
  // a new value" coverage, modelling one real RootLayout re-render per
  // `setGroups` call.
  it('re-renders consumers with a new groups value when its own prop changes', () => {
    function Consumer() {
      const { groups } = useGroupSummaries();
      return <div data-testid="names">{groups.map((g) => g.name).join(',')}</div>;
    }
    let setGroups: (groups: GroupSummary[] | null) => void = () => {};
    function Harness() {
      const [groups, setter] = useState<GroupSummary[] | null>([]);
      setGroups = setter;
      return (
        <GroupSummariesProvider groups={groups}>
          <Consumer />
        </GroupSummariesProvider>
      );
    }
    render(<Harness />);
    expect(screen.getByTestId('names')).toHaveTextContent('');

    // Models a client-side navigation or an AutoRefresh-triggered
    // `router.refresh()`: RootLayout re-runs `getMyGroups()` and passes a
    // new value down -- this must reach an already-mounted consumer, not
    // just a freshly-mounted one. See `GroupSummariesProvider`'s own doc
    // comment on why it holds no state of its own for exactly this reason.
    act(() => setGroups(FIXTURE));
    expect(screen.getByTestId('names')).toHaveTextContent('Family,Commuters');
  });
});
