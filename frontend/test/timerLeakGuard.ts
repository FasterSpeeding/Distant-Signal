/** FE-9: tracks every pending real timer and animation frame on `target`
 * so a test file's leftovers can be cancelled at the end of the file,
 * instead of sleeping until they fire. See vitest.setup.ts for why leaked
 * Mantine transition timers matter.
 *
 * `vi.useFakeTimers()` replaces these globals while active, and
 * `vi.useRealTimers()` restores the tracking wrappers installed here. */
type TimerTarget = Pick<
  typeof globalThis,
  'setTimeout' | 'clearTimeout' | 'requestAnimationFrame' | 'cancelAnimationFrame'
>;

export interface TimerLeakGuard {
  /** How many timers and frames are still pending. */
  pending(): { timeouts: number; frames: number };
  /** Cancels every pending frame (first, so none can schedule a new
   * timer) and then every pending timer. */
  cancelPending(): void;
}

export function installTimerLeakGuard(target: TimerTarget = globalThis): TimerLeakGuard {
  type Handle = ReturnType<typeof setTimeout>;
  const timeouts = new Set<Handle>();
  const frames = new Set<number>();

  const realSetTimeout = target.setTimeout;
  const realClearTimeout = target.clearTimeout;
  const trackedSetTimeout = ((handler: unknown, ms?: number, ...args: unknown[]) => {
    const handle: Handle = realSetTimeout(
      (...inner: unknown[]) => {
        timeouts.delete(handle);
        if (typeof handler === 'function') handler(...inner);
      },
      ms,
      ...args,
    );
    timeouts.add(handle);
    return handle;
  }) as typeof setTimeout;
  // Keeps `setTimeout[util.promisify.custom]` and friends.
  Object.assign(trackedSetTimeout, realSetTimeout);
  target.setTimeout = trackedSetTimeout;
  target.clearTimeout = ((handle?: Handle) => {
    if (handle !== undefined) timeouts.delete(handle);
    realClearTimeout(handle);
  }) as typeof clearTimeout;

  const realRaf = target.requestAnimationFrame;
  const realCancelRaf = target.cancelAnimationFrame;
  if (typeof realRaf === 'function' && typeof realCancelRaf === 'function') {
    target.requestAnimationFrame = (callback: FrameRequestCallback) => {
      const id = realRaf((time) => {
        frames.delete(id);
        callback(time);
      });
      frames.add(id);
      return id;
    };
    target.cancelAnimationFrame = (id: number) => {
      frames.delete(id);
      realCancelRaf(id);
    };
  }

  return {
    pending: () => ({ timeouts: timeouts.size, frames: frames.size }),
    cancelPending() {
      for (const id of [...frames]) target.cancelAnimationFrame(id);
      frames.clear();
      for (const handle of [...timeouts]) target.clearTimeout(handle);
      timeouts.clear();
    },
  };
}
