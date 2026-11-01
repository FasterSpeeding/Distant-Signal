import { describe, expect, it } from 'vitest';
import { untrackedLegPlaceNames, untrackedLegPlaces } from './untrackedLeg';

const walk = (originName: string, destinationName: string) => ({
  kind: 'untracked' as const,
  transitMode: 'walk' as const,
  originName,
  destinationName,
  originCrs: null,
  destinationCrs: null,
});
const train = (originCrs: string, destinationCrs: string) => ({
  kind: 'train' as const,
  originName: null,
  destinationName: null,
  originCrs,
  destinationCrs,
});
const ride = (transitMode: 'tube' | 'dlr' | 'tram', originStopId: string | null = null) => ({
  kind: 'untracked' as const,
  transitMode,
  originName: 'x',
  destinationName: 'y',
  originStopId,
  destinationStopId: null,
});

/** DS-239: a saved leg whose names read alike says what each end is when
 * shown, from what the journey knows; nothing is stored. */
describe('untrackedLegPlaceNames', () => {
  it('a walk off a train onto the Tube', () => {
    expect(
      untrackedLegPlaces(walk('Vauxhall', 'Vauxhall'), {
        previous: train('MKC', 'VXH'),
        next: ride('tube', '940GZZLUVXL'),
      }),
    ).toBe('Vauxhall (National Rail) to Vauxhall (Tube)');
  });

  it('a walk off the DLR onto a train, from the ride mode alone', () => {
    expect(
      untrackedLegPlaces(walk('Stratford', 'Stratford'), { previous: ride('dlr'), next: train('SRA', 'LST') }),
    ).toBe('Stratford (DLR) to Stratford (National Rail)');
  });

  it('a walk off a train onto a tram', () => {
    expect(
      untrackedLegPlaces(walk('London Wimbledon', 'Wimbledon'), { previous: train('WAT', 'WIM'), next: ride('tram') }),
    ).toBe('London Wimbledon (National Rail) to Wimbledon (tram)');
  });

  it('tags the one end the journey tells', () => {
    expect(untrackedLegPlaces(walk('Vauxhall', 'Vauxhall'), { previous: train('MKC', 'VXH') })).toBe(
      'Vauxhall (National Rail) to Vauxhall',
    );
  });

  it('is as saved with nothing to tell, with names that differ, or with tags already', () => {
    expect(untrackedLegPlaces(walk('Vauxhall', 'Vauxhall'))).toBe('Vauxhall to Vauxhall');
    expect(untrackedLegPlaces(walk('London Euston', 'London Kings Cross'), { previous: train('MKC', 'EUS') })).toBe(
      'London Euston to London Kings Cross',
    );
    expect(
      untrackedLegPlaceNames(walk('Vauxhall (National Rail)', 'Vauxhall (Tube)'), { previous: train('MKC', 'VXH') }),
    ).toEqual({ originName: 'Vauxhall (National Rail)', destinationName: 'Vauxhall (Tube)' });
  });

  it('one end only reads as before', () => {
    expect(untrackedLegPlaces({ originName: 'Vauxhall', destinationName: null })).toBe('From Vauxhall');
  });
});
