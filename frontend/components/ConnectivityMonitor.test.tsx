import { useState } from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { ConnectivityMonitor } from './ConnectivityMonitor';

// Only `useNetwork` is stubbed; `useMounted` keeps its real implementation
// so the pre-mount gating below is exercised for real rather than
// simulated.
const network = { online: true };
vi.mock('@mantine/hooks', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@mantine/hooks')>()),
  useNetwork: () => network,
}));

const BANNER = 'Reconnecting…';

type Observation = { backendReachable: boolean; observedAt: string };

// Deliberately NOT @testing-library's `rerender`: `renderWithMantine`
// wraps its argument in a MantineProvider, but `rerender` replaces the
// whole tree with the bare element, dropping that provider -- which throws
// inside `Notification` and silently *remounts* the subject rather than
// re-rendering it. Driving a parent's state instead keeps the provider and
// the component instance intact, and models the real thing more closely:
// each observation is one RootLayout server render.
let observe: (next: Observation) => void = () => {};

function Harness({ first }: { first: Observation }) {
  const [observation, setObservation] = useState(first);
  observe = setObservation;
  return (
    <ConnectivityMonitor
      backendReachable={observation.backendReachable}
      observedAt={observation.observedAt}
    >
      <p>page content</p>
    </ConnectivityMonitor>
  );
}

// RootLayout passes `new Date().toISOString()`, a fresh value per server
// render. Each call here stands for one such render.
let observation = 0;
const failure = (): Observation => ({ backendReachable: false, observedAt: `obs-${(observation += 1)}` });
const success = (): Observation => ({ backendReachable: true, observedAt: `obs-${(observation += 1)}` });

function renderMonitor(first: Observation) {
  return renderWithMantine(<Harness first={first} />);
}

describe('ConnectivityMonitor', () => {
  beforeEach(() => {
    network.online = true;
    observation = 0;
  });

  it('always renders its children, banner or not', () => {
    renderMonitor(success());
    expect(screen.getByText('page content')).toBeInTheDocument();
  });

  it('shows no banner while the backend is reachable and the device is online', () => {
    renderMonitor(success());
    expect(screen.queryByText(BANNER)).not.toBeInTheDocument();
  });

  // The two-strikes regression test (design spec Decision 2): one failed
  // freshness fetch is a blip and must not flash a banner.
  it('shows no banner after a single backend failure', () => {
    renderMonitor(failure());
    expect(screen.queryByText(BANNER)).not.toBeInTheDocument();
  });

  // The regression test for the bug this implementation had to fix: during
  // a real outage the server re-renders with the *same* `false` every 30s.
  // An effect keyed on `backendReachable` would never re-run, the counter
  // would stick at 1 and the banner would never appear at all. Each
  // distinct `observedAt` stands for one such server render.
  it('trips on the second consecutive failure even though the boolean never changes', () => {
    renderMonitor(failure());
    expect(screen.queryByText(BANNER)).not.toBeInTheDocument();

    act(() => observe(failure()));
    expect(screen.getByText(BANNER)).toBeInTheDocument();
  });

  it('clears the banner on the first success, and needs two fresh failures to trip again', () => {
    renderMonitor(failure());
    act(() => observe(failure()));
    expect(screen.getByText(BANNER)).toBeInTheDocument();

    // One success clears it immediately (design spec Decision 2).
    act(() => observe(success()));
    expect(screen.queryByText(BANNER)).not.toBeInTheDocument();

    // ...and the counter really reset, so one later failure is not enough.
    act(() => observe(failure()));
    expect(screen.queryByText(BANNER)).not.toBeInTheDocument();

    act(() => observe(failure()));
    expect(screen.getByText(BANNER)).toBeInTheDocument();
  });

  // No debounce on the browser's own offline signal (design spec Decision
  // 2): it is a local fact, not a round-trip that can blip.
  it('shows the banner immediately when the device goes offline, with no two-strikes delay', () => {
    network.online = false;
    renderMonitor(success());
    expect(screen.getByText(BANNER)).toBeInTheDocument();
  });

  it('clears an offline banner as soon as the device comes back online', () => {
    network.online = false;
    renderMonitor(success());
    expect(screen.getByText(BANNER)).toBeInTheDocument();

    network.online = true;
    act(() => observe(success()));
    expect(screen.queryByText(BANNER)).not.toBeInTheDocument();
  });

  it('announces the banner politely to assistive technology', () => {
    network.online = false;
    renderMonitor(success());
    const status = screen.getByRole('status');
    expect(status).toHaveAttribute('aria-live', 'polite');
    expect(status).toHaveTextContent(BANNER);
  });
});
