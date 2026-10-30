import { describe, expect, it } from 'vitest';
import { stationDeparturesHref } from './stationDepartures';

describe('stationDeparturesHref', () => {
  it.each([
    ['EUS', '/trains?station=EUS'],
    ['KGX', '/trains?station=KGX'],
    ['eus', '/trains?station=EUS'],
    ['Rdg', '/trains?station=RDG'],
  ])('links %s to the train search for that station', (crs, href) => {
    expect(stationDeparturesHref(crs)).toBe(href);
  });
});
