import type {
  TransitBoard,
  TransitLineDetail,
  TransitLineTimetable,
  TransitLineStatus,
  TransitNetworkDetail,
  TransitSituation,
  TransitSituations,
  TransitStop,
  TransitStopDetail,
} from '@/lib/types';

/** API responses for the transit pages' tests, in the api's shapes
 * (docs/api-changelog.md, 2026-10-14). */
export const NETWORK: TransitNetworkDetail = {
  id: 'metrolink',
  name: 'Manchester Metrolink',
  mode: 'tram',
  attribution: {
    licence: 'OGL-UK-3.0',
    text: 'Contains public sector information licensed under the Open Government Licence v3.0.',
  },
  feedVersion: '20261014_030249-063231e4ad19',
  horizonFrom: '2026-10-13',
  horizonTo: '2026-12-13',
  timetableAsOf: '2026-10-14T03:10:00Z',
  situationsAsOf: null,
  lines: [
    { id: 'metrolink:blue', lineId: 'blue', name: 'Blue Line', colour: '#1f6fb5' },
    { id: 'metrolink:green', lineId: 'green', name: 'Green Line', colour: '#2e8b3e' },
  ],
};

export const STOPS: TransitStop[] = [
  { id: '9400ZZMAECC', name: 'Eccles', lat: 53.48, lon: -2.33, lines: ['metrolink:blue'], railLinks: [] },
  {
    id: '9400ZZMAPIC',
    name: 'Piccadilly',
    lat: 53.48,
    lon: -2.23,
    lines: ['metrolink:blue', 'metrolink:green'],
    railLinks: [{ crs: 'MAN', tiploc: 'MNCRPIC', kind: 'interchange', note: null }],
  },
];

export const LINE: TransitLineDetail = {
  ...NETWORK.lines[0]!,
  directions: [
    {
      direction: 'outbound',
      headsign: 'Eccles',
      stations: [
        { id: '9400ZZMAPIC', name: 'Piccadilly' },
        { id: '9400ZZMAECC', name: 'Eccles' },
      ],
    },
    {
      direction: 'inbound',
      headsign: 'Piccadilly',
      stations: [
        { id: '9400ZZMAECC', name: 'Eccles' },
        { id: '9400ZZMAPIC', name: 'Piccadilly' },
      ],
    },
  ],
};

export const STOP: TransitStopDetail = {
  ...STOPS[1]!,
  platforms: [
    { id: '9400ZZMAPIC1', indicator: 'From City' },
    { id: '9400ZZMAPIC2', indicator: 'To Ashton' },
  ],
};

export const BOARD: TransitBoard = {
  date: '2026-10-14',
  departures: [
    {
      scheduledDeparture: '2026-10-14T07:02:00+01:00',
      tripId: 'T1',
      line: NETWORK.lines[0]!,
      destination: 'Eccles',
      platformId: '9400ZZMAPIC2',
      platform: 'To Ashton',
      replacementBus: false,
    },
    {
      scheduledDeparture: '2026-10-14T07:10:00+01:00',
      tripId: 'B1',
      line: null,
      destination: 'Eccles',
      platformId: '9400ZZMAPIC1',
      platform: null,
      replacementBus: true,
    },
  ],
  nextCursor: '25800.B1.9400ZZMAPIC1',
  outsideHorizon: false,
  realtime: false,
  kind: 'scheduled',
};

export const TIMETABLE: TransitLineTimetable = {
  date: '2026-10-14',
  direction: 'outbound',
  stations: LINE.directions[0]!.stations,
  trips: [
    { tripId: 'T1', headsign: 'Eccles', times: ['07:02', '07:32'] },
    { tripId: 'T2', headsign: 'Eccles', times: ['07:14', null] },
  ],
  outsideHorizon: false,
  realtime: false,
  kind: 'scheduled',
};

/** A current delay on the Blue Line, its description with markup in it
 * (which must show as text). */
export const DELAY: TransitSituation = {
  id: 'S1',
  summary: 'Delays between Piccadilly and Eccles',
  description: 'A <b>fault</b> at Cornbrook.',
  infoUrl: 'https://example.invalid/s1',
  planned: false,
  reason: 'equipment:pointsFailure',
  scope: 'line',
  lines: ['metrolink:blue'],
  stops: [],
  advice: ['Allow extra time.'],
  validity: [{ start: '2026-10-14T06:00:00Z', end: null }],
  starts: null,
};

