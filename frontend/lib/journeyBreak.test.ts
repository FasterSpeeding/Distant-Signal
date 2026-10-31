import { describe, expect, it } from 'vitest';
import {
  breakBody,
  breakTitle,
  changesLabel,
  fateBadge,
  missableLegId,
  openBreakTitleFacts,
  revisionLegLine,
  revisionTitle,
  suggestionBadges,
  suggestionDelay,
  suggestionLegBody,
  suggestionLegLines,
  suggestionSetBody,
  suggestionSetTitleFacts,
  suggestionSummary,
  type BreakTitleFacts,
} from './journeyBreak';
import { trainLeg, untrackedLeg } from '@/test/journeyFixtures';
import { replanSuggestion, suggestionSet, trainSuggestionLeg, openBreakSummary } from '@/test/replanFixtures';
import type { RevisionLeg } from './types';

const facts: BreakTitleFacts = {
  cause: 'cancelled',
  // 09:52 UK time (BST).
  departure: '2026-09-17T08:52:00Z',
  trainDestinationName: 'Edinburgh',
  legDestinationName: 'York',
  stationName: 'York',
};

describe('breakTitle', () => {
  it('words each cause', () => {
    expect(breakTitle(facts)).toBe('09:52 to Edinburgh cancelled');
    expect(breakTitle({ ...facts, cause: 'connection_missed' })).toBe('Connection at York missed');
    expect(breakTitle({ ...facts, cause: 'user_missed' })).toBe('You missed the 09:52 to Edinburgh');
    expect(breakTitle({ ...facts, cause: 'part_cancelled', stationName: 'Durham' })).toBe(
      '09:52 to Edinburgh no longer stops at Durham',
    );
    expect(breakTitle({ ...facts, cause: 'skipped_stop', stationName: 'Durham' })).toBe(
      '09:52 to Edinburgh no longer stops at Durham',
    );
  });

  it('falls back to fewer facts', () => {
    const none: BreakTitleFacts = {
      cause: 'cancelled',
      departure: null,
      trainDestinationName: null,
      legDestinationName: null,
      stationName: null,
    };
    expect(breakTitle(none)).toBe('Your train cancelled');
    expect(breakTitle({ ...none, departure: facts.departure })).toBe('09:52 train cancelled');
    expect(breakTitle({ ...none, cause: 'connection_missed' })).toBe('Connection missed');
    expect(breakTitle({ ...none, cause: 'user_missed' })).toBe('You missed your train');
    expect(breakTitle({ ...none, cause: 'user_missed', departure: facts.departure })).toBe(
      'You missed the 09:52 train',
    );
    expect(breakTitle({ ...none, cause: 'skipped_stop' })).toBe('Your train no longer stops where you need it');
    expect(breakTitle({ ...none, cause: 'part_cancelled', stationName: 'Durham' })).toBe(
      'Your train no longer stops at Durham',
    );
  });

  it('uses the leg destination when the train destination is unknown', () => {
    expect(breakTitle({ ...facts, trainDestinationName: null, cause: 'user_missed' })).toBe(
      'You missed the 09:52 to York',
    );
  });

  it('names a free choice by where it goes', () => {
    expect(breakTitle({ ...facts, cause: null, legDestinationName: 'Edinburgh' })).toBe('Other ways to Edinburgh');
    expect(breakTitle({ ...facts, cause: null, legDestinationName: null })).toBe('Other trains');
  });
});

describe('openBreakTitleFacts', () => {
  it('reads the trigger leg', () => {
    const leg = trainLeg(
      7,
      ['KGX', 'London Kings Cross'],
      ['YRK', 'York'],
      '2026-09-17T08:52:00Z',
      '2026-09-17T11:00:00Z',
      {
        scheduleDestinationName: 'Edinburgh',
      },
    );
    const result = openBreakTitleFacts(openBreakSummary({ triggerLegId: 7, cause: 'cancelled' }), [leg]);
    expect(breakTitle(result)).toBe('09:52 to Edinburgh cancelled');
  });

  it('copes with no trigger leg', () => {
    const result = openBreakTitleFacts(openBreakSummary({ triggerLegId: null, cause: 'connection_missed' }), []);
    expect(breakTitle(result)).toBe('Connection at York missed');
  });
});

describe('suggestionSetTitleFacts', () => {
  it('takes the station of a missed connection from the trigger', () => {
    const set = suggestionSet();
    expect(breakTitle(suggestionSetTitleFacts(set, 'connection_missed', null))).toBe('Connection at York missed');
    expect(breakTitle(suggestionSetTitleFacts(set, 'user_missed', null))).toBe('You missed the 09:52 to Edinburgh');
    expect(breakTitle(suggestionSetTitleFacts(set, null, null))).toBe('Other ways to Edinburgh');
  });
});

