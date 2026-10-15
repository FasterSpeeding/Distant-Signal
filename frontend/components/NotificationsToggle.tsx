'use client';

import { useEffect, useRef, useState, type RefObject } from 'react';
import { Button, Text } from '@mantine/core';
import { failureFromResponse } from '@/lib/failure';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import {
  EnableNotificationsError,
  GENERIC_ENABLE_ERROR,
  SERVICE_WORKER_TIMEOUT_MS,
  blockedMessage,
  errorName,
  failureReference,
  fetchVapidKey,
  logEnableFailure,
  pushSupported,
  requestNotificationPermission,
  subscribeToPush,
  unsupportedMessage,
  withTimeout,
} from './enableNotifications';

export { urlBase64ToUint8Array } from './enableNotifications';

/** A shown failure: the sentence, and for an unexpected failure a short
 * reference code naming the step (`failureReference`). */
interface Failure {
  message: string;
  reference?: string;
}

/** Global "Enable notifications" control (Decision 6) -- not per-line,
 * since Decision 5 reuses pinned_lines/tracked_trains directly as scope.
 * Renders for every visitor (Tier 2, per docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md),
 * gated on browser capability, not install state (Decision 1 -- Android
 * and every desktop browser get real push from a bare open tab; only iOS
 * requires Home Screen install, which this component doesn't require: on
 * iOS outside the home screen, Safari has no `PushManager`, so the button
 * stays disabled and a line under it says to add the site to the home
 * screen).
 *
 * The click flow and why its order matters (Firefox for Android) are in
 * `enableNotifications.ts`.
 *
 * Mirrors `PinToggle.tsx`'s established `useNeedsLogin()`/`LoginPromptModal`
 * Tier-2 shape exactly -- same anonymous-user-UX pattern, just triggered
 * from a click that first does browser-side `PushManager.subscribe()` work
 * instead of a straight fetch. */
