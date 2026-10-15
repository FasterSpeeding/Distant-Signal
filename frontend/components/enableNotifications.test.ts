import { describe, it, expect, vi, afterEach } from 'vitest';
import {
  BLOCKED_FIREFOX_MESSAGE,
  BLOCKED_MESSAGE,
  EnableNotificationsError,
  failureReference,
  NO_SUBSCRIPTION_ERROR,
  blockedMessage,
  isIos,
  isPushServiceMissing,
  isPushServiceUnavailable,
  subscribeToPush,
  urlBase64ToUint8Array,
  withTimeout,
} from './enableNotifications';

describe('urlBase64ToUint8Array', () => {
  it('decodes a plain base64url string with no padding characters needed', () => {
    expect(Array.from(urlBase64ToUint8Array('AAECAw'))).toEqual([0, 1, 2, 3]);
  });

  it('translates "-" and "_" to the standard base64 "+" and "/" alphabet', () => {
    expect(Array.from(urlBase64ToUint8Array('_-8'))).toEqual([255, 239]);
  });

  it('decodes a real-shaped 65-byte uncompressed P-256 VAPID public key', () => {
    const key = 'BEl62iUYgUivxIkv69yViEuiBIa40HI0DLLuxazjBk9j4H_hMVU2fV4kX_pMcFOxIRXVFrCzYqfE_ArNVjpJUBg';
    const decoded = urlBase64ToUint8Array(key);
    expect(decoded).toBeInstanceOf(Uint8Array);
    expect(decoded).toHaveLength(65);
    expect(decoded[0]).toBe(4);
  });
});

describe('withTimeout', () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  it('resolves with the value when the promise settles in time', async () => {
    await expect(withTimeout(Promise.resolve(3), 100, 'x')).resolves.toBe(3);
  });

  it('rejects once the time is up, for a promise that never settles (serviceWorker.ready)', async () => {
    vi.useFakeTimers();
    const result = withTimeout(new Promise(() => {}), 100, 'serviceWorker.ready');
    const assertion = expect(result).rejects.toThrow('serviceWorker.ready timed out after 100 ms');
    await vi.advanceTimersByTimeAsync(100);
    await assertion;
  });
});

describe('failureReference', () => {
  it('names the step and the error name, and no message text', () => {
    const error = new EnableNotificationsError('subscribe', new DOMException('secret detail', 'AbortError'));
    expect(failureReference(error)).toBe('subscribe-AbortError');
  });

  it('copes with an error from outside the flow', () => {
    expect(failureReference(new TypeError('x'))).toBe('unknown-TypeError');
  });
});

describe('blockedMessage', () => {
  it('gives Firefox its own steps, on Android and desktop', () => {
    expect(blockedMessage('Mozilla/5.0 (Android 14; Mobile; rv:131.0) Gecko/131.0 Firefox/131.0')).toBe(
      BLOCKED_FIREFOX_MESSAGE,
    );
    expect(blockedMessage('Mozilla/5.0 (X11; Linux x86_64; rv:131.0) Gecko/20100101 Firefox/131.0')).toBe(
      BLOCKED_FIREFOX_MESSAGE,
    );
  });

  it('gives other browsers general steps', () => {
    expect(blockedMessage('Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 Chrome/129.0.0.0 Mobile')).toBe(
      BLOCKED_MESSAGE,
    );
  });
});

describe('isIos', () => {
  it('detects an iPhone and an iPad reporting itself as a Mac', () => {
    expect(isIos({ userAgent: 'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X)', maxTouchPoints: 5 })).toBe(
      true,
    );
    expect(isIos({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)', maxTouchPoints: 5 })).toBe(true);
  });

  it('does not flag a desktop Mac or Android', () => {
    expect(isIos({ userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7)', maxTouchPoints: 0 })).toBe(false);
    expect(isIos({ userAgent: 'Mozilla/5.0 (Android 14; Mobile; rv:131.0) Firefox/131.0', maxTouchPoints: 5 })).toBe(
      false,
    );
  });
});

describe('isPushServiceUnavailable', () => {
  const abort = new DOMException('Error retrieving push subscription.', 'AbortError');
  const empty = Object.assign(new Error('x'), { name: NO_SUBSCRIPTION_ERROR });

  it('is true for an AbortError or no subscription from subscribe, while online', () => {
    expect(isPushServiceUnavailable(new EnableNotificationsError('subscribe', abort), true)).toBe(true);
    expect(isPushServiceUnavailable(new EnableNotificationsError('subscribe', empty), true)).toBe(true);
  });

  it('is false offline, for another step, or for another error', () => {
    expect(isPushServiceUnavailable(new EnableNotificationsError('subscribe', abort), false)).toBe(false);
    expect(isPushServiceUnavailable(new EnableNotificationsError('save', abort), true)).toBe(false);
    expect(
      isPushServiceUnavailable(
        new EnableNotificationsError('subscribe', new DOMException('x', 'NotAllowedError')),
        true,
      ),
    ).toBe(false);
    expect(isPushServiceUnavailable(abort, true)).toBe(false);
  });
});

describe('isPushServiceMissing', () => {
  it('is true only for an AbortError', () => {
    expect(isPushServiceMissing(new DOMException('Error retrieving push subscription.', 'AbortError'))).toBe(true);
    expect(isPushServiceMissing(new DOMException('x', 'InvalidStateError'))).toBe(false);
    expect(isPushServiceMissing(new TypeError('x'))).toBe(false);
  });
});

describe('subscribeToPush', () => {
  const KEY = 'BEl62iUYgUivxIkv69yViEuiBIa40HI0DLLuxazjBk9j4H_hMVU2fV4kX_pMcFOxIRXVFrCzYqfE_ArNVjpJUBg';

  function registrationWith(subscribe: () => Promise<unknown>): ServiceWorkerRegistration {
    return {
      pushManager: { subscribe, getSubscription: () => Promise.resolve(null) },
    } as unknown as ServiceWorkerRegistration;
  }

  it('fails the subscribe step when subscribe resolves without a subscription', async () => {
    const result = subscribeToPush(
      registrationWith(() => Promise.resolve(null)),
      KEY,
    );
    await expect(result).rejects.toBeInstanceOf(EnableNotificationsError);
    await expect(result.catch((error: unknown) => failureReference(error))).resolves.toBe(
      `subscribe-${NO_SUBSCRIPTION_ERROR}`,
    );
  });

  it('wraps an AbortError from subscribe in the subscribe step', async () => {
    const abort = new DOMException('Error retrieving push subscription.', 'AbortError');
    const result = subscribeToPush(
      registrationWith(() => Promise.reject(abort)),
      KEY,
    );
    await expect(result.catch((error: unknown) => failureReference(error))).resolves.toBe('subscribe-AbortError');
  });
});
