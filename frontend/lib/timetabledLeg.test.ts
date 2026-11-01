import { describe, expect, it } from 'vitest';
import {
  arrivalsForLeg,
  lineLabel,
  nextTimetabledNote,
  noticeHref,
  retimedArrival,
  showsLiveArrivals,
  timetableCaption,
  timetableNote,
  timetabledLineHref,
  vehicleNoun,
} from './timetabledLeg';
import type { JourneyLegDetail } from './types';
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
    expect(lineLabel(timetable({ line: { id: 'tfl:victoria', name: 'Victoria', colour: null, mode: 'tube' } }))).toBe(
      'Victoria line',
    );
    expect(lineLabel(timetable({ line: { id: 'tfl:dlr', name: 'DLR', colour: null, mode: 'dlr' } }))).toBe('DLR');
    expect(timetableNote(timetable({ state: 'notInTimetable' }), 'tube', false)?.text).toBe(
      'The timetable has no train at this time. The Victoria line still runs between these stops that day, so check the times before you travel.',
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

  it('says which ride a late traveller takes, only when it is not the planned one', () => {
    const leg = (connection: JourneyLegDetail['connection']) =>
      ({ kind: 'untracked', transitMode: 'tube', connection }) as JourneyLegDetail;
    const ride = { departure: '2026-11-04T08:19:00Z', arrival: '2026-11-04T08:29:00Z' };
    expect(
      nextTimetabledNote(leg({ gapMinutes: 2, minimumChangeMinutes: null, state: 'ok', nextTimetabled: ride })),
    ).toBe('On the timetable, the next train once you get there leaves at 08:19 and gets in at 08:29.');
    expect(retimedArrival(leg({ gapMinutes: 2, minimumChangeMinutes: null, state: 'ok', nextTimetabled: ride }))).toBe(
      '2026-11-04T08:29:00Z',
    );
    expect(nextTimetabledNote(leg({ gapMinutes: 5, minimumChangeMinutes: null, state: 'ok' }))).toBeNull();
    expect(retimedArrival(leg(null))).toBeNull();
  });

  it("picks the predictions for the leg's line that reach its stop, and shows them only around the leg", () => {
    const arrival = (id: string, destination: string, at: string) => ({
      id,
      destination,
      expectedArrival: at,
      timeToStationSecs: 60,
    });
    const picked = arrivalsForLeg(
      {
        stopId: '940GZZLUKSX',
        fetchedAt: '2026-11-04T08:00:00Z',
        groups: [
          {
            lineId: 'victoria',
            lineName: 'Victoria',
            platformName: '5',
            arrivals: [
              arrival('late', 'Kings Cross St Pancras', '2026-11-04T08:09:00Z'),
              arrival('other', 'Walthamstow Central', '2026-11-04T08:01:00Z'),
            ],
          },
          {
            lineId: 'victoria',
            lineName: 'Victoria',
            platformName: '6',
            arrivals: [arrival('soon', "King's Cross St. Pancras", '2026-11-04T08:02:00Z')],
          },
          {
            lineId: 'northern',
            lineName: 'Northern',
            platformName: '1',
            arrivals: [arrival('wrong line', "King's Cross St. Pancras", '2026-11-04T08:00:30Z')],
          },
        ],
      },
      { stopId: '940GZZLUKSX', lineId: 'victoria', destinations: ["King's Cross St. Pancras"] },
    );
    expect(picked.map((a) => a.id)).toEqual(['soon', 'late']);

    const leg = {
      kind: 'untracked',
      plannedDeparture: '2026-11-04T09:00:00Z',
      plannedArrival: '2026-11-04T09:20:00Z',
      timetable: { liveArrivals: { stopId: 'x', lineId: 'victoria', destinations: [] } },
    } as unknown as JourneyLegDetail;
    expect(showsLiveArrivals(leg, new Date('2026-11-04T07:59:00Z'))).toBe(false);
    expect(showsLiveArrivals(leg, new Date('2026-11-04T08:00:00Z'))).toBe(true);
    expect(showsLiveArrivals(leg, new Date('2026-11-04T09:19:59Z'))).toBe(true);
    expect(showsLiveArrivals(leg, new Date('2026-11-04T09:20:00Z'))).toBe(false);
    expect(
      showsLiveArrivals({ ...leg, kind: 'untracked', plannedDeparture: null }, new Date('2026-11-04T08:30:00Z')),
    ).toBe(false);
  });
});
