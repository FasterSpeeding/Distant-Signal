'use client';

import { useEffect, useRef } from 'react';
import { usePathname } from 'next/navigation';
import {
  installApiErrorReporting,
  OFFLINE_PENDING_STORAGE_KEY,
  setAnalyticsSession,
  setCurrentRoute,
  track,
  analyticsEnabled,
} from '@/lib/usageAnalytics';
import { OFFLINE_TEMPLATE, routeTemplateFor, type DisplayMode } from '@/lib/usageEvents';

function displayMode(): DisplayMode {
  const nav = navigator as Navigator & { standalone?: boolean };
  const standalone =
    nav.standalone === true ||
    (typeof window.matchMedia === 'function' && window.matchMedia('(display-mode: standalone)').matches);
  return standalone ? 'standalone' : 'browser';
}

/** Sends the opted-in visitor's page views (DS-190; `lib/usageAnalytics.ts`
 * has the rules). Mounted once, beside the nav, with the session the nav
 * was rendered with. Renders nothing.
 *
 * - `page_view` with the route template on every pathname change;
 * - `display_mode` once per page load, with the first page view;
 * - `offline_shown` when `public/offline.html` left its mark;
 * - every failed `/api/*` call as `api_error`
 *   (`installApiErrorReporting`). */
export function UsageTracker({ signedIn, accountOptIn }: { signedIn: boolean; accountOptIn: boolean }) {
  const pathname = usePathname();
  const sentDisplayMode = useRef(false);

  useEffect(() => installApiErrorReporting(), []);

  useEffect(() => {
    setAnalyticsSession(signedIn, accountOptIn);
  }, [signedIn, accountOptIn]);

  useEffect(() => {
    // The session first: the effect above may not have run yet on the
    // very first render.
    setAnalyticsSession(signedIn, accountOptIn);
    const route = routeTemplateFor(pathname);
    setCurrentRoute(route);
    if (!analyticsEnabled()) return;
    track('page_view');
    if (!sentDisplayMode.current) {
      sentDisplayMode.current = true;
      track('display_mode', { mode: displayMode() });
    }
    try {
      if (window.localStorage.getItem(OFFLINE_PENDING_STORAGE_KEY) === '1') {
        window.localStorage.removeItem(OFFLINE_PENDING_STORAGE_KEY);
        track('offline_shown', undefined, OFFLINE_TEMPLATE);
      }
    } catch {
      // Storage blocked: nothing pending.
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps -- only a new page counts: the session props are read above but must not trigger a second page view
  }, [pathname]);

  return null;
}
