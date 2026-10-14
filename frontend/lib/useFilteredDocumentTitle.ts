'use client';

import { useEffect, useRef } from 'react';
import { fullTitle } from '@/lib/pageMetadata';

/** Keeps the browser tab's title honest about a filtered list: `title`
 * ("Lines: Great Western Railway") while something is filtered, the page's
 * own server-rendered title when `title` is `null`.
 *
 * A page's `metadata` is static (a pre-filtered link is not the one people
 * paste), so it cannot know about client-side filter changes; this is the
 * one place that can. The unfiltered title is captured once, on mount, so
 * clearing the filters restores exactly what the server rendered rather
 * than a hand-rolled guess at it. */
export function useFilteredDocumentTitle(title: string | null): void {
  const unfilteredTitleRef = useRef<string | null>(null);
  useEffect(() => {
    unfilteredTitleRef.current ??= document.title;
    document.title = title ? fullTitle(title) : unfilteredTitleRef.current;
  }, [title]);
}
