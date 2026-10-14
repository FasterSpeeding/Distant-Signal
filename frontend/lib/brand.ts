/** The signal-arm mark, as data, so the nav wordmark (`components/Wordmark.tsx`)
 * and the link-preview images (`lib/previewImage.tsx`) draw the same shape
 * as the favicon (`app/icon.svg`): a fishtail-notched distant-signal arm,
 * raised to "proceed" on its post, on a grape tile. All coordinates are in
 * the icon's 32x32 box. */
export const SIGNAL_MARK = {
  viewBox: '0 0 32 32',
  tile: { x: 1, y: 1, size: 30, radius: 8 },
  /** Mantine grape 6 to grape 8, top left to bottom right, as the favicon. */
  tileGradient: ['#be4bdb', '#9c36b5'] as const,
  post: { x: 14.6, y: 9, width: 1.8, height: 17, radius: 0.9, fill: '#f8f0fc' },
  /** Mantine yellow 4: distant arms were painted yellow. */
  arm: {
    points: '0,-2.1 7.5,-2.1 10.5,-3.5 8.3,0 10.5,3.5 7.5,2.1 0,2.1',
    transform: 'translate(16.4,12) rotate(-32)',
    fill: '#ffd43b',
  },
} as const;

/** The mark as a standalone SVG document, for an `<img>` data URI where
 * JSX SVG isn't available (the preview images). */
export function signalMarkSvg(): string {
  const { viewBox, tile, tileGradient, post, arm } = SIGNAL_MARK;
  return [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}">`,
    '<defs><linearGradient id="bg" x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse">',
    `<stop offset="0" stop-color="${tileGradient[0]}"/><stop offset="1" stop-color="${tileGradient[1]}"/>`,
    '</linearGradient></defs>',
    `<rect x="${tile.x}" y="${tile.y}" width="${tile.size}" height="${tile.size}" rx="${tile.radius}" fill="url(#bg)"/>`,
    `<rect x="${post.x}" y="${post.y}" width="${post.width}" height="${post.height}" rx="${post.radius}" fill="${post.fill}"/>`,
    `<polygon fill="${arm.fill}" points="${arm.points}" transform="${arm.transform}"/>`,
    '</svg>',
  ].join('');
}
