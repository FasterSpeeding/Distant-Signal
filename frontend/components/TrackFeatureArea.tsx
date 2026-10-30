'use client';

import { useEffect, type MouseEvent, type ReactNode } from 'react';
import { trackFeature } from '@/lib/usageAnalytics';
import type { Feature, FeatureStep } from '@/lib/usageEvents';

/** Counts a feature's part of a page for a visitor who opted in to usage
 * counts (DS-190), around Server Component children: `shown` once when it
 * mounts, and `linkOpened` when a link inside it is followed. Only the
 * step is sent, never which link. Renders its children in a plain `div`. */
export function TrackFeatureArea<F extends Feature>({
  feature,
  shown,
  linkOpened,
  children,
}: {
  feature: F;
  shown?: FeatureStep<F> | undefined;
  linkOpened?: FeatureStep<F> | undefined;
  children: ReactNode;
}) {
  useEffect(() => {
    if (shown !== undefined) trackFeature(feature, shown);
  }, [feature, shown]);

  function handleClick(event: MouseEvent<HTMLDivElement>) {
    if (linkOpened === undefined) return;
    if (event.target instanceof Element && event.target.closest('a[href]')) trackFeature(feature, linkOpened);
  }

  return <div onClickCapture={handleClick}>{children}</div>;
}
