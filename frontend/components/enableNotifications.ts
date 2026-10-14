/**
 * The browser half of "Enable notifications" (`NotificationsToggle.tsx`):
 * asking for permission, subscribing to push and telling the failures
 * apart. Kept out of the component so each step and each failure is
 * unit-testable on its own.
 *
 * Why the order matters (Firefox for Android): Gecko's
 * `PushManager.subscribe()` runs its own permission check
 * (`Push.sys.mjs#askPermission`). When Gecko's permission store doesn't
 * hold an explicit "allow" for the site (GeckoView leaves site permissions
 * to the app, so it often doesn't, even after `requestPermission()`
 * resolved "granted"), it asks again, and GeckoView's permission actor
 * refuses any notification prompt without a transient user activation.
 * That activation lasts a few seconds after the tap. The old flow awaited
 * the permission prompt, then a network fetch of the VAPID key, then
 * `serviceWorker.ready`, and only then subscribed, so on a phone the
 * activation was usually gone and `subscribe()` rejected with a
 * `NotAllowedError`, which the page showed as a permissions problem
 * whatever the real permission state was. Now `requestPermission()` is
 * called synchronously in the click handler, the key and the service
 * worker registration are fetched before the click, and `subscribe()`
 * follows the permission answer with nothing slow in between.
 */

import { createLogger } from '@/lib/logger';
import { describeFailure } from '@/lib/failure';

const log = createLogger('components/NotificationsToggle');

/** "Couldn't turn on notifications. Try again." */
export const GENERIC_ENABLE_ERROR = describeFailure('turn on', 'notifications');

export const BLOCKED_FIREFOX_MESSAGE =
  "Couldn't turn on notifications. They're blocked for this site. In Firefox, tap the padlock next to the address, allow notifications, then try again.";

export const BLOCKED_MESSAGE =
  "Couldn't turn on notifications. They're blocked for this site. Allow notifications in your browser's site settings, then try again.";

export const UNSUPPORTED_MESSAGE = "This browser can't show notifications.";

export const UNSUPPORTED_IOS_MESSAGE =
  'On iPhone and iPad, add Distant Signal to your home screen, then turn on notifications from there.';

/** How long to wait for a service worker before giving up. `ready` never
 * settles when registration failed (private browsing, a blocked script),
 * which used to leave the button spinning for good. */
export const SERVICE_WORKER_TIMEOUT_MS = 10_000;

/** The step of the flow that failed, for the log and the on-screen
 * reference. */
export type EnableStep = 'permission' | 'key' | 'worker' | 'subscribe' | 'save';

/** A failure in one step, carrying what went wrong underneath. */
export class EnableNotificationsError extends Error {
  readonly step: EnableStep;

  constructor(step: EnableStep, cause: unknown) {
    super(`enabling notifications failed at ${step}`, { cause });
    this.name = 'EnableNotificationsError';
    this.step = step;
  }
}

function isFirefox(userAgent: string): boolean {
  return /\bFirefox\//.test(userAgent) || /\bFxiOS\//.test(userAgent);
}

/** iPhone and iPad, including iPadOS, which reports itself as a Mac. Safari
 * there exposes push only to a site added to the home screen. */
export function isIos(nav: Pick<Navigator, 'userAgent' | 'maxTouchPoints'>): boolean {
  return (
    /\b(iPhone|iPad|iPod)\b/.test(nav.userAgent) || (/\bMacintosh\b/.test(nav.userAgent) && nav.maxTouchPoints > 1)
  );
}

/** Whether this browser can subscribe to push from a page. */
export function pushSupported(): boolean {
  return (
    typeof window !== 'undefined' && 'serviceWorker' in navigator && 'PushManager' in window && 'Notification' in window
  );
}

export function unsupportedMessage(nav: Pick<Navigator, 'userAgent' | 'maxTouchPoints'>): string {
  return isIos(nav) ? UNSUPPORTED_IOS_MESSAGE : UNSUPPORTED_MESSAGE;
}

export function blockedMessage(userAgent: string): string {
  return isFirefox(userAgent) ? BLOCKED_FIREFOX_MESSAGE : BLOCKED_MESSAGE;
}

/** `Notification.requestPermission()` in both its forms: the promise one
 * every current browser returns, and the callback one older Safari takes
 * (it returns `undefined`). Calls it synchronously, so it must itself be
 * called synchronously from the click handler to keep the user activation. */
