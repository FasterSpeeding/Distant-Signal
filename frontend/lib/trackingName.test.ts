import { describe, it, expect } from 'vitest';
import { trackedTrainDisplayName, trackedTrainRouteLabel, UNIDENTIFIED_TRAIN_LABEL } from './trackingName';

describe('trackedTrainDisplayName', () => {
  const base = {
    customName: null as string | null,
    pinOriginCrs: 'KGX',
    pinOriginName: 'London Kings Cross' as string | null,
    pinDestinationCrs: 'EDB' as string | null,
    pinDestinationName: 'Edinburgh Waverley' as string | null,
    serviceDate: '2026-05-10',
    pinScheduledDeparture: '2026-05-10T13:32:00Z' as string | undefined,
  };

  it('renders the custom name verbatim when set, ignoring every other field', () => {
    expect(trackedTrainDisplayName({ ...base, customName: 'My commute' })).toBe('My commute');
  });

  it('falls back to route + date + time when there is no custom name and a departure time is present', () => {
    expect(trackedTrainDisplayName(base)).toBe(
      'London Kings Cross (KGX) to Edinburgh Waverley (EDB), 10 May 2026 · 14:32',
    );
  });

  it('degrades to date-only when pinScheduledDeparture is absent (TrackedTrainState has no such field)', () => {
    expect(trackedTrainDisplayName({ ...base, pinScheduledDeparture: undefined })).toBe(
      'London Kings Cross (KGX) to Edinburgh Waverley (EDB), 10 May 2026',
    );
  });

  it('falls back to origin-only when there is no destination yet (a pre-match pin)', () => {
    expect(trackedTrainDisplayName({ ...base, pinDestinationCrs: null, pinDestinationName: null })).toBe(
      'London Kings Cross (KGX), 10 May 2026 · 14:32',
    );
  });

  it('falls back to bare CRS codes when no station name resolved', () => {
    expect(
      trackedTrainDisplayName({
        ...base,
        pinOriginName: null,
        pinDestinationName: null,
      }),
    ).toBe('KGX to EDB, 10 May 2026 · 14:32');
  });

  it('an empty-string custom name is treated the same as null (defensive -- the backend never stores one, but this helper does not assume that)', () => {
    expect(trackedTrainDisplayName({ ...base, customName: '' })).toBe(
      'London Kings Cross (KGX) to Edinburgh Waverley (EDB), 10 May 2026 · 14:32',
    );
  });
});

// DS-111: a train tracked by UID before its timetable arrived has no route.
// That is a normal state, so it is never labelled "Unknown station".
describe('trackedTrainRouteLabel', () => {
  const noRoute = {
    pinOriginCrs: null,
    pinOriginName: null,
    pinDestinationCrs: null as string | null,
    pinDestinationName: null as string | null,
  };

  it('uses the route when there is an origin', () => {
    expect(
      trackedTrainRouteLabel({ ...noRoute, pinOriginCrs: 'KGX', pinOriginName: 'London Kings Cross', trainUid: 'W1' }),
    ).toBe('London Kings Cross (KGX)');
  });

  it('names the train by UID when there is no origin', () => {
    expect(trackedTrainRouteLabel({ ...noRoute, trainUid: 'W12345' })).toBe('Train W12345');
  });

  it('keeps a known destination', () => {
    expect(
      trackedTrainRouteLabel({ ...noRoute, pinDestinationCrs: 'YRK', pinDestinationName: 'York', trainUid: 'W12345' }),
    ).toBe('Train W12345 to York (YRK)');
  });

  it('says the train is not identified yet when there is neither a route nor a UID', () => {
    expect(trackedTrainRouteLabel({ ...noRoute, trainUid: null })).toBe(UNIDENTIFIED_TRAIN_LABEL);
    expect(trackedTrainRouteLabel(noRoute)).toBe('Train not identified yet');
  });

  it('feeds trackedTrainDisplayName, so the default name is never "Unknown station"', () => {
    expect(
      trackedTrainDisplayName({ ...noRoute, customName: null, serviceDate: '2026-10-21', trainUid: 'W12345' }),
    ).toBe('Train W12345, 21 Oct 2026');
  });
});
