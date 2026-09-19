import { useState } from 'react';
import { describe, it, expect, vi, beforeEach } from 'vitest';
import { act, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { formatDateTime } from '@/lib/dateFormat';
import { ConnectivityMonitor } from './ConnectivityMonitor';

// Only `useNetwork` is stubbed; `useMounted` keeps its real implementation
// so the pre-mount gating below is exercised for real rather than
// simulated.
const network = { online: true };
vi.mock('@mantine/hooks', async (importOriginal) => ({
  ...(await importOriginal<typeof import('@mantine/hooks')>()),
  useNetwork: () => network,
}));

// ConnectivityMonitor now reads the current route (review §2.16, form-page
// copy) via `usePathname()`, which throws "invariant expected app router to
// be mounted" outside a real Next.js App Router tree, same as every other
// component in this app that calls it.
let pathname = '/stations';
vi.mock('next/navigation', () => ({
  usePathname: () => pathname,
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
    pathname = '/stations';
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

  // Review §2.12/§5: "showing the last update" must name an actual time,
  // not remain an unverifiable claim. The banner body should carry the
  // formatted `observedAt` of the last render that was actually reachable,
  // not just the generic fallback copy.
  it('names the last known-good time in the offline banner body', () => {
    const goodAt = '2026-08-19T18:41:00.000Z';
    renderMonitor({ backendReachable: true, observedAt: goodAt });
    expect(screen.queryByText(BANNER)).not.toBeInTheDocument();

    act(() => observe(failure()));
    act(() => observe(failure()));
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent(BANNER);
    expect(status).toHaveTextContent(formatDateTime(goodAt));
  });

  // The one honest edge case: if this render has never once observed a
  // reachable backend, there is no real "last good" time to name, so the
  // banner falls back to the old generic copy rather than fabricating one.
  it('falls back to generic copy when no observation has ever been reachable', () => {
    renderMonitor(failure());
    act(() => observe(failure()));
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent(BANNER);
    expect(status).toHaveTextContent('showing the last update.');
  });

  // Review §2.16: "showing the update from {time}"/"showing the last
  // update" is untrue on a route whose whole content is a form -- there was
  // never any live data on screen to have last updated.
  describe('on a form route', () => {
    it.each(['/track', '/trains', '/track/mine/add-ticket', '/lines/new', '/lines/some-line/edit'])(
      'shows entry-safety copy instead of "showing the last update" on %s',
      (route) => {
        pathname = route;
        renderMonitor(failure());
        act(() => observe(failure()));
        const status = screen.getByRole('status');
        expect(status).toHaveTextContent(BANNER);
        expect(status).toHaveTextContent("Can't reach the server right now — your entries are safe until you submit.");
        expect(status).not.toHaveTextContent('showing the');
      },
    );

    it('still shows entry-safety copy even when a real lastGoodAt is on record', () => {
      // Regression: the form-route branch must win over the
      // lastGoodAt/generic split, not just over the generic fallback --
      // otherwise a visitor who loaded /track right after a good render
      // would still see the misleading "showing the update from {time}."
      pathname = '/track';
      renderMonitor({ backendReachable: true, observedAt: '2026-08-19T18:41:00.000Z' });
      act(() => observe(failure()));
      act(() => observe(failure()));
      const status = screen.getByRole('status');
      expect(status).toHaveTextContent('your entries are safe until you submit.');
    });
  });

  it('keeps the default "showing the last update" copy on a non-form, non-dynamic-segment route that merely starts with a form route\'s name', () => {
    // `/lines/new` is a form route; `/lines/newsletter` (a made-up
    // neighbour) must not be swept in by a loose prefix match.
    pathname = '/lines/newsletter';
    renderMonitor(failure());
    act(() => observe(failure()));
    const status = screen.getByRole('status');
    expect(status).toHaveTextContent('showing the last update.');
  });

  // Review §2.16: on a 390px phone the unconstrained `Notification` shrank
  // to ~200px and its body wrapped onto four lines.
  it('caps the fixed banner wrapper width so it cannot shrink-wrap on a narrow viewport', () => {
    renderMonitor(failure());
    act(() => observe(failure()));
    const status = screen.getByRole('status');
    // The width is set on the fixed-position wrapper `<div>`, one ancestor
    // up from the `Notification` itself (the element carrying `role="status"`).
    const wrapper = status.parentElement;
    expect(wrapper).toHaveStyle({ width: 'min(calc(100vw - 32px), 480px)' });
  });
});
