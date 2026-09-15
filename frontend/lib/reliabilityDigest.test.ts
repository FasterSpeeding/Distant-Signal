import { describe, it, expect } from 'vitest';
import { isEligibleForPunctuality, computePunctualitySummary, computeDelayRepayRollup } from './reliabilityDigest';
import type { TrackedTrainListItem, TicketListItem } from './types';

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

  it('every eligible journey cancelled: eligibleCount is 0 but cancelledCount reports the real count (review finding A)', () => {
    // computePunctualitySummary's own contract (eligibleCount is the
    // non-cancelled arithmetic population) means a caller must check
    // cancelledCount separately to tell "no data at all" apart from
    // "everything eligible was cancelled" -- this test locks in the shape
    // ReliabilityDigest's PunctualitySection now branches on.
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 45, status: 'cancelled' }),
        train({ id: 2, serviceDate: '2026-09-09', delayMinutes: 10, status: 'cancelled' }),
      ],
      TODAY,
    );
    expect(summary.eligibleCount).toBe(0);
    expect(summary.onTimePct).toBeNull();
    expect(summary.avgDelayMinutes).toBeNull();
    expect(summary.cancelledCount).toBe(2);
  });

  it('worstJourneys excludes on-time and early journeys (delayMinutes <= 0) -- review finding B', () => {
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 0 }),
        train({ id: 2, serviceDate: '2026-09-09', delayMinutes: -5 }),
        train({ id: 3, serviceDate: '2026-09-08', delayMinutes: 12 }),
      ],
      TODAY,
    );
    expect(summary.worstJourneys.map((j) => j.trainId)).toEqual([3]);
  });

  it('worstJourneys is empty (not padded with on-time rows) when nothing eligible is actually late', () => {
    const summary = computePunctualitySummary(
      [
        train({ id: 1, serviceDate: '2026-09-10', delayMinutes: 0 }),
        train({ id: 2, serviceDate: '2026-09-09', delayMinutes: -2 }),
      ],
      TODAY,
    );
    expect(summary.eligibleCount).toBe(2);
    expect(summary.worstJourneys).toEqual([]);
  });
});

function ticket(overrides: Partial<TicketListItem> = {}): TicketListItem {
  return {
    id: 1,
    trackedTrainId: 1,
    operator: 'LNER',
    ticketType: null,
    originCrs: 'KGX',
    destinationCrs: 'YRK',
    originName: null,
    destinationName: null,
    source: 'manual',
    createdAt: '2026-09-09T12:00:00Z',
    serviceDate: '2026-09-10',
    pinOriginCrs: 'KGX',
    pinDestinationCrs: 'YRK',
    pinScheduledDeparture: '2026-09-10T09:00:00Z',
    resolutionStatus: 'resolved',
    trainUid: 'W12345',
    status: 'completed',
    delayMinutes: 35,
    estimate: { scheme: 'DR30', bandMinutes: 30, percentage: 50, disclaimer: 'estimate disclaimer' },
    claimUrl: 'https://delayrepay.lner.co.uk/delayrepayV2/',
    disclaimer: 'route disclaimer',
    customName: null,
    ...overrides,
  };
}

describe('computeDelayRepayRollup', () => {
  it('a standalone ticket (trackedTrainId: null) is excluded from both numerator and denominator', () => {
    const rollup = computeDelayRepayRollup([ticket({ trackedTrainId: null, estimate: null })]);
    expect(rollup.attachedTicketsWithOperator).toBe(0);
    expect(rollup.eligibleCount).toBe(0);
  });

  it('an attached ticket with operator: null is excluded from the denominator, not counted as ineligible', () => {
    const rollup = computeDelayRepayRollup([ticket({ operator: null, estimate: null })]);
    expect(rollup.attachedTicketsWithOperator).toBe(0);
    expect(rollup.eligibleCount).toBe(0);
  });

  it('an attached ticket with a non-null operator but a null estimate counts toward the denominator only', () => {
    const rollup = computeDelayRepayRollup([ticket({ estimate: null })]);
    expect(rollup.attachedTicketsWithOperator).toBe(1);
    expect(rollup.eligibleCount).toBe(0);
  });

  it('bandCounts tallies one ticket in each of the three known bands', () => {
    const rollup = computeDelayRepayRollup([
      ticket({ id: 1, estimate: { scheme: 'DR15', bandMinutes: 15, percentage: 25, disclaimer: 'd' } }),
      ticket({ id: 2, estimate: { scheme: 'DR15', bandMinutes: 30, percentage: 50, disclaimer: 'd' } }),
      ticket({ id: 3, estimate: { scheme: 'DR30', bandMinutes: 60, percentage: 100, disclaimer: 'd' } }),
    ]);
    expect(rollup.attachedTicketsWithOperator).toBe(3);
    expect(rollup.eligibleCount).toBe(3);
    expect(rollup.bandCounts).toEqual({
      'DR15-15': 1,
      'DR15-30': 1,
      'DR30-60': 1,
    });
  });

  it('never produces a currency/percentage total field of any kind', () => {
    const rollup = computeDelayRepayRollup([ticket()]);
    expect(rollup).not.toHaveProperty('totalPercentage');
    expect(rollup).not.toHaveProperty('averagePercentage');
    expect(rollup).not.toHaveProperty('estimatedTotal');
    expect(rollup).not.toHaveProperty('total');
    expect(Object.keys(rollup).sort()).toEqual(['attachedTicketsWithOperator', 'bandCounts', 'eligibleCount']);
  });
});
