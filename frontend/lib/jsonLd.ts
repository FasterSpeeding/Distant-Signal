import { SITE_DESCRIPTION, SITE_NAME } from './pageMetadata';

/** Structured data (schema.org JSON-LD) for search engines, rendered
 * server-side by `components/JsonLd.tsx`. Kept minimal: what a page is,
 * never its live state, which would go stale in a search index. */
export type JsonLdObject = { '@context'?: string; '@type': string } & Record<string, unknown>;

/** `data` as the text of a `<script type="application/ld+json">`. JSON
 * alone can still close the script element (a `</script>` inside a station
 * name) or open an HTML comment, so `<`, `>` and `&` become `\u` escapes,
 * which JSON parsers read back as the same characters. U+2028 and U+2029
 * are escaped too: valid in JSON, but line terminators to older JavaScript
 * parsers. */
export function serializeJsonLd(data: JsonLdObject | JsonLdObject[]): string {
  return JSON.stringify(data)
    .replace(/</g, '\\u003c')
    .replace(/>/g, '\\u003e')
    .replace(/&/g, '\\u0026')
    .replace(/\u2028/g, '\\u2028')
    .replace(/\u2029/g, '\\u2029');
}

const CONTEXT = 'https://schema.org';

/** `path` on the site's `origin` (`https://host`, no trailing slash). */
function absolute(origin: string, path: string): string {
  return `${origin}${path}`;
}

/** The home page's `WebSite` and `Organization`. */
export function homeJsonLd(origin: string): JsonLdObject[] {
  const url = absolute(origin, '/');
  return [
    {
      '@context': CONTEXT,
      '@type': 'WebSite',
      name: SITE_NAME,
      url,
      description: SITE_DESCRIPTION,
      inLanguage: 'en-GB',
    },
    {
      '@context': CONTEXT,
      '@type': 'Organization',
      name: SITE_NAME,
      url,
      logo: absolute(origin, '/icon-512.png'),
    },
  ];
}

export interface StationJsonLdInput {
  origin: string;
  crs: string;
  name: string;
  /** Omitted when the station has no known coordinates. */
  geo?: { latitude: number; longitude: number };
}

/** A station page's `TrainStation` and its `BreadcrumbList`
 * (Home, Stations, the station). */
export function stationJsonLd({ origin, crs, name, geo }: StationJsonLdInput): JsonLdObject[] {
  const path = `/stations/${encodeURIComponent(crs)}`;
  return [
    {
      '@context': CONTEXT,
      '@type': 'TrainStation',
      name,
      identifier: crs,
      url: absolute(origin, path),
      ...(geo ? { geo: { '@type': 'GeoCoordinates', latitude: geo.latitude, longitude: geo.longitude } } : {}),
    },
    {
      '@context': CONTEXT,
      '@type': 'BreadcrumbList',
      itemListElement: [
        { '@type': 'ListItem', position: 1, name: SITE_NAME, item: absolute(origin, '/') },
        { '@type': 'ListItem', position: 2, name: 'Stations', item: absolute(origin, '/stations') },
        { '@type': 'ListItem', position: 3, name, item: absolute(origin, path) },
      ],
    },
  ];
}
