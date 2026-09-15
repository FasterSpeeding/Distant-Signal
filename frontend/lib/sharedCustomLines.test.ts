import { describe, it, expect } from 'vitest';
import { mergeSharedCustomLines } from './sharedCustomLines';
import type { SharedGroupCustomLine } from './types';

function row(overrides: Partial<SharedGroupCustomLine> = {}): SharedGroupCustomLine {
  return {
    groupId: 'group-1',
    groupName: 'Family',
    lineId: 'custom-my-commute',
    lineName: 'My Commute',
    grantedBy: 'user-1',
    grantedByName: 'Alex',
    ...overrides,
  };
}

describe('mergeSharedCustomLines', () => {
  it('keeps one row per line, carrying every group it arrived through', () => {
    const merged = mergeSharedCustomLines([
      row({ groupId: 'g1', groupName: 'Family' }),
      row({ groupId: 'g2', groupName: 'Commute Buddies' }),
    ]);

    expect(merged).toHaveLength(1);
    expect(merged[0].line.lineId).toBe('custom-my-commute');
    expect(merged[0].groupNames).toEqual(['Family', 'Commute Buddies']);
  });

  it('de-duplicates a repeated group name rather than tagging the row twice', () => {
    const merged = mergeSharedCustomLines([
      row({ groupId: 'g1', groupName: 'Family' }),
      row({ groupId: 'g1', groupName: 'Family' }),
    ]);

    expect(merged[0].groupNames).toEqual(['Family']);
  });

  it('keeps genuinely different lines apart', () => {
    const merged = mergeSharedCustomLines([
      row({ lineId: 'custom-a', lineName: 'A' }),
      row({ lineId: 'custom-b', lineName: 'B' }),
    ]);

    expect(merged.map((m) => m.line.lineId)).toEqual(['custom-a', 'custom-b']);
  });

  it('drops a line the caller owns, so it can never render twice on one page', () => {
    const merged = mergeSharedCustomLines(
      [row({ lineId: 'custom-mine' }), row({ lineId: 'custom-theirs' })],
      new Set(['custom-mine']),
    );

    expect(merged.map((m) => m.line.lineId)).toEqual(['custom-theirs']);
  });

  it('returns an empty list for no rows', () => {
    expect(mergeSharedCustomLines([])).toEqual([]);
  });
});
