import { describe, it, expect } from 'vitest';
import { trackedTrainDisplayName } from './trackingName';

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
      'London Kings Cross (KGX) → Edinburgh Waverley (EDB), 10 May 2026 · 14:32',
    );
  });

  it('degrades to date-only when pinScheduledDeparture is absent (TrackedTrainState has no such field)', () => {
    expect(
      trackedTrainDisplayName({ ...base, pinScheduledDeparture: undefined }),
    ).toBe('London Kings Cross (KGX) → Edinburgh Waverley (EDB), 10 May 2026');
  });

  it('falls back to origin-only when there is no destination yet (a pre-match pin)', () => {
    expect(
      trackedTrainDisplayName({ ...base, pinDestinationCrs: null, pinDestinationName: null }),
    ).toBe('London Kings Cross (KGX), 10 May 2026 · 14:32');
  });

  it('falls back to bare CRS codes when no station name resolved', () => {
    expect(
      trackedTrainDisplayName({
        ...base,
        pinOriginName: null,
        pinDestinationName: null,
      }),
    ).toBe('KGX → EDB, 10 May 2026 · 14:32');
  });

  it('an empty-string custom name is treated the same as null (defensive -- the backend never stores one, but this helper does not assume that)', () => {
    expect(trackedTrainDisplayName({ ...base, customName: '' })).toBe(
      'London Kings Cross (KGX) → Edinburgh Waverley (EDB), 10 May 2026 · 14:32',
    );
  });
});