export function NotificationsToggle() {
  // `checked` and `supported` are deliberately separate: `supported`
  // starts `false` (the same value it would settle on in an unsupported
  // browser), so collapsing them into one flag would make the pre-check
  // and "genuinely unsupported" states indistinguishable, and this
  // component render nothing for both -- which is exactly the review
  // §2.11 bug (button pops in after hydration). Reserving the slot instead
  // means the button is in the DOM from the very first (server) render,
  // `disabled` until the capability check resolves on mount; a supported
  // browser's button then just becomes clickable in place.
  const [checked, setChecked] = useState(false);
  const [supported, setSupported] = useState(false);
  const [unsupportedNote, setUnsupportedNote] = useState<string | null>(null);
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [failure, setFailure] = useState<Failure | null>(null);
  const needsLoginState = useNeedsLogin();
  // The VAPID key, fetched on mount rather than after the click, so
  // `subscribe()` can follow the permission answer with no network round
  // trip in between (see enableNotifications.ts). Null after a failed
  // fetch, which the click then retries.
  const vapidKeyRef = useRef<Promise<string> | null>(null);

  useEffect(() => {
    const isSupported = pushSupported();
    // eslint-disable-next-line react-hooks/set-state-in-effect -- feature detection needs navigator/window, which exist only after mount
    setSupported(isSupported);
    setChecked(true);
    if (!isSupported) {
      setUnsupportedNote(unsupportedMessage(navigator));
      return;
    }

    void prefetchVapidKey(vapidKeyRef);

    // Seed `enabled` from the REAL subscription state, not just an assumed
    // `false` -- without this a returning subscriber sees "Enable
    // notifications" on every visit. `getRegistration()` (not `.ready`,
    // which hangs until a service worker is active) resolves to
    // `undefined` immediately when there is none yet. Optional-chained
    // since a partial or stubbed `serviceWorker` may not expose it.
    let cancelled = false;
    const serviceWorker: Partial<Pick<ServiceWorkerContainer, 'getRegistration'>> = navigator.serviceWorker;
    const registrationPromise = serviceWorker.getRegistration?.();
    registrationPromise
      ?.then((registration) => registration?.pushManager.getSubscription())
      .then((subscription) => {
        if (!cancelled && subscription) {
          setEnabled(true);
        }
      })
      .catch(() => {
        // Nothing to seed -- falls back to "not yet enabled".
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function finishEnable(permissionRequest: Promise<NotificationPermission>) {
    try {
      let permission: NotificationPermission;
      try {
        permission = await permissionRequest;
      } catch (error) {
        throw new EnableNotificationsError('permission', error);
      }
      if (permission === 'denied') {
        logEnableFailure(
          new EnableNotificationsError('permission', new DOMException('Permission is denied', 'NotAllowedError')),
        );
        setFailure({ message: blockedMessage(navigator.userAgent) });
        return;
      }
      if (permission !== 'granted') {
        // The prompt was dismissed: nothing to report.
        return;
      }

      // Both were started before the click, so these normally settle at
      // once and `subscribe()` runs while the tap still counts as one.
      const [key, registration] = await Promise.all([vapidKey(vapidKeyRef), serviceWorkerRegistration()]);
      const subscriptionJson = (await subscribeToPush(registration, key)).toJSON();

      let response: Response;
      try {
        response = await fetch('/api/notifications/subscribe', {
          method: 'POST',
          headers: { 'Content-Type': 'application/json' },
          body: JSON.stringify({ endpoint: subscriptionJson.endpoint, keys: subscriptionJson.keys }),
        });
      } catch (error) {
        throw new EnableNotificationsError('save', error);
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (!response.ok) {
        const error = new EnableNotificationsError('save', new Error(`subscribe returned HTTP ${response.status}`));
        logEnableFailure(error);
        setFailure({
          message: await failureFromResponse('turn on', 'notifications', response),
          reference: failureReference(error),
        });
        return;
      }
      setEnabled(true);
    } catch (error) {
      logEnableFailure(error);
      setFailure(describeEnableFailure(error));
    }
  }

  function enable() {
    // `requestPermission()` comes first, synchronously, before any await
    // or state update can yield: Firefox only allows the prompt during the
    // tap's user activation.
    const permissionRequest = requestNotificationPermission();
    setFailure(null);
    needsLoginState.reset();
    setBusy(true);
    void finishEnable(permissionRequest).finally(() => setBusy(false));
  }

  // No `if (!supported) return null` branch: that's the reserved slot
  // above. An unsupported browser keeps a permanently `disabled` button,
  // with a line under it saying why.
  return (
    <>
      <Button
        onClick={enable}
        disabled={!checked || !supported || busy || enabled}
        variant={enabled ? 'light' : 'filled'}
      >
        {enabled ? 'Notifications enabled' : 'Enable notifications'}
      </Button>
      {unsupportedNote && (
        <Text size="sm" c="dimmed">
          {unsupportedNote}
        </Text>
      )}
      {failure && (
        <div role="alert">
          <Text c="var(--ds-color-error-text)">{failure.message}</Text>
          {failure.reference && (
            <Text size="xs" c="dimmed">
              Reference: {failure.reference}
            </Text>
          )}
        </div>
      )}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to enable notifications.
      </LoginPromptModal>
    </>
  );
}

/** Starts fetching the VAPID key into `ref`. A failed fetch clears `ref`,
 * so the click fetches again. */
function prefetchVapidKey(ref: RefObject<Promise<string> | null>): Promise<string> {
  const key = fetchVapidKey();
  ref.current = key;
  key.catch(() => {
    if (ref.current === key) ref.current = null;
  });
  return key;
}

/** The prefetched VAPID key, or a fresh fetch when there is none or it
 * failed. */
function vapidKey(ref: RefObject<Promise<string> | null>): Promise<string> {
  const cached = ref.current;
  const key = cached ? cached.catch(() => prefetchVapidKey(ref)) : prefetchVapidKey(ref);
  return key.catch((error: unknown) => {
    throw new EnableNotificationsError('key', error);
  });
}

/** The active service worker registration, or a `worker` failure when none
 * becomes active in time. */
function serviceWorkerRegistration(): Promise<ServiceWorkerRegistration> {
  return withTimeout(navigator.serviceWorker.ready, SERVICE_WORKER_TIMEOUT_MS, 'serviceWorker.ready').catch(
    (error: unknown) => {
      throw new EnableNotificationsError('worker', error);
    },
  );
}

/** The message for a failure thrown out of the click flow:
 *
 * - no Notification API at all: say the browser can't;
 * - a `NotAllowedError` while permission really is denied (Firefox also
 *   rejects `subscribe()` this way when push is blocked for the site):
 *   the "blocked" steps;
 * - anything else, including Firefox's `NotAllowedError` from a lost user
 *   activation while permission is granted, which a second tap fixes:
 *   "Couldn't turn on notifications. Try again." with a reference code. */
function describeEnableFailure(error: unknown): Failure {
  if (typeof Notification === 'undefined') {
    return { message: unsupportedMessage(navigator) };
  }
  const cause = error instanceof EnableNotificationsError ? error.cause : error;
  if (errorName(cause) === 'NotAllowedError' && Notification.permission === 'denied') {
    return { message: blockedMessage(navigator.userAgent) };
  }
  return { message: GENERIC_ENABLE_ERROR, reference: failureReference(error) };
}
