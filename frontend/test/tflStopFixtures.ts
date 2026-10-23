import type { LineStatusReport, TflStopArrivals, TflStopDetail } from '@/lib/types';

/** Russell Square, as `GET /public/tfl/stops/940GZZLURSQ` answers. */
export const RUSSELL_SQUARE: TflStopDetail = {
  id: '940GZZLURSQ',
  name: 'Russell Square',
  modes: ['tube'],
  lines: [{ id: 'piccadilly', name: 'Piccadilly' }],
  railStations: [],
};

/** King's Cross St. Pancras, with its National Rail station and two lines. */
export const KINGS_CROSS: TflStopDetail = {
  id: '940GZZLUKSX',
  name: "King's Cross St. Pancras",
  modes: ['tube'],
  lines: [
    { id: 'northern', name: 'Northern' },
    { id: 'victoria', name: 'Victoria' },
  ],
  railStations: [
    { crs: 'KGX', name: 'London Kings Cross' },
    { crs: 'STP', name: 'London St Pancras International' },
  ],
};

/** `GET /public/tfl/stops/940GZZLURSQ/arrivals`, two platforms. */
export const RUSSELL_SQUARE_ARRIVALS: TflStopArrivals = {
  stopId: '940GZZLURSQ',
  fetchedAt: '2026-10-23T09:00:00Z',
  groups: [
    {
      lineId: 'piccadilly',
      lineName: 'Piccadilly',
      platformName: 'Eastbound - Platform 1',
      direction: 'inbound',
      arrivals: [
        {
          id: '-1416081477',
          destination: 'Cockfosters',
          expectedArrival: '2026-10-23T09:00:40Z',
          timeToStationSecs: 40,
          currentLocation: 'At Holborn',
        },
        {
          id: '-1416081481',
          destination: 'Arnos Grove',
          expectedArrival: '2026-10-23T09:06:00Z',
          timeToStationSecs: 360,
        },
      ],
    },
    {
      lineId: 'piccadilly',
      lineName: 'Piccadilly',
      platformName: 'Westbound - Platform 2',
      direction: 'outbound',
      arrivals: [
        {
          id: '-1416081476',
          destination: 'Heathrow Terminal 5',
          expectedArrival: '2026-10-23T09:02:00Z',
          timeToStationSecs: 120,
          currentLocation: "Between King's Cross St. Pancras and Russell Square",
        },
      ],
    },
  ],
};

/** A TfL line's status, as `/Line/tfl-piccadilly/Status` answers. */
export function tflLineStatus(id: string, name: string, severity = 10, reason = ''): LineStatusReport {
  return {
    $type: 'Tfl.Api.Presentation.Entities.Line, Tfl.Api.Presentation.Entities',
    id: `tfl-${id}`,
    name,
    modeName: 'tube',
    operators: [],
    computedAt: '2026-10-23T08:59:00Z',
    lineStatuses: [
      {
        statusSeverity: severity,
        statusSeverityDescription: severity === 10 ? 'Good Service' : 'Minor Delays',
        reason,
        dataQuality: 'tfl',
        validityPeriods: [],
        sampleAvailability: { state: 'no-coverage' },
        fullCoverageAvailability: { state: 'not-enabled' },
      },
    ],
  };
}
