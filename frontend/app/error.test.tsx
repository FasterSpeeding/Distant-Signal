import { useState } from 'react';
import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { act, fireEvent, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ConnectivityContext } from '@/components/ConnectivityMonitor';
import ErrorBoundary from './error';

// `TextLink` is a client component that calls usePathname()/useSearchParams()
// -- same stub `app/page.test.tsx` uses for the same reason.
vi.mock('next/navigation', () => ({
  usePathname: () => '/',
  useSearchParams: () => new URLSearchParams(''),
}));

function errorWithDigest(message: string, digest?: string): Error & { digest?: string } {
  return Object.assign(new Error(message), { digest });
}

const reset = vi.fn();
const connectivityError = Object.assign(new globalThis.Error('API request to /Line/Mode failed: 500'), {
  digest: 'abc123',
});

// Same reason as ConnectivityMonitor.test.tsx: `renderWithMantine`'s
// `rerender` drops the MantineProvider wrapper, so context changes are
// driven through a parent's state instead, which also keeps the Error
// component instance (and therefore its `wasDisconnected` ref) alive --
// the whole point of the transition tests below.
let setDisconnected: (value: boolean) => void = () => {};

function Harness({ initial }: { initial: boolean }) {
  const [disconnected, setValue] = useState(initial);
  setDisconnected = setValue;
  return (
    <ConnectivityContext.Provider value={{ disconnected }}>
      <ErrorBoundary error={connectivityError} reset={reset} />
    </ConnectivityContext.Provider>
  );
}

describe('app/error.tsx', () => {
  beforeEach(() => {
    reset.mockClear();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // Not-connectivity-related behaviour: rendered with no ConnectivityContext
  // Provider, so it gets the context's own non-throwing default of
  // `disconnected: false` (see ConnectivityMonitor.tsx) -- i.e. the same
  // "ordinary error" case Harness's `initial={false}` also exercises below.
  it('never renders the raw error message', () => {
    renderWithMantine(
      <ErrorBoundary error={errorWithDigest('Minified React error #130', 'abc123')} reset={() => {}} />,
    );
    expect(screen.queryByText(/Minified React error/)).not.toBeInTheDocument();
  });

  it('logs the error instead of showing it', () => {
    const spy = vi.spyOn(console, 'error').mockImplementation(() => {});
    const error = errorWithDigest('boom', 'abc123');
    renderWithMantine(<ErrorBoundary error={error} reset={() => {}} />);
    expect(spy).toHaveBeenCalledWith('Unhandled error rendering a page', { digest: 'abc123', error });
  });

  it('shows the digest as a quotable reference when there is one', () => {
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom', 'abc123')} reset={() => {}} />);
    expect(screen.getByText('Reference: abc123')).toBeInTheDocument();
  });

  it('omits the reference line entirely for a client-side error with no digest', () => {
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom')} reset={() => {}} />);
    expect(screen.queryByText(/^Reference:/)).not.toBeInTheDocument();
  });

  it('offers a route out, not just a retry that will re-throw', () => {
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom')} reset={() => {}} />);
    const link = screen.getByRole('link', { name: 'Back to your dashboard' });
    expect(link).toHaveAttribute('href', '/');
  });

  it('still offers a Try again button that calls reset', () => {
    const localReset = vi.fn();
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom')} reset={localReset} />);
    screen.getByRole('button', { name: 'Try again' }).click();
    expect(localReset).toHaveBeenCalled();
  });

  it('renders a generic heading rather than the old hardcoded "status data" title', () => {
    renderWithMantine(<ErrorBoundary error={errorWithDigest('boom')} reset={() => {}} />);
    expect(screen.getByRole('heading', { name: 'Something went wrong' })).toBeInTheDocument();
  });

  // Connectivity-aware behaviour, per
  // docs/superpowers/specs/2026-09-02-frontend-disconnect-reconnect-ux-design.md
  // Decision 6.
  it('still lets the visitor retry by hand', () => {
    renderWithMantine(<Harness initial={false} />);

    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it('shows the generic fallback copy, not the raw error, when this is not a connectivity failure', () => {
    renderWithMantine(<Harness initial={false} />);

    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Something went wrong');
    expect(screen.queryByText(connectivityError.message)).not.toBeInTheDocument();
  });

  it('shows reconnecting copy, not the raw error, while disconnected', () => {
    renderWithMantine(<Harness initial />);

    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Trying to reconnect…');
    expect(screen.queryByText(connectivityError.message)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Try again' })).toBeInTheDocument();
  });

  it('auto-calls reset when the connection comes back', () => {
    renderWithMantine(<Harness initial />);
    expect(reset).not.toHaveBeenCalled();

    act(() => setDisconnected(false));
    expect(reset).toHaveBeenCalledTimes(1);
  });

  // The infinite-loop regression test, and the important one. A genuine
  // Server Component bug renders this page with `disconnected` already
  // false; auto-resetting there would re-throw and loop the boundary.
  it('does NOT auto-call reset on an initial render that is not disconnected', () => {
    renderWithMantine(<Harness initial={false} />);
    expect(reset).not.toHaveBeenCalled();
  });

  it('does not auto-call reset while it stays disconnected', () => {
    renderWithMantine(<Harness initial />);
    act(() => setDisconnected(true));
    act(() => setDisconnected(true));
    expect(reset).not.toHaveBeenCalled();
  });

  it('auto-resets again on a second disconnect/reconnect cycle', () => {
    renderWithMantine(<Harness initial />);
    act(() => setDisconnected(false));
    expect(reset).toHaveBeenCalledTimes(1);

    act(() => setDisconnected(true));
    expect(reset).toHaveBeenCalledTimes(1);

    act(() => setDisconnected(false));
    expect(reset).toHaveBeenCalledTimes(2);
  });
});
