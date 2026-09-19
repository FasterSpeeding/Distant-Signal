import { describe, it, expect } from 'vitest';
import { stationLabel, routeLabel } from './stationLabel';

describe('stationLabel', () => {
  it('renders "Name (CRS)" when a name resolved', () => {
    expect(stationLabel('KGX', 'London Kings Cross')).toBe('London Kings Cross (KGX)');
  });

  it('falls back to the bare code when name is null', () => {
    expect(stationLabel('KGX', null)).toBe('KGX');
  });

  it('falls back to the bare code when name is undefined', () => {
    expect(stationLabel('KGX', undefined)).toBe('KGX');
  });
});

describe('routeLabel', () => {
  it('renders both ends with names when both resolved', () => {
    expect(routeLabel('KGX', 'London Kings Cross', 'EDB', 'Edinburgh Waverley')).toBe(
      'London Kings Cross (KGX) → Edinburgh Waverley (EDB)',
    );
  });

  it('renders just the origin when there is no destination (a pre-match pin)', () => {
    expect(routeLabel('KGX', 'London Kings Cross', null, null)).toBe('London Kings Cross (KGX)');
  });

  it('renders just the origin when destination is undefined', () => {
    expect(routeLabel('KGX', 'London Kings Cross', undefined, undefined)).toBe('London Kings Cross (KGX)');
  });

  it('falls back to bare codes on both ends when neither name resolved', () => {
    expect(routeLabel('KGX', null, 'EDB', null)).toBe('KGX → EDB');
  });

  // Both of the next two used to mix forms -- one end got "Name (CODE)",
  // the other a bare code -- which read as a data error rather than a
  // degraded lookup (review §2.9). Now that only one end's name is
  // unresolved forces BOTH ends to bare codes, so the string is never
  // internally inconsistent.
  it('falls back to bare codes on both ends when only the origin name is unresolved', () => {
    expect(routeLabel('KGX', null, 'EDB', 'Edinburgh Waverley')).toBe('KGX → EDB');
  });

  it('falls back to bare codes on both ends when only the destination name is unresolved', () => {
    expect(routeLabel('KGX', 'London Kings Cross', 'EDB', null)).toBe('KGX → EDB');
  });

  // Fix 2 (review finding C2): an NR-primary subscription whose shared
  // train has no schedule data yet genuinely has no origin CRS at all --
  // not merely no origin *name*.
  it('renders a placeholder when the origin CRS itself is null', () => {
    expect(routeLabel(null, null, null, null)).toBe('Unknown station');
  });

  it('still renders a known destination when the origin CRS is null', () => {
    expect(routeLabel(null, null, 'EDB', 'Edinburgh Waverley')).toBe(
      'Unknown station → Edinburgh Waverley (EDB)',
    );
  });
});
