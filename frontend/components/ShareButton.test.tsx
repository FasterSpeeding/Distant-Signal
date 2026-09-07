import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { screen, fireEvent, waitFor, act } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ShareButton } from './ShareButton';

// jsdom implements neither `navigator.share` nor `navigator.clipboard` --
// both are defined per-test below via `Object.defineProperty` (rather than
// `vi.stubGlobal`, since these live on `navigator`, not on `globalThis`
// itself) so each test controls exactly what's "available" in this
// browser, matching how ShareButton feature-detects them.
function stubShare(impl: ReturnType<typeof vi.fn> | undefined) {
  Object.defineProperty(navigator, 'share', {
    value: impl,
    writable: true,
    configurable: true,
  });
}

function stubClipboard(writeText: ReturnType<typeof vi.fn>) {
  Object.defineProperty(navigator, 'clipboard', {
    value: { writeText },
    writable: true,
    configurable: true,
  });
}

describe('ShareButton', () => {
  beforeEach(() => {
    stubShare(undefined);
    stubClipboard(vi.fn().mockResolvedValue(undefined));
  });

  afterEach(() => {
    // Return `navigator` to a clean slate between tests -- `configurable:
    // true` above is what makes this deletion possible.
    // @ts-expect-error -- deliberately removing a property TS believes is
    // always present on Navigator.
    delete navigator.share;
    // @ts-expect-error -- same, for clipboard.
    delete navigator.clipboard;
    vi.useRealTimers();
  });

  it('starts with the default "Share this page" label', () => {
    renderWithMantine(<ShareButton />);
    expect(screen.getByLabelText('Share this page')).toBeInTheDocument();
  });

  it('calls navigator.share with the current URL when available', async () => {
    const shareMock = vi.fn().mockResolvedValue(undefined);
    stubShare(shareMock);

    renderWithMantine(<ShareButton />);
    fireEvent.click(screen.getByLabelText('Share this page'));

    await waitFor(() => {
      expect(shareMock).toHaveBeenCalledWith(
        expect.objectContaining({ url: window.location.href }),
      );
    });
  });

  it('does not fall back to the clipboard once navigator.share succeeds', async () => {
    const shareMock = vi.fn().mockResolvedValue(undefined);
    stubShare(shareMock);
    const writeTextMock = vi.mocked(navigator.clipboard.writeText);

    renderWithMantine(<ShareButton />);
    fireEvent.click(screen.getByLabelText('Share this page'));

    await waitFor(() => expect(shareMock).toHaveBeenCalledTimes(1));
    expect(writeTextMock).not.toHaveBeenCalled();
    // No transient "Copied!" state either -- the OS's own share sheet
    // already gave feedback.
    expect(screen.getByLabelText('Share this page')).toBeInTheDocument();
  });

  it('falls back to the clipboard when navigator.share does not exist', async () => {
    stubShare(undefined);
    const writeTextMock = vi.mocked(navigator.clipboard.writeText);

    renderWithMantine(<ShareButton />);
    fireEvent.click(screen.getByLabelText('Share this page'));

    await waitFor(() => {
      expect(writeTextMock).toHaveBeenCalledWith(window.location.href);
    });
  });

  it('shows a transient "Copied!" state after a clipboard fallback, which reverts after ~2s', async () => {
    // Fake timers so the revert can be asserted deterministically instead
    // of actually waiting out the real 2s. `waitFor`'s own polling isn't
    // used here -- it relies on real timers under the hood, and vitest's
    // fake timers aren't jest's (the pattern @testing-library/dom
    // special-cases), so combining the two would just hang. Flushing the
    // one microtask hop from the mocked (already-resolved) clipboard
    // promise inside `act` is enough to observe the state past it.
    vi.useFakeTimers();
    stubShare(undefined);

    renderWithMantine(<ShareButton />);
    await act(async () => {
      fireEvent.click(screen.getByLabelText('Share this page'));
      await Promise.resolve();
    });

    expect(screen.getByLabelText('Copied!')).toBeInTheDocument();

    act(() => {
      vi.advanceTimersByTime(2000);
    });

    expect(screen.getByLabelText('Share this page')).toBeInTheDocument();
    expect(screen.queryByLabelText('Copied!')).not.toBeInTheDocument();
  });

  it('a cancelled native share (AbortError) does not fall back to the clipboard or error out', async () => {
    const abortError = new DOMException('The user aborted a request.', 'AbortError');
    const shareMock = vi.fn().mockRejectedValue(abortError);
    stubShare(shareMock);
    const writeTextMock = vi.mocked(navigator.clipboard.writeText);

    renderWithMantine(<ShareButton />);
    fireEvent.click(screen.getByLabelText('Share this page'));

    await waitFor(() => expect(shareMock).toHaveBeenCalledTimes(1));
    expect(writeTextMock).not.toHaveBeenCalled();
    // Stays on the default label -- no "Copied!" state, and no thrown
    // error breaks the click.
    expect(screen.getByLabelText('Share this page')).toBeInTheDocument();
  });

  it('falls back to the clipboard when navigator.share exists but rejects with a non-abort error', async () => {
    const shareMock = vi.fn().mockRejectedValue(new Error('no share target configured'));
    stubShare(shareMock);
    const writeTextMock = vi.mocked(navigator.clipboard.writeText);

    renderWithMantine(<ShareButton />);
    fireEvent.click(screen.getByLabelText('Share this page'));

    await waitFor(() => {
      expect(writeTextMock).toHaveBeenCalledWith(window.location.href);
    });
  });
});
