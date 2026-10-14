import type {
  TransitBoard,
  TransitLineDetail,
  TransitLineTimetable,
  TransitNetworkDetail,
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
