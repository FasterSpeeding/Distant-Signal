import { describe, expect, it } from 'vitest';
import {
  canonicalPath,
  fullTitle,
  NOINDEX_FOLLOW,
  OWN_PREVIEW_IMAGE,
  pageMetadata,
  PREVIEW_IMAGE,
  previewImageAlt,
  withAbsoluteTitle,
} from './pageMetadata';

describe('pageMetadata', () => {
  it('leaves the site name to the root template, and spells it out on the preview cards', () => {
    const metadata = pageMetadata('Stations', 'Look up a UK station.');
    expect(metadata.title).toBe('Stations');
    expect(fullTitle('Stations')).toBe('Stations · Distant Signal');
    expect(metadata.openGraph).toMatchObject({
      title: 'Stations · Distant Signal',
      description: 'Look up a UK station.',
      siteName: 'Distant Signal',
      images: [PREVIEW_IMAGE],
    });
    expect(metadata.twitter).toMatchObject({ images: [PREVIEW_IMAGE.url] });
  });

  it('lists no image for a route with its own preview, so Next fills in its own', () => {
    const metadata = pageMetadata('Status', 'Live status.', OWN_PREVIEW_IMAGE);
    expect(metadata.openGraph).not.toHaveProperty('images');
    expect(metadata.twitter).not.toHaveProperty('images');
    expect(metadata.twitter).toMatchObject({
      card: 'summary_large_image',
      title: 'Status · Distant Signal',
    });
  });
});

describe('canonical URLs', () => {
  it.each([
    ['/stations/EUS', '/stations/EUS'],
    ['/lines/wcml?dir=up&at=0900', '/lines/wcml'],
    ['/incidents?severity=6#top', '/incidents'],
    ['/transit/', '/transit'],
    ['/', '/'],
    ['?q=x', '/'],
    ['stations', '/stations'],
  ])('canonicalPath(%s) is %s', (path, expected) => {
    expect(canonicalPath(path)).toBe(expected);
  });

  it('sets the canonical and og:url from the path, without its query', () => {
    const metadata = pageMetadata('Northern', 'd', { ...OWN_PREVIEW_IMAGE, path: '/lines/tfl-northern?dir=up' });
    expect(metadata.alternates).toEqual({ canonical: '/lines/tfl-northern' });
    expect(metadata.openGraph).toMatchObject({ url: '/lines/tfl-northern' });
  });

  it('sets neither without a path', () => {
    const metadata = pageMetadata('Northern', 'd');
    expect(metadata).not.toHaveProperty('alternates');
    expect(metadata.openGraph).not.toHaveProperty('url');
  });
});

describe('noindex and image alt', () => {
  it('marks a page noindex but followable', () => {
    expect(pageMetadata('Train', 'd', { noindex: true }).robots).toEqual(NOINDEX_FOLLOW);
    expect(NOINDEX_FOLLOW).toEqual({ index: false, follow: true });
    expect(pageMetadata('Train', 'd')).not.toHaveProperty('robots');
  });

  it("names the page's subject in the preview image's alt text", () => {
    const metadata = pageMetadata('Bank', 'd', { imageAlt: previewImageAlt('Bank') });
    expect(metadata.openGraph).toMatchObject({ images: [{ ...PREVIEW_IMAGE, alt: 'Bank on Distant Signal' }] });
    expect(metadata.twitter).toMatchObject({ images: [{ url: PREVIEW_IMAGE.url, alt: 'Bank on Distant Signal' }] });
  });
});

describe('withAbsoluteTitle', () => {
  it('keeps a title that already names the site out of the template', () => {
    expect(withAbsoluteTitle({ title: 'Terms of use · Distant Signal' }).title).toEqual({
      absolute: 'Terms of use · Distant Signal',
    });
  });
});
