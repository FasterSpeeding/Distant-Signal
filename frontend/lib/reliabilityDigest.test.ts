import { describe, it, expect } from 'vitest';
import { isEligibleForPunctuality, computePunctualitySummary } from './reliabilityDigest';
import type { TrackedTrainListItem } from './types';

const TODAY = '2026-09-12';

function train(overrides: Partial<TrackedTrainListItem> = {}): TrackedTrainListItem {
  return {
    id: 1,
    serviceDate: '2026-09-10',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinOriginName: 'London Kings Cross',
    pinDestinationName: 'York',
    pinScheduledDeparture: '2026-09-10T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'completed',
    delayMinutes: 3,
    trackedAt: '2026-09-09T12:00:00Z',
    customName: null,
    sharedGroupCount: 0,
    ...overrides,
  };
}

describe('isEligibleForPunctuality', () => {
  it('excludes pending', () => {
    expect(isEligibleForPunctuality(train({ resolutionStatus: 'pending', delayMinutes: null }), TODAY)).toBe(false);
  });

  it('excludes schedule_matched', () => {
    expect(
      isEligibleForPunctuality(train({ resolutionStatus: 'schedule_matched', delayMinutes: null }), TODAY),
    ).toBe(false);
  });

  it('excludes unresolved', () => {
    expect(isEligibleForPunctuality(train({ resolutionStatus: 'unresolved', delayMinutes: null }), TODAY)).toBe(
      false,
    );
  });

  it('excludes resolved with null delayMinutes', () => {
    expect(isEligibleForPunctuality(train({ resolutionStatus: 'resolved', delayMinutes: null }), TODAY)).toBe(
      false,
    );
  });

  it('excludes a resolved row whose serviceDate is today', () => {
    expect(isEligibleForPunctuality(train({ serviceDate: TODAY }), TODAY)).toBe(false);
  });

  it('excludes a resolved row whose serviceDate is in the future', () => {
    expect(isEligibleForPunctuality(train({ serviceDate: '2026-09-13' }), TODAY)).toBe(false);
  });

  it('includes a resolved, non-null-delay row strictly before today', () => {
    expect(isEligibleForPunctuality(train({ serviceDate: '2026-09-11' }), TODAY)).toBe(true);
  });
});

describe('computePunctualitySummary', () => {
  it('returns an explicit no-data shape when nothing is eligible', () => {
    const summary = computePunctualitySummary([train({ resolutionStatus: 'pending', delayMinutes: null })], TODAY);
    expect(summary).toEqual({
      eligibleCount: 0,
      onTimePct: null,
      avgDelayMinutes: null,
      cancelledCount: 0,
      worstJourneys: [],
    });
  });

  it('on-time uses delayMinutes <= 0, matching RowStatusBadge, not a 5-minute threshold', () => {
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 0 }),
        train({ id: 2, serviceDate: '2026-09-10', delayMinutes: -2 }),
        train({ id: 3, serviceDate: '2026-09-10', delayMinutes: 1 }),
        train({ id: 4, serviceDate: '2026-09-10', delayMinutes: 4 }),
      ],
      TODAY,
    );
    expect(summary.eligibleCount).toBe(4);
    expect(summary.onTimePct).toBe(50); // 2 of 4 have delayMinutes <= 0
  });

  it('excludes cancelled journeys from avgDelayMinutes/onTimePct and counts them separately', () => {
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 0, status: 'completed' }),
        train({ id: 2, serviceDate: '2026-09-10', delayMinutes: 90, status: 'cancelled' }),
      ],
      TODAY,
    );
    expect(summary.eligibleCount).toBe(1);
    expect(summary.onTimePct).toBe(100);
    expect(summary.avgDelayMinutes).toBe(0);
    expect(summary.cancelledCount).toBe(1);
  });

  it('worstJourneys returns at most 5, sorted descending by delayMinutes, ties broken by most recent serviceDate', () => {
    const trains = [
      train({ id: 1, serviceDate: '2026-09-01', delayMinutes: 10 }),
      train({ id: 2, serviceDate: '2026-09-05', delayMinutes: 30 }),
      train({ id: 3, serviceDate: '2026-09-02', delayMinutes: 30 }), // same delay as id 2, older date
      train({ id: 4, serviceDate: '2026-09-06', delayMinutes: 5 }),
      train({ id: 5, serviceDate: '2026-09-07', delayMinutes: 45 }),
      train({ id: 6, serviceDate: '2026-09-08', delayMinutes: 2 }),
      train({ id: 7, serviceDate: '2026-09-09', delayMinutes: 1 }),
    ];
    const summary = computePunctualitySummary(trains, TODAY);
    expect(summary.worstJourneys).toHaveLength(5);
    expect(summary.worstJourneys.map((j) => j.trainId)).toEqual([5, 2, 3, 1, 4]);
  });

  it('a cancelled journey with a leftover delayMinutes can still appear in worstJourneys context but never skews the average', () => {
    // Guard against a regression where cancelled rows get silently folded
    // into the delay/on-time arithmetic via worstJourneys' own sort.
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 5, status: 'completed' }),
        train({ id: 2, serviceDate: '2026-09-10', delayMinutes: 200, status: 'cancelled' }),
      ],
      TODAY,
    );
    expect(summary.avgDelayMinutes).toBe(5);
    expect(summary.cancelledCount).toBe(1);
  });
});
