import { describe, it, expect, vi } from 'vitest';
import { installTimerLeakGuard } from './timerLeakGuard';

// FE-9: the guard vitest.setup.ts uses instead of sleeping after each file.
function fakeTarget() {
  let nextFrame = 1;
  const frameCallbacks = new Map<number, FrameRequestCallback>();
  const target = {
    setTimeout: globalThis.setTimeout,
    clearTimeout: globalThis.clearTimeout,
    requestAnimationFrame: (cb: FrameRequestCallback) => {
      const id = nextFrame++;
      frameCallbacks.set(id, cb);
      return id;
    },
    cancelAnimationFrame: (id: number) => {
      frameCallbacks.delete(id);
    },
  };
  const runFrames = () => {
    for (const [id, cb] of [...frameCallbacks]) {
      frameCallbacks.delete(id);
      cb(0);
    }
  };
  return { target, runFrames, frameCallbacks };
}

describe('installTimerLeakGuard', () => {
  it('forgets a timer once it fires or is cleared', async () => {
    const { target } = fakeTarget();
    const guard = installTimerLeakGuard(target as unknown as typeof globalThis);
    const fired = vi.fn();
    target.setTimeout(fired, 0);
    const cleared = target.setTimeout(() => {}, 10_000);
    expect(guard.pending().timeouts).toBe(2);
    target.clearTimeout(cleared);
    await new Promise<void>((resolve) => globalThis.setTimeout(resolve, 5));
    expect(fired).toHaveBeenCalledTimes(1);
    expect(guard.pending().timeouts).toBe(0);
  });

  it('cancels pending frames before they can schedule timers, then pending timers', async () => {
    const { target, runFrames, frameCallbacks } = fakeTarget();
    const guard = installTimerLeakGuard(target as unknown as typeof globalThis);
    const leaked = vi.fn();
    // Mantine's shape: rAF -> setTimeout.
    target.requestAnimationFrame(() => target.setTimeout(leaked, 0));
    target.setTimeout(leaked, 0);
    expect(guard.pending()).toEqual({ timeouts: 1, frames: 1 });
    guard.cancelPending();
    expect(frameCallbacks.size).toBe(0);
    runFrames();
    await new Promise<void>((resolve) => globalThis.setTimeout(resolve, 5));
    expect(leaked).not.toHaveBeenCalled();
    expect(guard.pending()).toEqual({ timeouts: 0, frames: 0 });
  });

  it('passes extra arguments through to the handler', async () => {
    const { target } = fakeTarget();
    installTimerLeakGuard(target as unknown as typeof globalThis);
    const handler = vi.fn();
    (target.setTimeout as (h: (...a: unknown[]) => void, ms: number, ...a: unknown[]) => unknown)(handler, 0, 'a', 1);
    await new Promise<void>((resolve) => globalThis.setTimeout(resolve, 5));
    expect(handler).toHaveBeenCalledWith('a', 1);
  });
});
