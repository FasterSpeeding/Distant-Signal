'use client';

import { useLayoutEffect, useRef } from 'react';
import { fullTitle } from '@/lib/pageMetadata';

/** Keeps the browser tab's title honest about a filtered list: `title`
 * ("Lines: Great Western Railway") while something is filtered, the page's
 * own server-rendered title when `title` is `null`.
 *
 * A page's `metadata` is static (a pre-filtered link is not the one people
 * paste), so it cannot know about client-side filter changes; this is the
 * one place that can. The unfiltered title is captured once, on mount, so
 * clearing the filters restores exactly what the server rendered rather
 * than a hand-rolled guess at it.
 *
 * Setting `document.title` once is not enough. The `<title>` in `<head>` is
 * Next's metadata, rendered by React, and React can write that element's
 * text again after this hook has run: a search typed straight after load
 * was set, then put back to the unfiltered title a few milliseconds later
 * by a React text update (seen with a MutationObserver on `<head>`; DS-53,
 * the flaky "tab title follows the search" e2e tests). So while a filter is
 * on, this watches `<head>` and puts the filtered title back whenever
 * something else changes it.
 *
 * A layout effect, not a passive one, so that leaving the page disconnects
 * the observer inside the same commit that renders the next page's title.
 * A passive cleanup runs later, and the observer could first put this
 * page's filtered title over the next page's. */
export function useFilteredDocumentTitle(title: string | null): void {
  const unfilteredTitleRef = useRef<string | null>(null);
  useLayoutEffect(() => {
    unfilteredTitleRef.current ??= document.title;
    const wanted = title ? fullTitle(title) : unfilteredTitleRef.current;
    const apply = () => {
      if (document.title !== wanted) document.title = wanted;
    };
    apply();
    if (!title) return;
    const observer = new MutationObserver(apply);
    observer.observe(document.head, { subtree: true, childList: true, characterData: true });
    return () => observer.disconnect();
  }, [title]);
}
