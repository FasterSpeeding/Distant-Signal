import { describe, expect, it } from 'vitest';
import { homeJsonLd, serializeJsonLd, stationJsonLd } from './jsonLd';

describe('serializeJsonLd', () => {
  it('escapes everything that could close the script element or open a comment', () => {
    const text = serializeJsonLd({ '@type': 'Thing', name: '</script><script>alert(1)</script><!-- & \u2028' });
    expect(text).not.toMatch(/[<>&\u2028\u2029]/);
    expect(text).toContain('\\u003c/script\\u003e');
  });

  it('round-trips through JSON.parse unchanged', () => {
    const data = { '@type': 'Thing', name: 'A <b> & "c" \u2029' };
    expect(JSON.parse(serializeJsonLd(data))).toEqual(data);
  });
});

describe('stationJsonLd', () => {
  it('describes the station by name and code, with breadcrumbs', () => {
    const [station, crumbs] = stationJsonLd({ origin: 'https://ds.example', crs: 'EUS', name: 'London Euston' });
    expect(station).toEqual({
      '@context': 'https://schema.org',
      '@type': 'TrainStation',
      name: 'London Euston',
      identifier: 'EUS',
      url: 'https://ds.example/stations/EUS',
    });
    expect(crumbs?.['@type']).toBe('BreadcrumbList');
    expect(crumbs?.itemListElement).toEqual([
      { '@type': 'ListItem', position: 1, name: 'Distant Signal', item: 'https://ds.example/' },
      { '@type': 'ListItem', position: 2, name: 'Stations', item: 'https://ds.example/stations' },
      { '@type': 'ListItem', position: 3, name: 'London Euston', item: 'https://ds.example/stations/EUS' },
    ]);
  });

  it('adds coordinates when known', () => {
    const [station] = stationJsonLd({
      origin: 'https://ds.example',
      crs: 'EUS',
      name: 'London Euston',
      geo: { latitude: 51.528, longitude: -0.134 },
    });
    expect(station?.geo).toEqual({ '@type': 'GeoCoordinates', latitude: 51.528, longitude: -0.134 });
  });
});

describe('homeJsonLd', () => {
  it('is a WebSite and an Organization at the origin', () => {
    const [site, org] = homeJsonLd('https://ds.example');
    expect(site).toMatchObject({ '@type': 'WebSite', name: 'Distant Signal', url: 'https://ds.example/' });
    expect(org).toMatchObject({ '@type': 'Organization', logo: 'https://ds.example/icon-512.png' });
  });
});
