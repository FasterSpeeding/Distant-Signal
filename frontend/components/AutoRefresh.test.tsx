import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AutoRefresh } from './AutoRefresh';

const refreshMock = vi.fn();
vi.mock('next/navigation', () => ({
  useRouter: () => ({ refresh: refreshMock }),
}));

describe('AutoRefresh', () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
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

    act(() => {
      vi.advanceTimersByTime(60_000);
    });
    expect(refreshMock).not.toHaveBeenCalled();
  });
});
