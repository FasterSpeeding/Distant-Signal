import { useId } from 'react';
import { Group, Text } from '@mantine/core';
import { SIGNAL_MARK } from '@/lib/brand';

/** The signal-arm mark on its grape tile, the favicon's shape
 * (`lib/brand.ts`). Decorative: whatever it sits in carries the name. */
export function SignalMark({ size = 24 }: { size?: number }) {
  // A per-instance gradient id, so two marks on one page can't clash.
  const gradientId = `signal-mark-${useId().replace(/:/g, '')}`;
  const { viewBox, tile, tileGradient, post, arm } = SIGNAL_MARK;
  return (
    <svg width={size} height={size} viewBox={viewBox} aria-hidden="true" focusable="false" className="signalMark">
      <defs>
        <linearGradient id={gradientId} x1="0" y1="0" x2="32" y2="32" gradientUnits="userSpaceOnUse">
          <stop offset="0" stopColor={tileGradient[0]} />
          <stop offset="1" stopColor={tileGradient[1]} />
        </linearGradient>
      </defs>
      <rect x={tile.x} y={tile.y} width={tile.size} height={tile.size} rx={tile.radius} fill={`url(#${gradientId})`} />
      <rect x={post.x} y={post.y} width={post.width} height={post.height} rx={post.radius} fill={post.fill} />
      <polygon fill={arm.fill} points={arm.points} transform={arm.transform} />
    </svg>
  );
}

/** The site's wordmark: the signal arm, then "Distant Signal" in bold
 * type (round 2 decision, 12 Oct 2026). Used in the nav bar; the link
 * previews draw the same pair (`lib/previewImage.tsx`).
 *
 * 24px mark beside 16px/700 type: the nav bar's height is its tallest
 * control (28px), so this keeps the bar at 61px. `data-site-title` is the
 * pride mode hook in `globals.css` (the name turns into the flag's
 * gradient); it stays on the text only, never the mark. Server-renderable. */
export function Wordmark() {
  return (
    <Group gap={8} wrap="nowrap" align="center">
      <SignalMark size={24} />
      <Text fw={700} data-site-title className="ds-nowrap">
        Distant Signal
      </Text>
    </Group>
  );
}
