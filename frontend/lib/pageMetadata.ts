import type { Metadata } from 'next';

/** The site's name, the suffix of every page title. */
export const SITE_NAME = 'Distant Signal';

/** What Distant Signal is, in one line: the site-wide meta description
 * and the web app manifest's. */
export const SITE_DESCRIPTION = 'Live UK rail status, train tracking and Delay Repay help.';

/** The root layout's `title.template`: a page sets `title: 'Stations'` and
 * the tab reads "Stations · Distant Signal". */
export const TITLE_TEMPLATE = `%s · ${SITE_NAME}`;

/** The link-preview image, public/opengraph-image.png (1200x630): the
 * signal-arm icon and the site name, a placeholder until the wordmark
 * exists. Listed on every card explicitly: Next merges metadata per
 * top-level field, so a page that sets `openGraph` would drop an image
 * inherited from the root layout. The root layout's `metadataBase` makes
 * the URL absolute. */
export const PREVIEW_IMAGE = {
  url: '/opengraph-image.png',
  width: 1200,
  height: 630,
  alt: 'Distant Signal: live UK rail status',
};

/** A page title with the site name, as the tab shows it. Link previews
 * need it spelled out: Next applies `title.template` to `<title>` only. */
export function fullTitle(title: string): string {
  return TITLE_TEMPLATE.replace('%s', title);
}

/** The Open Graph and Twitter cards for a preview titled `title` (as
 * shown, site name included) and described by `description`. */
export function previewCards(title: string, description: string): Pick<Metadata, 'openGraph' | 'twitter'> {
  return {
    openGraph: { title, description, type: 'website', siteName: SITE_NAME, images: [PREVIEW_IMAGE] },
    twitter: { card: 'summary_large_image', title, description, images: [PREVIEW_IMAGE.url] },
  };
}

/** A page's title, description and link-preview cards. `title` is the
 * page's own name (its h1); the root template adds the site name. */
export function pageMetadata(title: string, description: string): Metadata {
  return { title, description, ...previewCards(fullTitle(title), description) };
}

/** Keeps a title that already names the site (the legal pages' "Terms of
 * use — Distant Signal", built in lib/legal.ts) out of the root template,
 * so it isn't suffixed twice. */
export function withAbsoluteTitle(metadata: Metadata): Metadata {
  return typeof metadata.title === 'string' ? { ...metadata, title: { absolute: metadata.title } } : metadata;
}
