import { describe, expect, it } from 'vitest';
import { fullTitle, OWN_PREVIEW_IMAGE, pageMetadata, PREVIEW_IMAGE, withAbsoluteTitle } from './pageMetadata';

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

describe('withAbsoluteTitle', () => {
  it('keeps a title that already names the site out of the template', () => {
    expect(withAbsoluteTitle({ title: 'Terms of use · Distant Signal' }).title).toEqual({
      absolute: 'Terms of use · Distant Signal',
    });
  });
});
