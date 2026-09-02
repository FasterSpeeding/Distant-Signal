import { useState } from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, fireEvent, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ConnectivityContext } from '@/components/ConnectivityMonitor';
import Error from './error';

const reset = vi.fn();
const error = Object.assign(new globalThis.Error('API request to /Line/Mode failed: 500'), {
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
      <Error error={error} reset={reset} />
    </ConnectivityContext.Provider>
  );
}

describe('app/error.tsx', () => {
  beforeEach(() => {
    reset.mockClear();
  });

  it('still lets the visitor retry by hand', () => {
    renderWithMantine(<Harness initial={false} />);

    fireEvent.click(screen.getByRole('button', { name: 'Try again' }));
    expect(reset).toHaveBeenCalledTimes(1);
  });

  it('shows the raw error message when this is not a connectivity failure', () => {
    renderWithMantine(<Harness initial={false} />);

    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent("Couldn't load status data");
    expect(screen.getByText(error.message)).toBeInTheDocument();
  });

  it('shows reconnecting copy, not the raw error, while disconnected', () => {
    renderWithMantine(<Harness initial />);

    expect(screen.getByRole('heading', { level: 1 })).toHaveTextContent('Trying to reconnect…');
    expect(screen.queryByText(error.message)).not.toBeInTheDocument();
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
