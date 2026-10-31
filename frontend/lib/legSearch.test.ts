import { describe, it, expect } from 'vitest';
import {
  legSearchDraftFromLeg,
  legSearchFromDraft,
  legSearchPickBody,
  legSearchPrefill,
  legSearchQuery,
} from './legSearch';
import { trainLeg } from '@/test/journeyFixtures';

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
      filters: null,
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
      filters: null,
    });
  });
});

describe('legSearch with Between stations filters (DS-215)', () => {
  it('sends the filters as the query and saves them with the pick', () => {
    const filters = { operator: 'SW', startsAt: 'WOK', callsAt: 'CLJ,VXH' };
    const search = legSearchFromDraft(legSearchDraftFromLeg(leg), { origin: 'KGX', destination: 'YRK' }, filters);
    expect(search?.filters).toEqual(filters);
    if (search === null) return;
    expect(legSearchQuery(search).toString()).toBe(
      'origin=KGX&destination=YRK&serviceDate=2026-10-23&departAfter=08%3A00&arriveAfter=10%3A00&arriveBefore=11%3A30' +
        '&operator=SW&startsAt=WOK&callsAt=CLJ%2CVXH',
    );
    expect(legSearchPickBody(search).filters).toEqual(filters);
  });
});

describe('legSearchPrefill: Change train on a leg never window-searched', () => {
  const KGX: [string, string] = ['KGX', 'London Kings Cross'];
  const YRK: [string, string] = ['YRK', 'York'];

  it("searches the leg's stations and date, from its train's timetabled departure", () => {
    expect(legSearchPrefill(trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z'))).toEqual({
      originCrs: 'KGX',
      destinationCrs: 'YRK',
      serviceDate: '2026-09-17',
      departAfter: '10:00',
      departBefore: null,
      arriveAfter: null,
      arriveBefore: null,
      filters: null,
    });
  });

  it("falls back to the train's pin for stations the leg doesn't store", () => {
    const leg = { ...trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z'), originCrs: null };
    expect(legSearchPrefill(leg)).toMatchObject({ originCrs: 'KGX', destinationCrs: 'YRK' });
  });

  it("starts at midnight when the departure is not on the leg's date", () => {
    const leg = { ...trainLeg(1, KGX, YRK, '2026-09-17T23:30:00Z', '2026-09-18T01:00:00Z'), serviceDate: '2026-09-17' };
    expect(legSearchPrefill(leg)).toMatchObject({ departAfter: '00:00' });
  });

  it('is null for a window-searched leg, which has its own search', () => {
    const leg = { ...trainLeg(1, KGX, YRK, '2026-09-17T09:00:00Z', '2026-09-17T11:00:00Z'), windowSearched: true };
    expect(legSearchPrefill(leg)).toBeNull();
  });
});