describe('breakBody', () => {
  it('counts the ways and names the first arrival', () => {
    expect(breakBody(3, 'Edinburgh', '2026-09-17T10:12:00Z')).toBe('3 other ways to Edinburgh. First arrives 11:12.');
    expect(breakBody(1, 'Edinburgh', null)).toBe('1 other way to Edinburgh.');
    expect(breakBody(2, null, null)).toBe('2 other ways.');
  });

  it('says when there are none', () => {
    expect(breakBody(0, 'Edinburgh', null)).toBe('No more trains to Edinburgh today.');
    expect(breakBody(0, null, null)).toBe('No more trains today.');
  });

  it('says when none are planned yet', () => {
    expect(breakBody(0, 'Edinburgh', null, false)).toBe('Looking for other trains.');
  });

  it('reads a suggestion set', () => {
    expect(suggestionSetBody(suggestionSet())).toBe('3 other ways to Edinburgh. First arrives 12:10.');
  });
});

describe('suggestion rows', () => {
  it('summarises a suggestion', () => {
    expect(suggestionSummary(replanSuggestion())).toBe('10:04 York to Edinburgh, direct');
    const changing = replanSuggestion({
      changeCount: 1,
      legs: [
        trainSuggestionLeg({ destinationName: 'Newcastle', destinationCrs: 'NCL' }),
        trainSuggestionLeg({ originName: 'Newcastle', originCrs: 'NCL', departure: '2026-09-17T10:30:00Z' }),
      ],
    });
    expect(suggestionSummary(changing)).toBe('10:04 York to Edinburgh, 1 change');
    expect(suggestionLegLines(changing)).toEqual([
      '10:04 York to Newcastle (LNER)',
      '11:30 Newcastle to Edinburgh (LNER)',
    ]);
  });

  it('falls back to the set ends with no legs', () => {
    expect(
      suggestionSummary(replanSuggestion({ legs: [] }), { originName: 'York', destinationName: 'Edinburgh' }),
    ).toBe('10:04 York to Edinburgh, direct');
  });

  it('words changes', () => {
    expect(changesLabel(0)).toBe('direct');
    expect(changesLabel(1)).toBe('1 change');
    expect(changesLabel(2)).toBe('2 changes');
  });

  it('words an untracked leg', () => {
    const walk = replanSuggestion({
      legs: [
        {
          kind: 'untracked',
          transitMode: 'WALK',
          originName: 'Euston',
          destinationName: "King's Cross",
          lineName: null,
          originStopId: null,
          destinationStopId: null,
          plannedDeparture: '2026-09-17T09:00:00Z',
          plannedArrival: '2026-09-17T09:15:00Z',
        },
      ],
    });
    expect(suggestionLegLines(walk)).toEqual(["Walk, Euston to King's Cross"]);
  });
});

describe('suggestionDelay', () => {
  it('is orange until an hour, then red', () => {
    expect(suggestionDelay(12)).toEqual({
      text: '+12 min',
      spoken: 'Arrives 12 min after your booked time',
      tone: 'late',
    });
    expect(suggestionDelay(59)?.tone).toBe('late');
    expect(suggestionDelay(60)?.tone).toBe('critical');
  });

  it('says on time and early', () => {
    expect(suggestionDelay(0)?.text).toBe('On time');
    expect(suggestionDelay(-3)).toMatchObject({ text: '−3 min', tone: 'early' });
    expect(suggestionDelay(null)).toBeNull();
  });
});

describe('suggestionBadges', () => {
  it('puts the picks first, then the fit, at most three', () => {
    const badges = suggestionBadges(
      replanSuggestion({
        pickLabels: ['Recommended', 'Fastest', 'Same operator'],
        fit: 'likelyNewTicket',
        fitLabel: 'May need a new ticket',
        fitReason: 'Your Advance ticket is for the 09:52 only.',
      }),
    );
    expect(badges).toEqual([
      { label: 'Recommended', tone: 'neutral', reason: null },
      { label: 'Fastest', tone: 'neutral', reason: null },
      { label: 'May need a new ticket', tone: 'caution', reason: 'Your Advance ticket is for the 09:52 only.' },
    ]);
  });

  it('keeps a covered fit gray and three picks without a fit', () => {
    expect(
      suggestionBadges(replanSuggestion({ fit: 'covered', fitLabel: 'Usually covered by your ticket' })).at(-1)?.tone,
    ).toBe('neutral');
    expect(
      suggestionBadges(replanSuggestion({ pickLabels: ['A', 'B', 'C', 'D'], fit: null, fitLabel: null })).map(
        (badge) => badge.label,
      ),
    ).toEqual(['A', 'B', 'C']);
  });

  it('flags a suggestion outside the operator filter and names an operator-only one', () => {
    expect(
      suggestionBadges(
        replanSuggestion({
          pickLabels: ['Recommended', 'Fastest'],
          fit: 'covered',
          fitLabel: 'Usually covered by your ticket',
          outsideOperatorFilter: true,
        }),
      ).map((badge) => [badge.label, badge.tone]),
    ).toEqual([
      ['Recommended', 'neutral'],
      ['Uses other operators', 'caution'],
      ['Usually covered by your ticket', 'neutral'],
    ]);
    expect(
      suggestionBadges(
        replanSuggestion({
          pickLabels: [],
          fit: null,
          fitLabel: null,
          legs: [trainSuggestionLeg()],
          operatorOnly: ['GR'],
        }),
      ).map((badge) => badge.label),
    ).toEqual(['LNER only']);
  });
});

