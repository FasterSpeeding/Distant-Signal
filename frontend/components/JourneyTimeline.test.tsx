import { screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { renderWithMantine } from '@/test/render';
import { JourneyTimeline, isGenuineCallingPoint } from './JourneyTimeline';
import type { JourneyStop } from '@/lib/types';

function stop(overrides: Partial<JourneyStop>): JourneyStop {
  return {
    crs: 'RDG',
    name: 'Reading',
    tiploc: null,
    kind: 'Intermediate',
    scheduledArrival: null,
    scheduledDeparture: null,
    actualArrival: null,
    actualDeparture: null,
    estimatedArrival: null,
    estimatedDeparture: null,
    lastEventType: null,
    variationStatus: null,
    delayMinutes: null,
    stopStatus: 'Unknown',
    skipSource: null,
    platform: null,
    plannedPlatform: null,
    platformChanged: false,
    ...overrides,
  };
}

// Real bug report, 2026-09-24: train `L81634` served three `journeyStops`
// with `crs: null, name: null, scheduledArrival: null, scheduledDeparture:
// null` for genuine non-station junctions (`HCRTJN`, `BRLANJN`, `WATRLWC`)
// -- no identity AND no booked time at all, confirmed unresolvable rather
// than a name/CRS join gap. `isGenuineCallingPoint` is the shared predicate
// `TrainJourney.tsx` filters `journeyStops` through, ONCE, before handing
// the result to both `JourneyProgress` and `JourneyTimeline` -- these cases
// pin its exact boundary so neither component needs (or risks
// reimplementing) this logic itself.
describe('isGenuineCallingPoint', () => {
  it('is false for a stop with no identity and no booked time at all (a genuine junction pass)', () => {
    expect(
      isGenuineCallingPoint(
        stop({ crs: null, name: null, scheduledArrival: null, scheduledDeparture: null }),
      ),
    ).toBe(false);
  });

  it('is true for a stop with a real booked time even when its name/CRS never resolved (a join-failure gap, still a real row)', () => {
    expect(
      isGenuineCallingPoint(
        stop({ crs: null, name: null, scheduledArrival: '2026-09-24T08:00:00Z', scheduledDeparture: null }),
      ),
    ).toBe(true);
    expect(
      isGenuineCallingPoint(
        stop({ crs: null, name: null, scheduledArrival: null, scheduledDeparture: '2026-09-24T08:00:00Z' }),
      ),
    ).toBe(true);
  });

  it('is true for a stop with a resolved identity even when it has no booked time', () => {
    expect(
      isGenuineCallingPoint(
        stop({ crs: 'RDG', name: 'Reading', scheduledArrival: null, scheduledDeparture: null }),
      ),
    ).toBe(true);
  });
});

describe('JourneyTimeline', () => {
  it('renders a station name for every stop, in order', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: 'RDG', name: 'Reading', kind: 'Origin' }),
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Terminate' }),
        ]}
      />,
    );
    const names = screen.getAllByText(/Reading|London Waterloo/);
    expect(names[0]).toHaveTextContent('Reading');
    expect(names[1]).toHaveTextContent('London Waterloo');
  });

  it('falls back to the CRS code when no station name is known', () => {
    renderWithMantine(<JourneyTimeline stops={[stop({ crs: 'ZZZ', name: null })]} />);
    expect(screen.getByText('ZZZ')).toBeInTheDocument();
  });

  it('shows only the scheduled time for a stop with no actual time yet', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[stop({ scheduledDeparture: '2026-09-08T08:00:00Z', actualDeparture: null })]}
      />,
    );
    expect(screen.queryByText(/late|early|on time/i)).not.toBeInTheDocument();
  });

  it('shows a late badge for a positive delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledDeparture: '2026-09-08T08:00:00Z',
            actualDeparture: '2026-09-08T08:04:00Z',
            delayMinutes: 4,
          }),
        ]}
      />,
    );
    expect(screen.getByText('4m late')).toBeInTheDocument();
  });

  it('shows an early badge for a negative delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledArrival: '2026-09-08T08:00:00Z',
            actualArrival: '2026-09-08T07:59:00Z',
            delayMinutes: -1,
          }),
        ]}
      />,
    );
    expect(screen.getByText('1m early')).toBeInTheDocument();
  });

  it('shows an on-time badge for a zero delayMinutes', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledDeparture: '2026-09-08T08:00:00Z',
            actualDeparture: '2026-09-08T08:00:00Z',
            delayMinutes: 0,
          }),
        ]}
      />,
    );
    expect(screen.getByText('On time')).toBeInTheDocument();
  });

  it('has a Platform column header when a stop has a known platform', () => {
    renderWithMantine(<JourneyTimeline stops={[stop({ platform: '6' })]} />);
    expect(screen.getByRole('columnheader', { name: 'Platform' })).toBeInTheDocument();
  });

  // 2026-09-22 UX review finding I21/M21/2.10: a column that is empty on
  // every row reads as broken, not as "nothing to report" -- hide it
  // outright rather than rendering an always-blank header cell.
  it('hides the Platform column entirely when no stop has a known platform', () => {
    renderWithMantine(<JourneyTimeline stops={[stop({ platform: null }), stop({ platform: null })]} />);
    expect(screen.queryByRole('columnheader', { name: 'Platform' })).not.toBeInTheDocument();
  });

  it('hides the Delay column entirely when no stop has a delay figure', () => {
    renderWithMantine(<JourneyTimeline stops={[stop({ delayMinutes: null }), stop({ delayMinutes: null })]} />);
    expect(screen.queryByRole('columnheader', { name: 'Delay' })).not.toBeInTheDocument();
  });

  it('shows the Delay column when at least one stop has a delay figure, even if others do not', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ delayMinutes: null }),
          stop({ scheduledDeparture: '2026-09-08T08:00:00Z', actualDeparture: '2026-09-08T08:04:00Z', delayMinutes: 4 }),
        ]}
      />,
    );
    expect(screen.getByRole('columnheader', { name: 'Delay' })).toBeInTheDocument();
  });

  it('shows a platform badge for a stop with a known platform', () => {
    renderWithMantine(
      <JourneyTimeline stops={[stop({ kind: 'Origin', platform: '6', plannedPlatform: '6', platformChanged: false })]} />,
    );
    expect(screen.getByText('Platform 6')).toBeInTheDocument();
  });

  it('shows no platform badge for a stop with no known platform, alongside one that does', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ kind: 'Origin', crs: 'RDG', platform: '6', plannedPlatform: '6', platformChanged: false }),
          stop({ kind: 'Terminate', crs: 'WAT', platform: null }),
        ]}
      />,
    );
    // The "Platform" column itself renders (the origin row has one) -- only
    // the per-stop badge text ("Platform 6", etc.) must be absent for the
    // row with none.
    expect(screen.getByRole('columnheader', { name: 'Platform' })).toBeInTheDocument();
    expect(screen.getByText('Platform 6')).toBeInTheDocument();
    expect(screen.queryAllByText(/Platform \S/)).toHaveLength(1);
  });

  it('names both the current and originally planned platform in text when it has changed', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[stop({ kind: 'Origin', platform: '9', plannedPlatform: '6', platformChanged: true })]}
      />,
    );
    expect(screen.getByText('Platform 9 (changed from 6)')).toBeInTheDocument();
  });

  it('renders as a table with a column for each fact shown, and hides the ones with no data', () => {
    renderWithMantine(<JourneyTimeline stops={[stop({})]} />);
    const table = screen.getByRole('table', { name: 'Journey timeline' });
    expect(table).toBeInTheDocument();
    expect(screen.getByText('Station')).toBeInTheDocument();
    expect(screen.getByText('Scheduled')).toBeInTheDocument();
    // "Actual", not "Actual / est." (2.8) -- the shorter header freed up
    // the width that was pushing the table past 390px on its own.
    expect(screen.getByText('Actual')).toBeInTheDocument();
    expect(screen.queryByText('Actual / est.')).not.toBeInTheDocument();
    // This single stop has neither a delay figure nor a platform, so both
    // columns are hidden entirely (I21/M21/2.10) rather than rendered
    // empty.
    expect(screen.queryByText('Delay')).not.toBeInTheDocument();
    expect(screen.queryByText('Platform')).not.toBeInTheDocument();
  });

  it('shows an estimated time, prefixed and visually distinguished, for a stop with no actual time yet', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledArrival: '2026-09-08T08:00:00Z',
            actualArrival: null,
            estimatedArrival: '2026-09-08T08:05:00Z',
          }),
        ]}
      />,
    );
    const estimate = screen.getByText(/est\. 09:05/);
    expect(estimate).toBeInTheDocument();
    // Italicised/muted -- visually distinct from a confirmed actual time,
    // not just distinguishable by its "est." prefix.
    expect(estimate).toHaveStyle({ fontStyle: 'italic' });
  });

  it('never shows an estimated time alongside a confirmed actual time', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledArrival: '2026-09-08T08:00:00Z',
            actualArrival: '2026-09-08T08:04:00Z',
            estimatedArrival: '2026-09-08T08:05:00Z',
          }),
        ]}
      />,
    );
    expect(screen.queryByText(/^est\./)).not.toBeInTheDocument();
    expect(screen.getByText('09:04')).toBeInTheDocument();
  });

  // The PASS-rendering fix, now keyed off the server-computed
  // `stopStatus` rather than `lastEventType` directly: a booked calling
  // point the train ran through without stopping has no `actual*` time --
  // and, since nothing has confirmed it, `apply_delay_estimates` would
  // otherwise still fill in a forward-looking "est." time for it. That
  // would be actively wrong (the stop is already behind the train, not
  // ahead of it), so it's suppressed.
  it('suppresses the estimated time for a booked stop the train passed without calling', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({
            scheduledArrival: '2026-09-08T08:00:00Z',
            actualArrival: null,
            estimatedArrival: '2026-09-08T08:05:00Z',
            lastEventType: 'PASS',
            stopStatus: 'Skipped',
            skipSource: 'Trust',
          }),
        ]}
      />,
    );
    expect(screen.queryByText(/^est\./)).not.toBeInTheDocument();
  });

  // The "Skipped" treatment itself (Task follow-up to the PASS-rendering
  // fix above): a `'Trust'`-sourced skip -- an INFERENCE from a real-time
  // PASS event, not Darwin's own authoritative flag -- must get the softer
  // wording, never asserted as flatly as a confirmed Darwin skip.
  it('shows a hedged caption for a TRUST-inferred skip', () => {
    renderWithMantine(
      <JourneyTimeline stops={[stop({ crs: 'SLO', name: 'Slough', stopStatus: 'Skipped', skipSource: 'Trust' })]} />,
    );
    expect(screen.getByText('Does not appear to have stopped here')).toBeInTheDocument();
  });

  // Darwin's own explicit per-calling-point flag is treated as
  // authoritative -- confident, unhedged wording.
  it('shows a confident caption for a Darwin-confirmed skip', () => {
    renderWithMantine(
      <JourneyTimeline stops={[stop({ crs: 'SLO', name: 'Slough', stopStatus: 'Skipped', skipSource: 'Darwin' })]} />,
    );
    expect(screen.getByText('Did not stop here')).toBeInTheDocument();
  });

  // Both signals agreeing is at least as confident as Darwin alone -- same
  // wording as the Darwin-only case, not a third, different message.
  it('shows the confident caption when both signals agree', () => {
    renderWithMantine(
      <JourneyTimeline stops={[stop({ crs: 'SLO', name: 'Slough', stopStatus: 'Skipped', skipSource: 'Both' })]} />,
    );
    expect(screen.getByText('Did not stop here')).toBeInTheDocument();
  });

  // `skippedCrs` (§5.2's leg-scoped, live-Darwin-sample-derived signal) is
  // additive to, and independent of, `stopStatus`/`skipSource` above -- it
  // drives its own "Skipped" badge next to the station name, matched
  // case-insensitively (`JourneyStopRow`'s own `isSkippedOnLeg`).
  it('shows a "Skipped" badge for a stop whose CRS is in skippedCrs, matched case-insensitively', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[stop({ crs: 'wok', name: 'Woking', stopStatus: 'Scheduled', skipSource: null })]}
        skippedCrs={['WOK']}
      />,
    );
    expect(screen.getByText('Skipped')).toBeInTheDocument();
  });

  // `skippedCrs` is optional -- every caller outside the journey view
  // (single-train tracking) omits it entirely, and must get no badge at
  // all, not a crash or a badge rendered for every stop.
  it('renders no "Skipped" badge when skippedCrs is omitted', () => {
    renderWithMantine(
      <JourneyTimeline stops={[stop({ crs: 'WOK', name: 'Woking', stopStatus: 'Scheduled', skipSource: null })]} />,
    );
    expect(screen.queryByText('Skipped')).not.toBeInTheDocument();
  });

  // A skipped stop's `delayMinutes` is already `null` by the time it
  // reaches the frontend (`apply_stop_status`'s own documented decision) --
  // this proves the timeline doesn't show a delay badge next to a "did not
  // stop here" caption even so, i.e. that it doesn't need its own separate
  // suppression logic.
  it('shows no delay badge for a skipped stop', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[stop({ crs: 'SLO', name: 'Slough', stopStatus: 'Skipped', skipSource: 'Darwin', delayMinutes: null })]}
      />,
    );
    expect(screen.queryByText(/late|early|on time/i)).not.toBeInTheDocument();
  });

  // The "not skipped" cases must never show a caption at all -- an
  // ordinary future stop (`'Scheduled'`) and a genuinely-called one
  // (`'Called'`) both render exactly as before this feature existed.
  it('shows no skip caption for a scheduled or a called stop', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: 'AAA', name: 'Stop A', stopStatus: 'Scheduled', skipSource: null }),
          stop({ crs: 'BBB', name: 'Stop B', stopStatus: 'Called', skipSource: null }),
        ]}
      />,
    );
    expect(screen.queryByText('Did not stop here')).not.toBeInTheDocument();
    expect(screen.queryByText(/does not appear to have stopped here/i)).not.toBeInTheDocument();
  });

  // Task 3.6.2: a stop whose server-side TIPLOC->CRS->name join didn't
  // resolve gets a by-index placeholder, never the old "Unknown location"
  // string.
  it('falls back to a by-index placeholder ("Stop N") when neither name nor CRS is known', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: 'RDG', name: 'Reading', kind: 'Origin' }),
          stop({ crs: null, name: null, kind: 'Intermediate' }),
          stop({ crs: 'WAT', name: 'London Waterloo', kind: 'Terminate' }),
        ]}
      />,
    );
    expect(screen.getByText('Stop 2')).toBeInTheDocument();
    expect(screen.queryByText('Unknown location')).not.toBeInTheDocument();
  });

  // The first/last row is special-cased: the tracked pin's own
  // origin/destination is always known, even when this particular stop's
  // own name/CRS didn't resolve, so it seeds the label instead of falling
  // through to "Stop 1"/"Stop N".
  it('seeds the first/last row from the tracked pin origin/destination when the stop itself has no name or CRS', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: null, name: null, kind: 'Origin' }),
          stop({ crs: 'RDG', name: 'Reading', kind: 'Intermediate' }),
          stop({ crs: null, name: null, kind: 'Terminate' }),
        ]}
        endpointNames={{ originName: 'London Waterloo', destinationName: 'Woking' }}
      />,
    );
    expect(screen.getByText('London Waterloo')).toBeInTheDocument();
    expect(screen.getByText('Woking')).toBeInTheDocument();
    expect(screen.queryByText('Stop 1')).not.toBeInTheDocument();
    expect(screen.queryByText('Stop 3')).not.toBeInTheDocument();
  });

  // Genuinely degenerate case: nothing at all resolved, not even the pin's
  // own origin/destination -- N rows of "Stop 1"/"Stop 2"/... would look
  // like a real (if terse) timetable, so this collapses to one honest,
  // dimmed line instead and renders no table at all.
  it('collapses to a single dimmed line when every stop is unnamed, with no table rendered', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: null, name: null, kind: 'Origin' }),
          stop({ crs: null, name: null, kind: 'Intermediate' }),
          stop({ crs: null, name: null, kind: 'Terminate' }),
        ]}
      />,
    );
    expect(screen.getByText('3 stops — station names unavailable')).toBeInTheDocument();
    expect(screen.queryByRole('table')).not.toBeInTheDocument();
    expect(screen.queryByText(/Stop \d/)).not.toBeInTheDocument();
  });

  it('does not collapse when only some stops are unnamed', () => {
    renderWithMantine(
      <JourneyTimeline
        stops={[
          stop({ crs: 'RDG', name: 'Reading', kind: 'Origin' }),
          stop({ crs: null, name: null, kind: 'Intermediate' }),
        ]}
      />,
    );
    expect(screen.getByRole('table')).toBeInTheDocument();
    expect(screen.queryByText(/station names unavailable/)).not.toBeInTheDocument();
  });

  // 2026-09-22 UX review finding I16/2.7: "the traveller gets off at York,
  // which is a plain-weight row indistinguishable from Peterborough."
  describe('legDestinationCrs', () => {
    it('bolds the leg destination row and marks it "You get off here", even for a plain intermediate stop', () => {
      renderWithMantine(
        <JourneyTimeline
          stops={[
            stop({ crs: 'KGX', name: 'London Kings Cross', kind: 'Origin' }),
            stop({ crs: 'YRK', name: 'York', kind: 'Intermediate' }),
            stop({ crs: 'NCL', name: 'Newcastle', kind: 'Intermediate' }),
            stop({ crs: 'EDB', name: 'Edinburgh', kind: 'Terminate' }),
          ]}
          legDestinationCrs="YRK"
        />,
      );
      expect(screen.getByText('You get off here')).toBeInTheDocument();
      expect(screen.getByText('York')).toHaveStyle({ fontWeight: '700' });
    });

    it('dims every row after the leg destination, but not the destination row itself or earlier rows', () => {
      // Every stop given an `actualDeparture`/`actualArrival` (`reached`)
      // so the ONLY thing dimming a row here is `isPastLegDestination` --
      // isolated from the pre-existing "not yet reached" dimming, which
      // would otherwise dim NCL/EDB for an unrelated reason and give this
      // test a false pass.
      renderWithMantine(
        <JourneyTimeline
          stops={[
            stop({ crs: 'KGX', name: 'London Kings Cross', kind: 'Origin', actualDeparture: '2026-09-22T16:00:00Z' }),
            stop({ crs: 'YRK', name: 'York', kind: 'Intermediate', actualArrival: '2026-09-22T18:00:00Z' }),
            stop({ crs: 'NCL', name: 'Newcastle', kind: 'Intermediate', actualArrival: '2026-09-22T19:00:00Z' }),
            stop({ crs: 'EDB', name: 'Edinburgh', kind: 'Terminate', actualArrival: '2026-09-22T20:00:00Z' }),
          ]}
          legDestinationCrs="YRK"
        />,
      );
      const origin = screen.getByText('London Kings Cross');
      const destination = screen.getByText('York');
      const pastFirst = screen.getByText('Newcastle');
      const pastLast = screen.getByText('Edinburgh');
      expect(origin).not.toHaveStyle({ color: 'var(--mantine-color-dimmed)' });
      expect(destination).not.toHaveStyle({ color: 'var(--mantine-color-dimmed)' });
      expect(pastFirst).toHaveStyle({ color: 'var(--mantine-color-dimmed)' });
      expect(pastLast).toHaveStyle({ color: 'var(--mantine-color-dimmed)' });
    });

    it('renders no marker and dims nothing extra when legDestinationCrs is omitted', () => {
      renderWithMantine(
        <JourneyTimeline
          stops={[
            stop({ crs: 'KGX', name: 'London Kings Cross', kind: 'Origin', actualDeparture: '2026-09-22T16:00:00Z' }),
            stop({ crs: 'YRK', name: 'York', kind: 'Intermediate', actualArrival: '2026-09-22T18:00:00Z' }),
          ]}
        />,
      );
      expect(screen.queryByText('You get off here')).not.toBeInTheDocument();
      expect(screen.getByText('York')).not.toHaveStyle({ color: 'var(--mantine-color-dimmed)' });
    });

    it('is a no-op when legDestinationCrs matches no stop in the list', () => {
      renderWithMantine(
        <JourneyTimeline
          stops={[
            stop({ crs: 'KGX', name: 'London Kings Cross', kind: 'Origin' }),
            stop({ crs: 'YRK', name: 'York', kind: 'Terminate' }),
          ]}
          legDestinationCrs="ZZZ"
        />,
      );
      expect(screen.queryByText('You get off here')).not.toBeInTheDocument();
    });
  });
});
