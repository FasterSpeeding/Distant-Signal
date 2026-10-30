'use client';

import { useEffect } from 'react';
import { trackEmptyState } from '@/lib/usageAnalytics';
import type { EmptyStateKind } from '@/lib/usageEvents';

/** Counts an empty or no-result state once when it is shown, for a visitor
 * who opted in to usage counts (DS-190). Renders nothing, so a Server
 * Component can put it beside its own empty-state copy. */
export function TrackEmptyState({ kind }: { kind: EmptyStateKind }) {
  useEffect(() => {
    trackEmptyState(kind);
  }, [kind]);
  return null;
}
