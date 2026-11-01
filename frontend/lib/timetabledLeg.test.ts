import { describe, expect, it } from 'vitest';
import { noticeHref, timetableCaption, timetableNote, timetabledLineHref, vehicleNoun } from './timetabledLeg';
import type { LegTimetable } from './types';

function timetable(overrides: Partial<LegTimetable> = {}): LegTimetable {
  return {
    network: 'tfl',
    line: { id: 'tfl:victoria', name: 'Victoria line', colour: '#0098d4', mode: 'tube' },
    state: 'matched',
    scheduledDeparture: '2026-11-04T08:14:00Z',
    scheduledArrival: '2026-11-04T08:24:00Z',
    headsign: 'Brixton',
    calls: [],
    notices: [],
    ...overrides,
  };
}

describe('timetabled legs (DS-237)', () => {
  it('says whose timetable the times are from, never that they are live', () => {
    expect(timetableCaption(timetable(), 'tube')).toBe("Times from TfL's timetable, not live.");
    expect(timetableCaption(timetable({ network: 'metrolink', headwayMinutes: 6 }), 'tram')).toBe(
      "Times from Metrolink's timetable, not live. Trams usually run every 6 minutes.",
    );
    expect(timetableCaption(timetable({ network: 'elsewhere', headwayMinutes: 1 }), 'dlr')).toBe(
      'Times from the timetable, not live. Trains usually run every minute.',
    );
    expect(vehicleNoun('tram')).toBe('tram');
    expect(vehicleNoun('dlr')).toBe('train');
  });

  it('notes a retimed, dropped or closed ride, and nothing once the leg is done', () => {
    expect(timetableNote(timetable(), 'tube', false)).toBeNull();
    expect(timetableNote(timetable({ state: 'retimed' }), 'tube', false)).toEqual({
      text: 'The timetable now has this train at 08:14 to 08:24.',
      tone: 'neutral',
    });
    expect(timetableNote(timetable({ state: 'notInTimetable' }), 'tube', false)?.text).toBe(
      "This train isn't in the timetable any more. The Victoria line still runs between these stops that day, so check the times before you travel.",
    );
    expect(timetableNote(timetable({ state: 'noService' }), 'tram', false)).toEqual({
      text: 'The timetable has no Victoria line trams between these stops that day. It may be closed for planned work, so check before you travel.',
      tone: 'caution',
    });
    expect(timetableNote(timetable({ state: 'noTimetable' }), 'tube', false)?.text).toBe(
      "We don't have the timetable for that day yet.",
    );
    for (const state of ['notInTimetable', 'noService', 'noTimetable'] as const) {
      expect(timetableNote(timetable({ state }), 'tube', true)).toBeNull();
    }
  });

  it('links a disruption to its page, and a line to its page', () => {
    const notice = {
      kind: 'disruption' as const,
      severity: 6,
      description: 'Severe Delays',
      message: 'm',
      reason: null,
    };
    expect(noticeHref('metrolink', { ...notice, lineId: 'metrolink:blue', situationId: 'S 1' })).toBe(
      '/transit/metrolink/incidents/S%201',
    );
    expect(noticeHref('tfl', { ...notice, lineId: 'tfl:victoria' })).toBe('/lines/tfl-victoria');
    expect(noticeHref('metrolink', { ...notice, lineId: 'metrolink:blue' })).toBeNull();
    expect(timetabledLineHref(timetable())).toBe('/transit/tfl/lines/victoria');
    expect(timetabledLineHref(timetable({ line: null }))).toBeNull();
  });
});
