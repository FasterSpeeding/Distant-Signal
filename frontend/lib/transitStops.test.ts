import { describe, expect, it } from 'vitest';
import { stopPageHref, transitNetworkName, transitStopPlaceName } from './transitStops';

describe('stopPageHref', () => {
  it("links a Metrolink station to its transit stop page, and a TfL stop to TfL's", () => {
    expect(stopPageHref('9400ZZMAPIC')).toBe('/transit/metrolink/stops/9400ZZMAPIC');
    expect(stopPageHref('9400zzmavic')).toBe('/transit/metrolink/stops/9400ZZMAVIC');
    // A London tram stop is a TfL stop.
    expect(stopPageHref('940GZZCRWMB')).toBe('/tfl/stops/940GZZCRWMB');
    expect(stopPageHref('940GZZLUVIC')).toBe('/tfl/stops/940GZZLUVIC');
  });

  it('links nothing else', () => {
    for (const id of [null, undefined, '', 'MAN', '9400ZZXXPIC', 'Home']) expect(stopPageHref(id)).toBeNull();
  });
});

describe('transit stop names', () => {
  it('says the network after the name, in its short form', () => {
    expect(transitStopPlaceName('Piccadilly', transitNetworkName('metrolink'))).toBe('Piccadilly (Metrolink)');
    expect(transitNetworkName('elsewhere')).toBe('tram');
    expect(transitNetworkName('elsewhere', 'Elsewhere Trams')).toBe('Elsewhere Trams');
  });
});
