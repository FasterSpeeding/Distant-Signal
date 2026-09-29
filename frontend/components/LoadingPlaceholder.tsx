import type { ReactNode } from 'react';
import { Skeleton, Stack, Text } from '@mantine/core';

/** The app's one loading placeholder: a visible "Loading…" label inside a
 * `role="status"` / `aria-busy` region, with an optional skeleton below it
 * to hold the space the real content will take.
 *
 * A bare Mantine `Skeleton` is an empty `div` -- a screen reader gets
 * nothing at all from it, and a sighted user gets a grey box with no word
 * saying it is loading rather than broken. The style guide's loading rule
 * (and the collated UX review) asks for exactly this shape instead, the
 * same one `/lines/[id]`'s own trends/trains fallbacks already use.
 *
 * `height` draws a single full-width skeleton (charts use 320px); pass
 * `children` for a custom skeleton shape instead. Skeletons are
 * `aria-hidden` -- the label is the only thing announced. No hooks, so it
 * works as a `Suspense` fallback in a server component as well as inside a
 * client one. */
export function LoadingPlaceholder({
  label,
  height,
  children,
}: {
  /** Visible and announced, e.g. "Loading trends…". End with "…". */
  label: string;
  height?: number;
  children?: ReactNode;
}) {
  return (
    <Stack gap="xs" role="status" aria-busy="true" data-loading-placeholder>
      <Text size="sm" c="dimmed">
        {label}
      </Text>
      {children !== undefined ? (
        <Stack gap="xs" aria-hidden="true">
          {children}
        </Stack>
      ) : height !== undefined ? (
        <Skeleton height={height} aria-hidden="true" />
      ) : null}
    </Stack>
  );
}
