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
  /** Mantine yellow 4: distant arms were painted yellow. Drawn level from
   * its pivot on the post, then turned up by `angle` degrees: `transform`
   * is those two as SVG, which `signalMarkBounds` reads back as numbers. */
  arm: {
    points: '0,-2.1 7.5,-2.1 10.5,-3.5 8.3,0 10.5,3.5 7.5,2.1 0,2.1',
    pivot: { x: 16.4, y: 12 },
    angle: -32,
    transform: 'translate(16.4,12) rotate(-32)',
    fill: '#ffd43b',
  },
} as const;

/** A box in the icon's 32x32 coordinates. */
export interface MarkBox {
  x: number;
  y: number;
  width: number;
  height: number;
}

function boxOf(xs: number[], ys: number[]): MarkBox {
  const x = Math.min(...xs);
  const y = Math.min(...ys);
  return { x, y, width: Math.max(...xs) - x, height: Math.max(...ys) - y };
}

/** The raised arm's extent: its points turned by `angle` about the pivot. */
export function signalArmBounds(): MarkBox {
  const { arm } = SIGNAL_MARK;
  const radians = (arm.angle * Math.PI) / 180;
  const cos = Math.cos(radians);
  const sin = Math.sin(radians);
  const xs: number[] = [];
  const ys: number[] = [];
  for (const point of arm.points.split(' ')) {
    const [px = 0, py = 0] = point.split(',').map(Number);
    xs.push(arm.pivot.x + px * cos - py * sin);
    ys.push(arm.pivot.y + px * sin + py * cos);
  }
  return boxOf(xs, ys);
}

/** The post and the raised arm's true extent, without the tile. The arm's
 * fishtail tip rises to y ~3.5, well above the post's top (y 9), so a box
 * guessed from the post alone slices the tip off flat. */
export function signalMarkBounds(): MarkBox {
  const { post } = SIGNAL_MARK;
  const arm = signalArmBounds();
  return boxOf(
    [post.x, post.x + post.width, arm.x, arm.x + arm.width],
    [post.y, post.y + post.height, arm.y, arm.y + arm.height],
  );
}

/** The mark as a standalone SVG document, for an `<img>` data URI where
 * JSX SVG isn't available (the preview images). `tile: false` leaves out
 * the grape tile, for use on the grape background itself. */
export function signalMarkSvg({ tile: withTile = true }: { tile?: boolean } = {}): string {
  const { viewBox, tile, tileGradient, post, arm } = SIGNAL_MARK;
  return [
    `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}">`,
    ...(withTile
      ? [
          '<defs><linearGradient id="bg" x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse">',
          `<stop offset="0" stop-color="${tileGradient[0]}"/><stop offset="1" stop-color="${tileGradient[1]}"/>`,
          '</linearGradient></defs>',
          `<rect x="${tile.x}" y="${tile.y}" width="${tile.size}" height="${tile.size}" rx="${tile.radius}" fill="url(#bg)"/>`,
        ]
      : []),
    `<rect x="${post.x}" y="${post.y}" width="${post.width}" height="${post.height}" rx="${post.radius}" fill="${post.fill}"/>`,
    `<polygon fill="${arm.fill}" points="${arm.points}" transform="${arm.transform}"/>`,
    '</svg>',
  ].join('');
}
