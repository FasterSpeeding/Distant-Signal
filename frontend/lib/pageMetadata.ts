import type { Metadata } from 'next';

/** The site's name, the suffix of every page title. */
export const SITE_NAME = 'Distant Signal';

/** What Distant Signal is, in one line: the site-wide meta description
 * and the web app manifest's. */
export const SITE_DESCRIPTION = 'Live UK rail status, train tracking and Delay Repay help.';

/** The root layout's `title.template`: a page sets `title: 'Stations'` and
 * the tab reads "Stations · Distant Signal". */
export const TITLE_TEMPLATE = `%s · ${SITE_NAME}`;

/** A page title with the site name, as the tab shows it. Link previews
 * need it spelled out: Next applies `title.template` to `<title>` only. */
export function fullTitle(title: string): string {
  return TITLE_TEMPLATE.replace('%s', title);
}

/** The site-wide link-preview image, public/opengraph-image.png
 * (1200x630): the wordmark and strapline on the brand background, saved
 * from `lib/previewImage.tsx`'s template with no page title. Listed on
 * every card explicitly: Next merges metadata per top-level field, so a
 * page that sets `openGraph` would drop an image inherited from the root
 * layout. The root layout's `metadataBase` makes the URL absolute. */
export const PREVIEW_IMAGE = {
  url: '/opengraph-image.png',
  width: 1200,
  height: 630,
  alt: 'Distant Signal: live UK rail status',
};

/** For a route with its own `opengraph-image.tsx` (the page's title on the
 * preview template, `lib/previewImage.tsx`): its cards list no image, so
 * Next fills in the route's own. An explicit image would win over it. */
export const OWN_PREVIEW_IMAGE = { ownPreviewImage: true } as const;

/** `robots` for a public page that is worth following but not worth
 * indexing (a train on one day, one incident, a history range): crawlers
 * may fetch it and follow its links, so it must stay out of robots.txt's
 * `Disallow`, or they would never see this tag. */
export const NOINDEX_FOLLOW = { index: false, follow: true } as const;

/** `path` reduced to the bare path a search engine should index: no query
 * string or fragment and no trailing slash (but `/` stays `/`). Every query
 * variant of a page (`/lines/x?dir=up`, `/incidents?severity=...`) names
 * the same canonical. Percent-encoding is kept as given. */
export function canonicalPath(path: string): string {
  const bare = path.split(/[?#]/, 1)[0] ?? '';
  const withSlash = bare.startsWith('/') ? bare : `/${bare}`;
  const trimmed = withSlash.replace(/\/+$/, '');
  return trimmed === '' ? '/' : trimmed;
}

/** A page's own preview image, listed explicitly (an incident's
 * `card.png`): a URL the page computes, so it can carry a version. */
export interface PreviewImage {
  url: string;
  alt: string;
  width: number;
  height: number;
  type: string;
}

export interface PreviewCardOptions {
  /** See `OWN_PREVIEW_IMAGE`. */
  ownPreviewImage?: boolean;
  /** List this image instead of the site-wide one. */
  image?: PreviewImage;
  /** The page's path, as `og:url`; the root layout's `metadataBase` makes
   * it absolute. */
  path?: string;
  /** Alt text for the site-wide preview image naming the page's subject
   * (`Bank on Distant Signal`); default `PREVIEW_IMAGE.alt`. A route with
   * its own image sets its alt in its `opengraph-image.tsx`. */
  imageAlt?: string;
}

/** The Open Graph and Twitter cards for a preview titled `title` (as
 * shown, site name included) and described by `description`. */
export function previewCards(
  title: string,
  description: string,
  { ownPreviewImage = false, image, path, imageAlt }: PreviewCardOptions = {},
): Pick<Metadata, 'openGraph' | 'twitter'> {
  const openGraph = {
    title,
    description,
    type: 'website',
    siteName: SITE_NAME,
    ...(path === undefined ? {} : { url: canonicalPath(path) }),
  } as const;
  const twitter = { card: 'summary_large_image', title, description } as const;
  if (ownPreviewImage) return { openGraph, twitter };
  if (image !== undefined) {
    return {
      openGraph: { ...openGraph, images: [image] },
      twitter: { ...twitter, images: [{ url: image.url, alt: image.alt }] },
    };
  }
  if (imageAlt === undefined) {
    return {
      openGraph: { ...openGraph, images: [PREVIEW_IMAGE] },
      twitter: { ...twitter, images: [PREVIEW_IMAGE.url] },
    };
  }
  return {
    openGraph: { ...openGraph, images: [{ ...PREVIEW_IMAGE, alt: imageAlt }] },
    twitter: { ...twitter, images: [{ url: PREVIEW_IMAGE.url, alt: imageAlt }] },
  };
}

/** The alt text of a page's preview image: what the page is about, on
 * Distant Signal. */
export function previewImageAlt(subject: string): string {
  return `${subject} on ${SITE_NAME}`;
}

export interface PageMetadataOptions extends PreviewCardOptions {
  /** Mark the page `noindex, follow` (`NOINDEX_FOLLOW`). */
  noindex?: boolean;
}

/** A page's title, description and link-preview cards. `title` is the
 * page's own name (its h1); the root template adds the site name. Pass
 * `OWN_PREVIEW_IMAGE` from a route that has its own `opengraph-image.tsx`.
 *
 * `path` (`/stations/EUS`, no query) becomes the canonical URL
 * (`alternates.canonical`) and `og:url`, both made absolute by the root
 * layout's `metadataBase`. Every indexable public page passes it, so each
 * query-string variant of a page names the bare path as its canonical. */
export function pageMetadata(title: string, description: string, options: PageMetadataOptions = {}): Metadata {
  const { noindex = false, ...cardOptions } = options;
  return {
    title,
    description,
    ...(options.path === undefined ? {} : { alternates: { canonical: canonicalPath(options.path) } }),
    ...(noindex ? { robots: NOINDEX_FOLLOW } : {}),
    ...previewCards(fullTitle(title), description, cardOptions),
  };
}

/** Keeps a title that already names the site (the legal pages' "Terms of
 * use · Distant Signal", built in lib/legal.ts) out of the root template,
 * so it isn't suffixed twice. */
export function withAbsoluteTitle(metadata: Metadata): Metadata {
  return typeof metadata.title === 'string' ? { ...metadata, title: { absolute: metadata.title } } : metadata;
}