describe('suggestionLegBody', () => {
  it('maps a train leg, leaving out unknown ends', () => {
    expect(suggestionLegBody(trainSuggestionLeg())).toEqual({
      trainUid: 'L12345',
      serviceDate: '2026-09-17',
      originCrs: 'YRK',
      destinationCrs: 'EDB',
    });
    expect(suggestionLegBody(trainSuggestionLeg({ originCrs: null, destinationCrs: null }))).toEqual({
      trainUid: 'L12345',
      serviceDate: '2026-09-17',
    });
  });

  it('maps an untracked leg, leaving out nulls', () => {
    expect(
      suggestionLegBody({
        kind: 'untracked',
        transitMode: 'TUBE',
        originName: 'Euston',
        destinationName: 'Victoria',
        lineName: 'Victoria line',
        originStopId: '940GZZLUEUS',
        destinationStopId: null,
        plannedDeparture: '2026-09-17T09:00:00Z',
        plannedArrival: '2026-09-17T09:15:00Z',
      }),
    ).toEqual({
      kind: 'untracked',
      transitMode: 'tube',
      originName: 'Euston',
      destinationName: 'Victoria',
      lineName: 'Victoria line',
      originStopId: '940GZZLUEUS',
      plannedDeparture: '2026-09-17T09:00:00Z',
      plannedArrival: '2026-09-17T09:15:00Z',
    });
  });
});

describe('history', () => {
  it('labels fates', () => {
    expect(fateBadge('cancelled')).toEqual({ label: 'Cancelled', tone: 'critical' });
    expect(fateBadge('missed')).toEqual({ label: 'Missed', tone: 'late' });
    expect(fateBadge('not_used')).toEqual({ label: 'Not used', tone: 'neutral' });
    expect(fateBadge('travelled')).toEqual({ label: 'Travelled', tone: 'neutral' });
    expect(fateBadge('planned')).toBeNull();
  });

  it('titles plans', () => {
    expect(revisionTitle({ reason: 'booked' })).toBe('Booked plan');
    expect(revisionTitle({ reason: 'replace' })).toBe('Changed plan');
    expect(revisionTitle({ reason: 'auto_shift' })).toBe('Updated plan');
    expect(revisionTitle({ reason: 'revert' })).toBe('Earlier plan restored');
  });

  it('words a plan leg', () => {
    const leg: RevisionLeg = {
      position: 0,
      sourceLegId: 1,
      kind: 'train',
      trainUid: 'L1',
      serviceDate: '2026-09-17',
      atocCode: 'GR',
      originCrs: 'YRK',
      originName: 'York',
      destinationCrs: 'EDB',
      destinationName: 'Edinburgh',
      bookedDeparture: '2026-09-17T08:52:00Z',
      bookedArrival: '2026-09-17T11:00:00Z',
      transitMode: null,
      originPlace: null,
      destinationPlace: null,
      lineName: null,
      plannedDeparture: null,
      plannedArrival: null,
      ticketIds: [],
      fate: 'cancelled',
    };
    expect(revisionLegLine(leg)).toBe('09:52 York to Edinburgh');
    expect(
      revisionLegLine({
        ...leg,
        kind: 'untracked',
        transitMode: 'walk',
        originPlace: 'Euston',
        destinationPlace: "King's Cross",
        plannedDeparture: '2026-09-17T08:00:00Z',
      }),
    ).toBe("09:00 Walk, Euston to King's Cross");
  });
});

describe('missableLegId', () => {
  const now = new Date('2026-09-17T09:30:00Z');
  it('is the first train leg not yet over', () => {
    const first = trainLeg(1, ['KGX', 'Kings Cross'], ['YRK', 'York'], '2026-09-17T07:00:00Z', '2026-09-17T09:00:00Z', {
      status: 'completed',
    });
    const walk = untrackedLeg(2, 'York', 'York', '2026-09-17T09:05:00Z', '2026-09-17T09:10:00Z');
    const second = trainLeg(3, ['YRK', 'York'], ['EDB', 'Edinburgh'], '2026-09-17T10:00:00Z', '2026-09-17T12:00:00Z');
    expect(missableLegId([first, walk, second], now)).toBe(3);
  });

  it('is null when every train leg is over', () => {
    const done = trainLeg(1, ['KGX', 'Kings Cross'], ['YRK', 'York'], '2026-09-17T07:00:00Z', '2026-09-17T09:00:00Z', {
      status: 'completed',
    });
    expect(missableLegId([done], now)).toBeNull();
  });
});
