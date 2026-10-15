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

/** The Open Graph and Twitter cards for a preview titled `title` (as
 * shown, site name included) and described by `description`. */
export function previewCards(
  title: string,
  description: string,
  { ownPreviewImage = false }: { ownPreviewImage?: boolean } = {},
): Pick<Metadata, 'openGraph' | 'twitter'> {
  const openGraph = { title, description, type: 'website', siteName: SITE_NAME } as const;
  const twitter = { card: 'summary_large_image', title, description } as const;
  if (ownPreviewImage) return { openGraph, twitter };
  return {
    openGraph: { ...openGraph, images: [PREVIEW_IMAGE] },
    twitter: { ...twitter, images: [PREVIEW_IMAGE.url] },
  };
}

/** A page's title, description and link-preview cards. `title` is the
 * page's own name (its h1); the root template adds the site name. Pass
 * `OWN_PREVIEW_IMAGE` from a route that has its own `opengraph-image.tsx`. */
export function pageMetadata(
  title: string,
  description: string,
  options: { ownPreviewImage?: boolean } = {},
): Metadata {
  return { title, description, ...previewCards(fullTitle(title), description, options) };
}

/** Keeps a title that already names the site (the legal pages' "Terms of
 * use · Distant Signal", built in lib/legal.ts) out of the root template,
 * so it isn't suffixed twice. */
export function withAbsoluteTitle(metadata: Metadata): Metadata {
  return typeof metadata.title === 'string' ? { ...metadata, title: { absolute: metadata.title } } : metadata;
}
