'use client';

import { useEffect, useState } from 'react';
import { Button, Text } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';

/** Shown for any failure in `enable()` -- a rejected `pushManager.subscribe()`
 * (common on Firefox/Brave with push disabled at the OS/browser level, or a
 * malformed VAPID key), a denied `Notification.requestPermission()` prompt
 * the user backs out of via the browser chrome rather than an explicit
 * "Block", or a network failure on either `fetch()`. Deliberately generic
 * rather than the raw `Error#message` -- those come from `DOMException`s
 * whose wording ("The user denied permission for the notification" /
 * "Registration failed - push service error", depending on browser) is not
 * written for an end user, and other browsers throw with no message text at
 * all. */
const ENABLE_ERROR_MESSAGE = "Couldn't enable notifications. Check your browser's notification permissions.";

/** Converts the VAPID public key (base64url, as returned by
 * `GET /public/notifications/vapid-public-key`) into the raw
 * `Uint8Array` form `PushManager.subscribe({ applicationServerKey })`
 * expects. Newer browsers accept the base64url string directly per the
 * Push API spec's `(BufferSource or DOMString)` union, but Safari/WebKit
 * has historically required the `BufferSource` form -- converting always
 * is the one call shape that works across every supported browser,
 * so this is not a browser-conditional fallback, just the safe default.
 * Exported standalone so it's unit-testable without any Web Push globals. */
export function urlBase64ToUint8Array(base64String: string): Uint8Array<ArrayBuffer> {
  const padding = '='.repeat((4 - (base64String.length % 4)) % 4);
  const base64 = (base64String + padding).replace(/-/g, '+').replace(/_/g, '/');
  const rawData = atob(base64);
  // `new Uint8Array(new ArrayBuffer(n))`, not `new Uint8Array(n)` -- the
  // latter types as `Uint8Array<ArrayBufferLike>` under this project's TS
  // lib version, which `PushManager.subscribe`'s `BufferSource` parameter
  // (an `ArrayBufferView<ArrayBuffer>`) rejects.
  const outputArray = new Uint8Array(new ArrayBuffer(rawData.length));
  for (let i = 0; i < rawData.length; i++) {
    outputArray[i] = rawData.charCodeAt(i);
  }
  return outputArray;
}

/** Global "Enable notifications" control (Decision 6) -- not per-line,
 * since Decision 5 reuses pinned_lines/tracked_trains directly as scope.
 * Renders for every visitor (Tier 2, per docs/superpowers/specs/2026-08-31-anonymous-user-ux-design.md),
 * gated on browser capability, not install state (Decision 1 -- Android
 * and every desktop browser get real push from a bare open tab; only iOS
 * requires Home Screen install, which this component makes no attempt to
 * detect or require).
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
  // §2.11 bug (button pops in after hydration, splitting the mobile hero
  // between the `<h1>` and tagline on the most-visited page in the app).
  // Reserving the slot instead means the button is in the DOM from the
  // very first (server) render, `disabled` until the capability check
  // resolves on mount; a supported browser's button then just becomes
  // clickable in place, with nothing inserted or removed around it.
  const [checked, setChecked] = useState(false);
  const [supported, setSupported] = useState(false);
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  useEffect(() => {
    const isSupported = 'serviceWorker' in navigator && 'PushManager' in window;
    setSupported(isSupported);
    setChecked(true);
    if (!isSupported) return;

    // Seed `enabled` from the REAL subscription state, not just an assumed
    // `false` -- without this a returning subscriber sees "Enable
    // notifications" on every visit and would call `subscribe()` again on
    // every click, rather than the button reflecting they're already
    // subscribed. `getRegistration()` (not `.ready`, which resolves only
    // once a service worker HAS become active, and hangs forever until
    // then) resolves to `undefined` immediately when there is none yet to
    // check. Optional-chained/guarded throughout since a partial or
    // stubbed `serviceWorker` implementation may not expose it.
    let cancelled = false;
    const registrationPromise = navigator.serviceWorker.getRegistration?.();
    registrationPromise
      ?.then((registration) => registration?.pushManager.getSubscription())
      .then((subscription) => {
        if (!cancelled && subscription) {
          setEnabled(true);
        }
      })
      .catch(() => {
        // Nothing to seed -- falls back to the same "not yet enabled"
        // default a browser that was never subscribed would show.
      });
    return () => {
      cancelled = true;
    };
  }, []);

  async function enable() {
    setBusy(true);
    setError(null);
    needsLoginState.reset();
    try {
      const permission = await Notification.requestPermission();
      if (permission !== 'granted') {
        return;
      }

      const keyResponse = await fetch('/api/notifications/vapid-public-key');
      if (!keyResponse.ok) {
        setError(ENABLE_ERROR_MESSAGE);
        return;
      }
      const vapidPublicKey = await keyResponse.text();

      // Resolves once whatever service worker the sibling PWA effort
      // registers is active -- this component makes no assumption about
      // that SW's own file location or scope.
      const registration = await navigator.serviceWorker.ready;
      const subscription = await registration.pushManager.subscribe({
        userVisibleOnly: true,
        applicationServerKey: urlBase64ToUint8Array(vapidPublicKey),
      });
      const subscriptionJson = subscription.toJSON();

      const subscribeResponse = await fetch('/api/notifications/subscribe', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ endpoint: subscriptionJson.endpoint, keys: subscriptionJson.keys }),
      });
      if (!subscribeResponse.ok) {
        if (subscribeResponse.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          setError(ENABLE_ERROR_MESSAGE);
        }
        return;
      }
      setEnabled(true);
    } catch {
      // A rejected `Notification.requestPermission()`, `pushManager.subscribe()`
      // (common on Firefox/Brave with push disabled, or a malformed VAPID
      // key), or either `fetch()` all land here -- previously an unhandled
      // promise rejection, with the button just silently re-enabling and
      // no indication anything went wrong.
      setError(ENABLE_ERROR_MESSAGE);
    } finally {
      setBusy(false);
    }
  }

  // No `if (!supported) return null` branch any more: that's the reserved
  // slot above. A genuinely unsupported browser (rare -- effectively every
  // current mainstream browser has both `serviceWorker` and `PushManager`)
  // now keeps a permanently `disabled` "Enable notifications" button rather
  // than the button vanishing again post-mount, which would just move the
  // layout-shift problem from "pops in" to "pops out" for that same
  // Tier-2-anonymous, most-visited page.
  return (
    <>
      <Button onClick={enable} disabled={!checked || !supported || busy || enabled} variant={enabled ? 'light' : 'filled'}>
        {enabled ? 'Notifications enabled' : 'Enable notifications'}
      </Button>
      {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to enable notifications.
      </LoginPromptModal>
    </>
  );
}
