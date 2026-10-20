import { describe, it, expect } from 'vitest';
import { newUntrackedLegDraft, untrackedDraftArrivesNextDay, untrackedDraftRequest } from './UntrackedLegFields';

describe('untrackedDraftRequest', () => {
  const draft = (overrides: Partial<ReturnType<typeof newUntrackedLegDraft>> = {}) => ({
    ...newUntrackedLegDraft({ date: '2026-10-21', leaves: '08:05', originName: '  Euston ' }),
    arrives: '08:20',
    ...overrides,
  });

  it('sends London wall-clock times as UTC instants and leaves blank text out', () => {
    expect(untrackedDraftRequest(draft())).toEqual({
      mode: 'untracked',
      transitMode: 'tube',
      originName: 'Euston',
      plannedDeparture: '2026-10-21T07:05:00.000Z',
      plannedArrival: '2026-10-21T07:20:00.000Z',
    });
  });

  it('takes an arrival earlier than the departure as the next day', () => {
    const late = draft({ leaves: '23:50', arrives: '00:15' });
    expect(untrackedDraftArrivesNextDay(late)).toBe(true);
    expect(untrackedDraftRequest(late)).toMatchObject({
      plannedDeparture: '2026-10-21T22:50:00.000Z',
      plannedArrival: '2026-10-21T23:15:00.000Z',
    });
  });

  it('is null until the date and both times are complete', () => {
    expect(untrackedDraftRequest(draft({ arrives: '' }))).toBeNull();
    expect(untrackedDraftRequest(draft({ date: '2026-02-30' }))).toBeNull();
    expect(untrackedDraftRequest(draft({ incomplete: { leaves: true, arrives: false } }))).toBeNull();
  });
});
