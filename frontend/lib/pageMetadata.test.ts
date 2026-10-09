import { describe, expect, it } from 'vitest';
import { fullTitle, pageMetadata, PREVIEW_IMAGE, withAbsoluteTitle } from './pageMetadata';

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
    expect(metadata.twitter).toMatchObject({
      card: 'summary_large_image',
      title: 'Stations · Distant Signal',
      images: [PREVIEW_IMAGE.url],
    });
  });
});

describe('withAbsoluteTitle', () => {
  it('keeps a title that already names the site out of the template', () => {
    expect(withAbsoluteTitle({ title: 'Terms of use — Distant Signal' }).title).toEqual({
      absolute: 'Terms of use — Distant Signal',
    });
  });
});
