import { useState } from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AutoRefresh } from './AutoRefresh';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
}));

// Stubbed so a test can drive visibility directly; the rest of
// @mantine/hooks (notably useInterval) keeps its real implementation.
const visibility = { state: 'visible' as DocumentVisibilityState };
vi.mock('@mantine/hooks', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@mantine/hooks')>()),
  useDocumentVisibility: () => visibility.state,
}));

// Deliberately NOT @testing-library's `rerender`: `renderWithMantine`
// wraps its argument in a MantineProvider, and `rerender` replaces the
// whole tree with the bare element -- a different root child type, so React
// *remounts* AutoRefresh instead of re-rendering it, firing its
// become-visible effect again and masking exactly the dependency-array bug
// the last case here guards against. Driving a parent's state keeps the
// component instance alive.
let rerenderParent: () => void = () => {};

function Harness() {
  const [, setTick] = useState(0);
  rerenderParent = () => setTick((t) => t + 1);
  return <AutoRefresh />;
}

describe('AutoRefresh', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    visibility.state = 'visible';
    refreshMock.mockClear();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('renders nothing', () => {
    // MantineProvider itself injects `<style>` tags into the container, so
    // this asserts the component contributes no elements of its own rather
    // than asserting the whole container is empty.
    const { container } = renderWithMantine(<AutoRefresh />);
    expect(container.querySelectorAll('*:not(style)')).toHaveLength(0);
  });

  it('calls router.refresh() on a 30s interval', () => {
    renderWithMantine(<AutoRefresh />);
    // Mount is not a hidden->visible transition, so it must not refresh a
    // page the server rendered milliseconds ago.
    expect(refreshMock).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    expect(refreshMock).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    expect(refreshMock).toHaveBeenCalledTimes(2);
  });

  it('stops refreshing once unmounted', () => {
    const { unmount } = renderWithMantine(<AutoRefresh />);
    unmount();
    refreshMock.mockClear();

    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('does not refresh at all while the document is hidden', () => {
    visibility.state = 'hidden';
    renderWithMantine(<AutoRefresh />);
    expect(refreshMock).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });

  it('refreshes immediately when the document becomes visible again', () => {
    visibility.state = 'hidden';
    renderWithMantine(<Harness />);
    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(refreshMock).not.toHaveBeenCalled();

    visibility.state = 'visible';
    act(() => {
      rerenderParent();
    });
    // Immediately, without waiting out another 30s window -- whatever is on
    // screen is as stale as the time the visitor spent away.
    expect(refreshMock).toHaveBeenCalledTimes(1);

    act(() => {
      vi.advanceTimersByTime(30_000);
    });
    expect(refreshMock).toHaveBeenCalledTimes(2);
  });

  // Regression guard for the dependency-array trap: `useInterval` returns a
  // fresh object literal on every render, so listing it (or a possibly
  // unstable `router`) in the effect's deps would re-run the effect on
  // every render and fire router.refresh() in a loop.
  it('does not refresh again on a re-render with unchanged visibility', () => {
    renderWithMantine(<Harness />);
    expect(refreshMock).not.toHaveBeenCalled();

    act(() => {
      rerenderParent();
      rerenderParent();
      rerenderParent();
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
