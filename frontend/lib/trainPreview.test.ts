import { describe, expect, it } from 'vitest';
import { SNAPSHOT_NOTE } from './linePreview';
import {
  serviceDateLabel,
  trainIsLive,
  trainPreviewCard,
  trainPreviewImage,
  trainPreviewTitle,
  trainPreviewVersion,
  trainScheduleLine,
} from './trainPreview';
import {
  CANCELLED_TRAIN,
  DELAYED_TRAIN,
  ON_TIME_TRAIN,
  PLANNED_TRAIN,
  TRAIN_PREVIEW_NOW,
  previewStop,
  previewTrain,
} from '@/test/trainPreviewFixtures';

const NOW = TRAIN_PREVIEW_NOW;
const MINUTE = 60_000;

describe('trainPreviewCard', () => {
  it('says an on-time train is on time, where it last called and its next stop', () => {
    expect(trainPreviewCard(ON_TIME_TRAIN, NOW)).toEqual({
      band: { tone: 'good', label: 'On time', aside: 'As of Fri 30 Oct, 14:00' },
      title: '13:42 Woking to London Waterloo',
      lead: 'Departed Surbiton at 13:56',
      chips: { names: [], more: 0 },
      rows: [
        'Next stop: Wimbledon, expected 14:03',
        'South Western Railway · Headcode 2Z42 · Fri 30 Oct',
        SNAPSHOT_NOTE,
      ],
    });
  });

  it("gives a late train's delay in the late badge's words and colour", () => {
    const card = trainPreviewCard(DELAYED_TRAIN, NOW);
    expect(card.band).toMatchObject({ tone: 'late', label: '12 min late' });
    expect(card.lead).toBe('Departed Haywards Heath at 13:47');
    expect(card.rows[0]).toBe('Next stop: East Croydon, expected 14:08');
  });

  it('marks a forecast delay as expected, and an early train teal', () => {
    expect(trainPreviewCard({ ...DELAYED_TRAIN, delayProvisional: true }, NOW).band.label).toBe('Exp. 12 min late');
    expect(trainPreviewCard({ ...ON_TIME_TRAIN, delayMinutes: -2 }, NOW).band).toMatchObject({
      tone: 'early',
      label: '2 min early',
    });
  });

  it('says a cancelled train is cancelled, and why', () => {
    const card = trainPreviewCard(CANCELLED_TRAIN, NOW);
    expect(card.band).toMatchObject({ tone: 'severe', label: 'Cancelled' });
    expect(card.lead).toBe('Reason: Points failure');
    expect(card.rows).toEqual(['Lewes 14:20 to Seaford 14:36', 'Southern · Headcode 2Z20 · Fri 30 Oct', SNAPSHOT_NOTE]);
  });

  it('puts the timetable first for a cancellation without a reason', () => {
    const card = trainPreviewCard(
      { ...CANCELLED_TRAIN, cancellation: { ...CANCELLED_TRAIN.cancellation!, reason: null } },
      NOW,
    );
    expect(card.lead).toBe('Lewes 14:20 to Seaford 14:36');
    expect(card.rows).toEqual(['Southern · Headcode 2Z20 · Fri 30 Oct', SNAPSHOT_NOTE]);
  });

  it('names where a train cut short terminates', () => {
    const surbiton = { crs: 'SUR', name: 'Surbiton', stopIndex: 1 };
    const waterloo = { crs: 'WAT', name: 'London Waterloo', stopIndex: 3 };
    const card = trainPreviewCard(
      {
        ...ON_TIME_TRAIN,
        cancellation: {
          kind: 'partial',
          startsFrom: null,
          terminatesAt: surbiton,
          from: surbiton,
          to: waterloo,
          sections: [{ from: surbiton, to: waterloo }],
          reason: { code: 'TG', text: 'Driver', source: 'trust' },
          at: '2026-10-30T13:50:00Z',
        },
      },
      NOW,
    );
    expect(card.band).toMatchObject({ tone: 'severe', label: 'Terminates at Surbiton' });
    expect(card.lead).toBe('Reason: Driver');
  });

  it('gives a future train a static card: its timetable and booked platform, under its date', () => {
    expect(trainPreviewCard(PLANNED_TRAIN, NOW)).toEqual({
      band: { tone: 'informational', label: 'Planned', aside: 'Sat 31 Oct' },
      title: '07:15 Reading to London Paddington',
      lead: 'Reading 07:15 to London Paddington 07:42',
      chips: { names: [], more: 0 },
      rows: ['Booked platform 4 at Reading', 'Great Western Railway · Headcode 1Z15'],
    });
  });

  it("says today's train that hasn't started is planned or awaiting its first report", () => {
    const today = { ...PLANNED_TRAIN, serviceDate: '2026-10-30' };
    expect(trainPreviewCard(today, NOW).band).toMatchObject({ label: 'Planned', aside: 'As of Fri 30 Oct, 14:00' });
    expect(trainPreviewCard(today, NOW).rows).toContain(SNAPSHOT_NOTE);
    expect(trainPreviewCard({ ...today, trackingPhase: 'awaitingReport' }, NOW).band.label).toBe(
      'Awaiting first report',
    );
  });

  it('names a live platform at the origin without calling it booked', () => {
    const today = {
      ...PLANNED_TRAIN,
      serviceDate: '2026-10-30',
      journeyStops: [
        previewStop('Reading', 'RDG', { kind: 'Origin', platform: '9', platformBasis: 'board', bookedPlatform: '4' }),
      ],
    };
    expect(trainPreviewCard(today, NOW).rows[0]).toBe('Platform 9 at Reading');
  });

  it('says an arrived train arrived, with its final delay', () => {
    const stops = ON_TIME_TRAIN.journeyStops!.map((stop) =>
      stop.kind === 'Terminate' ? { ...stop, actualArrival: '2026-10-30T14:21:00Z' } : stop,
    );
    const card = trainPreviewCard({ ...ON_TIME_TRAIN, status: 'completed', delayMinutes: 5, journeyStops: stops }, NOW);
    expect(card.band).toMatchObject({ tone: 'late', label: 'Arrived 5 min late' });
    expect(card.lead).toBe('Arrived at London Waterloo at 14:21');
    const onTime = trainPreviewCard({ ...ON_TIME_TRAIN, status: 'completed', delayMinutes: 0 }, NOW);
    expect(onTime.band).toMatchObject({ tone: 'good', label: 'Arrived on time' });
  });

  it('gives a train whose day is over a static card of how it ended', () => {
    const tomorrow = NOW + 24 * 60 * MINUTE;
    const arrived = trainPreviewCard({ ...ON_TIME_TRAIN, status: 'completed', delayMinutes: 0 }, tomorrow);
    expect(arrived.band).toEqual({ tone: 'good', label: 'Arrived on time', aside: 'Fri 30 Oct' });
    expect(arrived.rows).not.toContain(SNAPSHOT_NOTE);
    expect(trainPreviewCard(ON_TIME_TRAIN, tomorrow).band.label).toBe('No arrival reported');
    expect(trainPreviewCard({ ...ON_TIME_TRAIN, status: null }, tomorrow).band.label).toBe('No report received');
    expect(trainPreviewCard(CANCELLED_TRAIN, tomorrow).band).toEqual({
      tone: 'severe',
      label: 'Cancelled',
      aside: 'Fri 30 Oct',
    });
  });

  it('keeps a train that started before midnight live until a while after it is due', () => {
    const lateNight = previewTrain({
      journeyStops: [
        previewStop('Woking', 'WOK', { kind: 'Origin', scheduledDeparture: '2026-10-30T23:40:00Z' }),
        previewStop('London Waterloo', 'WAT', { kind: 'Terminate', scheduledArrival: '2026-10-31T00:14:00Z' }),
      ],
    });
    expect(trainIsLive(lateNight, Date.parse('2026-10-31T00:30:00Z'))).toBe(true);
    expect(trainIsLive(lateNight, Date.parse('2026-10-31T04:00:00Z'))).toBe(false);
    expect(trainIsLive(PLANNED_TRAIN, NOW)).toBe(false);
  });

  it('says a train may have arrived, never that it did', () => {
    expect(trainPreviewCard({ ...ON_TIME_TRAIN, mayHaveArrived: true }, NOW).band).toMatchObject({
      tone: 'informational',
      label: 'May have arrived',
    });
  });

  it('gives a bus its timetable, never a live state', () => {
    const bus = { ...ON_TIME_TRAIN, serviceMode: 'replacementBus' as const, liveTracking: false };
    const card = trainPreviewCard(bus, NOW);
    expect(card.band).toMatchObject({ tone: 'informational', label: 'Timetable only', aside: 'Fri 30 Oct' });
    expect(card.lead).toBe('Woking 13:42 to London Waterloo 14:16');
  });

  it('names a reported location by its stop, or by its code when it is none of them', () => {
    const bare = { ...ON_TIME_TRAIN, journeyStops: null, lastReportedLocation: 'CLJ' };
    expect(trainPreviewCard(bare, NOW).lead).toBe('Last reported at CLJ');
    const named = {
      ...ON_TIME_TRAIN,
      lastReportedLocation: 'WIM',
      journeyStops: ON_TIME_TRAIN.journeyStops!.map((stop) => ({
        ...stop,
        actualArrival: null,
        actualDeparture: null,
      })),
    };
    expect(trainPreviewCard(named, NOW).lead).toBe('Last reported at Wimbledon');
  });

  it("says a train that isn't in the timetable can't be tracked", () => {
    const card = trainPreviewCard({ ...ON_TIME_TRAIN, status: null, trackingPhase: 'noTimetable' }, NOW);
    expect(card.band.label).toBe('Not in the timetable');
  });
});

