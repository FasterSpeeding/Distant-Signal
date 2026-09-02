'use client';

import { useEffect, useState } from 'react';
import { Button } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';

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
  const [supported, setSupported] = useState(false);
  const [enabled, setEnabled] = useState(false);
  const [busy, setBusy] = useState(false);
  const needsLoginState = useNeedsLogin();

  useEffect(() => {
    setSupported('serviceWorker' in navigator && 'PushManager' in window);
  }, []);

  async function enable() {
    setBusy(true);
    needsLoginState.reset();
    try {
      const permission = await Notification.requestPermission();
      if (permission !== 'granted') {
        return;
      }

      const keyResponse = await fetch('/api/notifications/vapid-public-key');
      if (!keyResponse.ok) {
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
        }
        return;
      }
      setEnabled(true);
    } finally {
      setBusy(false);
    }
  }

  if (!supported) {
    return null;
  }

  return (
    <>
      <Button onClick={enable} disabled={busy || enabled} variant={enabled ? 'light' : 'filled'}>
        {enabled ? 'Notifications enabled' : 'Enable notifications'}
      </Button>
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to enable notifications.
      </LoginPromptModal>
    </>
  );
}
