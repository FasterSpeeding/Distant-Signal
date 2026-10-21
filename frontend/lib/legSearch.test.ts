import { describe, it, expect } from 'vitest';
import { legSearchDraftFromLeg, legSearchFromDraft, legSearchPickBody, legSearchQuery } from './legSearch';

const leg = {
  originCrs: 'KGX',
  destinationCrs: 'YRK',
  serviceDate: '2026-10-23',
  departAfter: '08:00:00',
  departBefore: null,
  arriveAfter: '10:00:00',
  arriveBefore: '11:30:00',
};

describe('legSearch', () => {
  it('starts from every setting stored on the leg, times to the minute', () => {
    expect(legSearchDraftFromLeg(leg)).toEqual({
      origin: 'KGX',
      destination: 'YRK',
      serviceDate: '2026-10-23',
      departAfter: '08:00',
      departBefore: '',
      arriveAfter: '10:00',
      arriveBefore: '11:30',
    });
    expect(legSearchDraftFromLeg({ ...leg, originCrs: null, destinationCrs: null })).toMatchObject({
      origin: '',
      destination: '',
    });
  });

  it('is a search only with both stations, a date, and at least one time in order', () => {
    const draft = legSearchDraftFromLeg(leg);
    const stations = { origin: 'KGX', destination: 'YRK' };
    expect(legSearchFromDraft(draft, stations)).toEqual({
      originCrs: 'KGX',
      destinationCrs: 'YRK',
      serviceDate: '2026-10-23',
      departAfter: '08:00',
      departBefore: null,
      arriveAfter: '10:00',
      arriveBefore: '11:30',
    });
    expect(legSearchFromDraft(draft, { ...stations, destination: null })).toBeNull();
    expect(legSearchFromDraft({ ...draft, serviceDate: '' }, stations)).toBeNull();
    const noTimes = { ...draft, departAfter: '', arriveAfter: '', arriveBefore: ' ' };
    expect(legSearchFromDraft(noTimes, stations)).toBeNull();
    expect(legSearchFromDraft({ ...draft, arriveAfter: '12:00' }, stations)).toBeNull();
    expect(legSearchFromDraft({ ...draft, departBefore: '07:00' }, stations)).toBeNull();
    expect(legSearchFromDraft({ ...draft, departAfter: '8am' }, stations)).toBeNull();
  });

  it('sends the set bounds as query and as the pick body', () => {
    const search = legSearchFromDraft(legSearchDraftFromLeg(leg), { origin: 'KGX', destination: 'YRK' });
    expect(search).not.toBeNull();
    if (search === null) return;
    expect(legSearchQuery(search).toString()).toBe(
      'origin=KGX&destination=YRK&serviceDate=2026-10-23&departAfter=08%3A00&arriveAfter=10%3A00&arriveBefore=11%3A30',
    );
    expect(legSearchPickBody(search)).toEqual({
      originCrs: 'KGX',
      destinationCrs: 'YRK',
      departWindow: { after: '08:00', before: null },
      arriveWindow: { after: '10:00', before: '11:30' },
    });
  });
});
