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

  it('falls back to a bare origin code with a named destination', () => {
    expect(routeLabel('KGX', null, 'EDB', 'Edinburgh Waverley')).toBe('KGX → Edinburgh Waverley (EDB)');
  });

  it('falls back to a bare destination code with a named origin', () => {
    expect(routeLabel('KGX', 'London Kings Cross', 'EDB', null)).toBe('London Kings Cross (KGX) → EDB');
  });
});
