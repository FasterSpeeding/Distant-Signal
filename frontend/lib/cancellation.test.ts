import { describe, expect, it } from 'vitest';
import {
  boardCancellationNote,
  cancellationHeadline,
  cancellationReasonPhrase,
  cancellationReasonSource,
  cancellationSummary,
  cancelledSectionLines,
  trainCancellation,
} from './cancellation';
import type { Cancellation, CancellationStop } from './types';

function stop(name: string, stopIndex: number): CancellationStop {
  return { crs: name.slice(0, 3).toUpperCase(), name, stopIndex };
}

const ORIGIN = stop('Chessington South', 0);
const RAYNES = stop('Raynes Park', 5);
const WIMBLEDON = stop('Wimbledon', 6);
const WATERLOO = stop('London Waterloo', 11);

function cancellation(overrides: Partial<Cancellation>): Cancellation {
  return {
    kind: 'partial',
    startsFrom: null,
    terminatesAt: null,
    from: null,
    to: null,
    sections: [],
    reason: null,
    at: null,
    ...overrides,
  };
}

describe('cancellationHeadline', () => {
  it('says a wholly cancelled train is cancelled', () => {
    expect(cancellationHeadline(cancellation({ kind: 'full', sections: [{ from: ORIGIN, to: WATERLOO }] }))).toBe(
      'Cancelled',
    );
  });

  it('names where a terminated-short train ends', () => {
    const c = cancellation({ terminatesAt: RAYNES, sections: [{ from: RAYNES, to: WATERLOO }] });
    expect(cancellationHeadline(c)).toBe('Terminates at Raynes Park');
    expect(cancelledSectionLines(c)).toEqual(['Cancelled between Raynes Park and London Waterloo']);
  });

  it('names where a started-short train starts', () => {
    expect(cancellationHeadline(cancellation({ startsFrom: RAYNES, sections: [{ from: ORIGIN, to: RAYNES }] }))).toBe(
      'Starts from Raynes Park',
    );
  });

  it('names both ends of a train cut at both ends', () => {
    const c = cancellation({
      startsFrom: RAYNES,
      terminatesAt: WIMBLEDON,
      sections: [
        { from: ORIGIN, to: RAYNES },
        { from: WIMBLEDON, to: WATERLOO },
      ],
    });
    expect(cancellationHeadline(c)).toBe('Starts from Raynes Park and terminates at Wimbledon');
  });

  it('names a section cancelled between two stations', () => {
    expect(cancellationHeadline(cancellation({ sections: [{ from: RAYNES, to: WATERLOO }] }))).toBe(
      'Cancelled between Raynes Park and London Waterloo',
    );
  });

  it('says part cancelled for several middle sections', () => {
    const c = cancellation({
      sections: [
        { from: ORIGIN, to: RAYNES },
        { from: WIMBLEDON, to: WATERLOO },
      ],
    });
    expect(cancellationHeadline(c)).toBe('Part cancelled');
    expect(cancelledSectionLines(c)).toHaveLength(2);
  });

  it('falls back to the code for an unnamed stop', () => {
    expect(
      cancellationHeadline(
        cancellation({
          terminatesAt: { crs: 'RAY', name: null, stopIndex: 5 },
          sections: [{ from: RAYNES, to: WATERLOO }],
        }),
      ),
    ).toBe('Terminates at RAY');
  });
});

describe('cancellation reasons', () => {
  it('drops the board sentence opening so it follows "Cancelled:"', () => {
    const c = cancellation({
      kind: 'full',
      reason: {
        code: null,
        text: 'This train has been cancelled because of a fault with the signalling system.',
        source: 'ldbws',
      },
    });
    expect(cancellationSummary(c)).toBe('Cancelled: a fault with the signalling system');
    expect(cancellationReasonSource(c.reason)).toBe('Reason from National Rail Enquiries');
  });

  it('lower-cases a glossary description, but not an acronym', () => {
    expect(cancellationReasonPhrase({ code: 'TG', text: 'Driver', source: 'trust' })).toBe('driver');
    expect(cancellationReasonPhrase({ code: 'XX', text: 'TOC staff', source: 'trust' })).toBe('TOC staff');
  });

  it('puts the reason after a partial headline', () => {
    const c = cancellation({
      terminatesAt: RAYNES,
      sections: [{ from: RAYNES, to: WATERLOO }],
      reason: {
        code: 'JP',
        text: 'Failure to maintain vegetation within network boundaries',
        source: 'trust',
      },
    });
    expect(cancellationSummary(c)).toBe(
      'Terminates at Raynes Park: failure to maintain vegetation within network boundaries',
    );
    expect(cancellationReasonSource(c.reason)).toBe('Reason recorded by Network Rail');
  });

  it('has nothing to add for a code with no text', () => {
    const c = cancellation({ kind: 'full', reason: { code: 'PD', text: null, source: 'trust' } });
    expect(cancellationSummary(c)).toBe('Cancelled');
    expect(cancellationReasonSource(c.reason)).toBeNull();
  });
});

describe('trainCancellation', () => {
  it("prefers the api's own cancellation", () => {
    const own = cancellation({ terminatesAt: RAYNES, sections: [{ from: RAYNES, to: WATERLOO }] });
    expect(trainCancellation({ status: 'cancelled', cancellation: own })).toBe(own);
  });

  it('reads an older backend: a cancelled status is the whole train, with its reason', () => {
    const c = trainCancellation({ status: 'cancelled', cancelReasonCode: 'TG', cancelReason: 'Driver' });
    expect(c?.kind).toBe('full');
    expect(c?.reason).toEqual({ code: 'TG', text: 'Driver', source: 'trust' });
  });

  it('is null for a running or reinstated train', () => {
    expect(trainCancellation({ status: 'en_route', cancellation: null })).toBeNull();
    expect(trainCancellation({ status: 'en_route' })).toBeNull();
  });
});

describe('boardCancellationNote', () => {
  const points = (cancelled: boolean[]) =>
    ['Surbiton', 'Wimbledon', 'Clapham Junction', 'London Waterloo'].map((locationName, i) => ({
      locationName,
      crs: locationName.slice(0, 3).toUpperCase(),
      isCancelled: cancelled[i] ?? false,
    }));

  it("adds a cancelled row's reason", () => {
    expect(
      boardCancellationNote({
        isCancelled: true,
        cancelReason: 'This train has been cancelled because of a broken down train',
      }),
    ).toBe('Cancelled: a broken down train');
    expect(boardCancellationNote({ isCancelled: true, cancelReason: null })).toBeNull();
  });

  it('says where a running train now terminates or which section it misses', () => {
    expect(
      boardCancellationNote({
        isCancelled: false,
        cancelReason: null,
        callingPoints: points([false, false, true, true]),
      }),
    ).toBe('Terminates at Wimbledon');
    expect(
      boardCancellationNote({
        isCancelled: false,
        cancelReason: null,
        callingPoints: points([false, true, false, false]),
      }),
    ).toBe('Cancelled between Surbiton and Clapham Junction');
    expect(
      boardCancellationNote({
        isCancelled: false,
        cancelReason: null,
        callingPoints: points([false, false, false, false]),
      }),
    ).toBeNull();
  });
});
