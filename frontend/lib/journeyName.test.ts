import { describe, expect, it } from 'vitest';
import {
  journeyDefaultName,
  journeyDisplayName,
  journeyNameInputFromLegs,
  journeyPreviewLine,
  journeyRouteLabel,
  NO_LEGS_JOURNEY_NAME,
  type JourneyNameLeg,
} from './journeyName';

function leg(
  originCrs: string | null,
  originName: string | null,
  destinationCrs: string | null,
  destinationName: string | null,
  serviceDate = '2026-09-17',
): JourneyNameLeg {
  return { originCrs, originName, destinationCrs, destinationName, serviceDate };
}

const kgxYrk = leg('KGX', 'London Kings Cross', 'YRK', 'York');
const yrkNcl = leg('YRK', 'York', 'NCL', 'Newcastle', '2026-09-17');
const nclEdb = leg('NCL', 'Newcastle', 'EDB', 'Edinburgh', '2026-09-18');

describe('journey names', () => {
  it('names a single-leg journey by its route and date', () => {
    const input = journeyNameInputFromLegs(null, [kgxYrk]);
    expect(journeyDisplayName(input)).toBe('London Kings Cross (KGX) to York (YRK), 17 Sep 2026');
    expect(journeyPreviewLine(input)).toBeNull();
  });

  it('names a multi-leg journey from the first origin to the last destination, with a leg count', () => {
    const input = journeyNameInputFromLegs(null, [kgxYrk, yrkNcl, nclEdb]);
    expect(journeyDisplayName(input)).toBe('London Kings Cross (KGX) to Edinburgh (EDB), 17 Sep 2026');
    expect(journeyPreviewLine(input)).toBe('3 legs');
  });

  it('uses the first leg date even when a later leg runs the next day', () => {
    const input = journeyNameInputFromLegs(null, [yrkNcl, nclEdb]);
    expect(journeyDefaultName(input)).toBe('York (YRK) to Edinburgh (EDB), 17 Sep 2026');
  });

  it('names an unresolved journey (no station names) with bare codes on both ends', () => {
    const input = journeyNameInputFromLegs(null, [leg('KGX', null, 'YRK', null), leg('YRK', null, 'EDB', null)]);
    expect(journeyDisplayName(input)).toBe('KGX to EDB, 17 Sep 2026');
  });

  it('degrades both ends together when only one end resolved a name', () => {
    const input = journeyNameInputFromLegs(null, [
      leg('KGX', 'London Kings Cross', 'YRK', 'York'),
      leg('YRK', 'York', 'EDB', null),
    ]);
    expect(journeyRouteLabel(input)).toBe('KGX to EDB');
  });

  it('names a journey with no tracking data from its planned legs alone', () => {
    // A planned journey: no leg has a train yet, so all a name has is each
    // leg's own planned ends -- and it still ends at the LAST leg.
    const input = journeyNameInputFromLegs(null, [
      leg('WAT', 'London Waterloo', 'WOK', 'Woking'),
      leg('WOK', 'Woking', 'GLD', 'Guildford'),
      leg('GLD', 'Guildford', 'PMH', 'Portsmouth Harbour'),
    ]);
    expect(journeyDisplayName(input)).toBe('London Waterloo (WAT) to Portsmouth Harbour (PMH), 17 Sep 2026');
  });

  it('shows the custom name verbatim, and the route and date beneath it', () => {
    const input = journeyNameInputFromLegs('Trip north', [kgxYrk, yrkNcl]);
    expect(journeyDisplayName(input)).toBe('Trip north');
    expect(journeyPreviewLine(input)).toBe('London Kings Cross (KGX) to Newcastle (NCL), 17 Sep 2026 · 2 legs');
  });

  it('ignores a blank custom name', () => {
    const input = journeyNameInputFromLegs('   ', [kgxYrk]);
    expect(journeyDisplayName(input)).toBe('London Kings Cross (KGX) to York (YRK), 17 Sep 2026');
    expect(journeyPreviewLine(input)).toBeNull();
  });

  it('names a journey with no legs', () => {
    const input = journeyNameInputFromLegs(null, []);
    expect(journeyDisplayName(input)).toBe(NO_LEGS_JOURNEY_NAME);
    expect(journeyPreviewLine(input)).toBeNull();
    expect(journeyDisplayName(journeyNameInputFromLegs('Holiday', []))).toBe('Holiday');
  });

  it('names a journey whose last leg is cancelled by its planned destination all the same', () => {
    // Cancellation is the status chip's job; the name stays the trip.
    const input = journeyNameInputFromLegs(null, [kgxYrk, yrkNcl]);
    expect(journeyDisplayName(input)).toBe('London Kings Cross (KGX) to Newcastle (NCL), 17 Sep 2026');
  });

  it('names a leg end that has a name but no station code (an untracked leg) by its name', () => {
    const input = journeyNameInputFromLegs(null, [kgxYrk, leg(null, 'Bank', null, 'Canary Wharf')]);
    expect(journeyDisplayName(input)).toBe('London Kings Cross (KGX) to Canary Wharf, 17 Sep 2026');
  });

  it('falls back to "Unknown station" for an end with neither code nor name', () => {
    const input = journeyNameInputFromLegs(null, [leg(null, null, 'YRK', 'York')]);
    expect(journeyRouteLabel(input)).toBe('Unknown station to York (YRK)');
  });
});