export function requestNotificationPermission(): Promise<NotificationPermission> {
  return new Promise((resolve, reject) => {
    const returned = Notification.requestPermission((permission) => resolve(permission)) as
      Promise<NotificationPermission> | undefined;
    if (returned && typeof returned.then === 'function') {
      returned.then(resolve, reject);
    }
  });
}

/** `promise`, or a rejection after `ms`. */
export function withTimeout<T>(promise: Promise<T>, ms: number, what: string): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_, reject) => {
    timer = setTimeout(() => reject(new Error(`${what} timed out after ${ms} ms`)), ms);
  });
  return Promise.race([promise, timeout]).finally(() => clearTimeout(timer));
}

/** Fetches the VAPID public key (base64url text). */
export async function fetchVapidKey(): Promise<string> {
  const response = await fetch('/api/notifications/vapid-public-key');
  if (!response.ok) {
    throw new Error(`vapid-public-key returned HTTP ${response.status}`);
  }
  return (await response.text()).trim();
}

/** Converts the VAPID public key (base64url, as returned by
 * `GET /public/notifications/vapid-public-key`) into the raw
 * `Uint8Array` form `PushManager.subscribe({ applicationServerKey })`
 * expects. Newer browsers accept the base64url string directly per the
 * Push API spec's `(BufferSource or DOMString)` union, but Safari/WebKit
 * has historically required the `BufferSource` form, and Firefox is strict
 * about the decoded key being a 65-byte P-256 point. Converting always is
 * the one call shape that works across every supported browser, so this
 * is not a browser-conditional fallback, just the safe default. */
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

/** Subscribes with `vapidKey`. An `InvalidStateError` means this browser
 * already holds a subscription made with a different key (the key was
 * rotated): that one is dropped and the subscribe retried once. */
export async function subscribeToPush(
  registration: ServiceWorkerRegistration,
  vapidKey: string,
): Promise<PushSubscription> {
  let applicationServerKey: Uint8Array<ArrayBuffer>;
  try {
    applicationServerKey = urlBase64ToUint8Array(vapidKey);
  } catch (error) {
    throw new EnableNotificationsError('key', error);
  }
  const options: PushSubscriptionOptionsInit = { userVisibleOnly: true, applicationServerKey };
  try {
    return await registration.pushManager.subscribe(options);
  } catch (error) {
    if (errorName(error) !== 'InvalidStateError') {
      throw new EnableNotificationsError('subscribe', error);
    }
    try {
      const stale = await registration.pushManager.getSubscription();
      await stale?.unsubscribe();
      return await registration.pushManager.subscribe(options);
    } catch (retryError) {
      throw new EnableNotificationsError('subscribe', retryError);
    }
  }
}

/** An error's `name` (`NotAllowedError`, `AbortError`, `TypeError`), read
 * by shape rather than `instanceof Error`: a `DOMException` isn't an
 * `Error` instance everywhere (jsdom, some older engines). */
export function errorName(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'name' in error && typeof error.name === 'string') {
    return error.name;
  }
  return typeof error;
}

function errorMessage(error: unknown): string {
  if (typeof error === 'object' && error !== null && 'message' in error && typeof error.message === 'string') {
    return error.message;
  }
  return String(error);
}

/** A short code naming the failed step and the browser's error name, such
 * as `subscribe-NotAllowedError`. Shown as the "Reference" under the error
 * so a screenshot from a phone says what failed. Holds no message text,
 * endpoint or key. */
export function failureReference(error: unknown): string {
  const step = error instanceof EnableNotificationsError ? error.step : 'unknown';
  const cause = error instanceof EnableNotificationsError ? error.cause : error;
  return `${step}-${errorName(cause)}`;
}

/** Logs a failed enable with enough detail to diagnose it from a report:
 * the step, the error's name and message, the permission state and the
 * browser. Never the push endpoint, the subscription keys or the VAPID key. */
export function logEnableFailure(error: unknown): void {
  const step = error instanceof EnableNotificationsError ? error.step : 'unknown';
  const cause = error instanceof EnableNotificationsError ? error.cause : error;
  log.warn('Enabling notifications failed', {
    step,
    errorName: errorName(cause),
    errorMessage: errorMessage(cause),
    permission: typeof Notification === 'undefined' ? 'unsupported' : Notification.permission,
    // Missing before Firefox 120 and Safari 16.4, whatever the DOM types say.
    userActivationActive: (navigator as Partial<Pick<Navigator, 'userActivation'>>).userActivation?.isActive,
    userAgent: navigator.userAgent,
  });
}