describe('trainPreviewTitle and trainScheduleLine', () => {
  it("use the page's own title, and the timetable end to end", () => {
    expect(trainPreviewTitle(ON_TIME_TRAIN)).toBe('13:42 Woking to London Waterloo');
    expect(trainPreviewTitle({ ...ON_TIME_TRAIN, originName: null, originCrs: null })).toBe('Train Z90001');
    expect(trainScheduleLine(ON_TIME_TRAIN)).toBe('Woking 13:42 to London Waterloo 14:16');
    expect(trainScheduleLine({ ...ON_TIME_TRAIN, journeyStops: null })).toBe('Woking 13:42 to London Waterloo');
    expect(serviceDateLabel('2026-10-31')).toBe('Sat 31 Oct');
  });
});

describe('trainPreviewVersion', () => {
  it('changes with what the card says, not with the clock', () => {
    const v = trainPreviewVersion('Z90001', '2026-10-30', trainPreviewCard(ON_TIME_TRAIN, NOW));
    expect(v).toMatch(/^[0-9a-f]{8}$/);
    // A minute later, nothing moved: the same URL.
    expect(trainPreviewVersion('Z90001', '2026-10-30', trainPreviewCard(ON_TIME_TRAIN, NOW + MINUTE))).toBe(v);
    // Late now, or cancelled: a new one.
    const late = trainPreviewCard({ ...ON_TIME_TRAIN, delayMinutes: 4 }, NOW);
    expect(trainPreviewVersion('Z90001', '2026-10-30', late)).not.toBe(v);
    const cancelled = trainPreviewCard({ ...ON_TIME_TRAIN, status: 'cancelled' }, NOW);
    expect(trainPreviewVersion('Z90001', '2026-10-30', cancelled)).not.toBe(v);
    // Its day over: the static card, a new URL once.
    const over = trainPreviewCard(ON_TIME_TRAIN, NOW + 24 * 60 * MINUTE);
    expect(trainPreviewVersion('Z90001', '2026-10-30', over)).not.toBe(v);
  });

  it('still versions the URL when the train could not be read', () => {
    expect(trainPreviewVersion('Z90001', '2026-10-30', null)).toMatch(/^[0-9a-f]{8}$/);
    expect(trainPreviewVersion('Z90001', '2026-10-30', null)).not.toBe(
      trainPreviewVersion('Z90001', '2026-10-30', trainPreviewCard(ON_TIME_TRAIN, NOW)),
    );
  });
});

describe('trainPreviewImage', () => {
  it("is the page's card.png with ?v=, as a 1200x630 PNG", () => {
    const card = trainPreviewCard(ON_TIME_TRAIN, NOW);
    expect(trainPreviewImage('Z90001', '2026-10-30', card, 'alt')).toEqual({
      url: `/train/Z90001/2026-10-30/card.png?v=${trainPreviewVersion('Z90001', '2026-10-30', card)}`,
      alt: 'alt',
      width: 1200,
      height: 630,
      type: 'image/png',
    });
  });
});