/** A lift notice at Piccadilly. */
export const LIFT: TransitSituation = {
  id: 'S2',
  summary: 'Lift out of service at Piccadilly',
  description: null,
  infoUrl: 'javascript:alert(1)',
  planned: false,
  reason: 'equipment:liftFailure',
  scope: 'stop',
  lines: [],
  stops: ['9400ZZMAPIC'],
  advice: [],
  validity: [{ start: '2026-10-14T06:00:00Z', end: '2099-10-20T00:00:00Z' }],
  starts: null,
};

/** Planned works on the Green Line, starting later. */
export const WORKS: TransitSituation = {
  id: 'S3',
  summary: 'No trams to Deansgate-Castlefield',
  description: null,
  infoUrl: null,
  planned: true,
  reason: 'miscellaneous:engineeringWorks',
  scope: 'line',
  lines: ['metrolink:green'],
  stops: [],
  advice: ['Use replacement buses.'],
  validity: [{ start: '2026-10-18T05:00:00Z', end: '2026-10-19T23:00:00Z' }],
  starts: '2026-10-18T05:00:00Z',
};

export const SITUATIONS: TransitSituations = {
  state: 'current',
  asOf: '2026-10-14T09:00:00Z',
  current: [DELAY, LIFT],
  upcoming: [WORKS],
};

export const MINOR_DELAYS: TransitLineStatus = {
  state: 'current',
  asOf: '2026-10-14T09:00:00Z',
  statuses: [{ severity: 9, description: 'Minor Delays', reason: DELAY.summary, situationId: 'S1' }],
  upcoming: [],
};

export const NOTHING_REPORTED: TransitLineStatus = {
  state: 'current',
  asOf: '2026-10-14T09:00:00Z',
  statuses: [
    {
      severity: 27,
      description: 'No Reported Disruption',
      reason: 'No disruption has been published for this line.',
      situationId: null,
    },
  ],
  upcoming: [],
};

export const STATUS_UNAVAILABLE: TransitLineStatus = {
  state: 'unavailable',
  asOf: '2026-10-14T08:00:00Z',
  statuses: [],
  upcoming: [],
};

/** The TfL network (the TfL transit design): lines of several modes, TfL's
 * own status, the National Rail lines it links. Invented ids. */
export const TFL_NETWORK: TransitNetworkDetail = {
  id: 'tfl',
  name: 'London (TfL)',
  mode: 'metro',
  attribution: { licence: 'TfL-Open-Data', text: 'Powered by TfL Open Data.' },
  feedVersion: '20261025_200505-0123456789ab',
  horizonFrom: '2026-10-29',
  horizonTo: '2026-12-29',
  timetableAsOf: '2026-10-30T04:00:00Z',
  situationsAsOf: null,
  statusSource: 'tfl',
  status: { state: 'current', asOf: '2026-10-30T09:00:00Z' },
  lines: [
    {
      id: 'tfl:victoria',
      lineId: 'victoria',
      name: 'Victoria',
      colour: '#1f97d4',
      mode: 'tube',
      statusLineId: 'tfl-victoria',
      status: MINOR_DELAYS,
    },
    { id: 'tfl:dlr', lineId: 'dlr', name: 'DLR', colour: '#14a39a', mode: 'dlr', statusLineId: 'tfl-dlr' },
    {
      id: 'tfl:central',
      lineId: 'central',
      name: 'Central',
      colour: '#d6362b',
      mode: 'tube',
      statusLineId: 'tfl-central',
    },
  ],
  nationalRailLines: [
    { id: 'elizabeth-line', name: 'Elizabeth line' },
    { id: 'overground-mildmay', name: 'Mildmay line' },
  ],
};

export const TFL_LINE: TransitLineDetail = {
  ...TFL_NETWORK.lines[0]!,
  statusSource: 'tfl',
  status: MINOR_DELAYS,
  situations: { state: 'current', asOf: '2026-10-30T09:00:00Z', current: [], upcoming: [] },
  directions: [
    {
      direction: 'outbound',
      headsign: null,
      stations: [
        { id: '9400ZZLUBXN', name: 'Brixton' },
        { id: '9400ZZLUWWL', name: 'Walthamstow Central' },
      ],
    },
  ],
  frequency: {
    date: '2026-10-30',
    outsideHorizon: false,
    directions: [
      {
        direction: 'outbound',
        station: { id: '9400ZZLUBXN', name: 'Brixton' },
        first: '05:27',
        last: '00:28',
        bands: [
          { band: 'early', from: '00:00', to: '07:00', departures: 28, minGapMinutes: 2, maxGapMinutes: 10 },
          { band: 'morning-peak', from: '07:00', to: '10:00', departures: 106, minGapMinutes: 1, maxGapMinutes: 3 },
        ],
      },
    ],
  },
};
