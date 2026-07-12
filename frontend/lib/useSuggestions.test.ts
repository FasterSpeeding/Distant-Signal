import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { renderHook, waitFor, act } from '@testing-library/react';
import { useSuggestions } from './useSuggestions';
import type { Suggestion } from './suggestions';

const sample: Suggestion[] = [{ code: 'WOK', name: 'Woking' }];

describe('useSuggestions', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it('returns no suggestions and does not call search for an empty query', () => {
    const search = vi.fn();
    const { result } = renderHook(() => useSuggestions('', search));
    expect(result.current.suggestions).toEqual([]);
    expect(search).not.toHaveBeenCalled();
  });

  it('debounces before calling search', async () => {
    const search = vi.fn().mockResolvedValue(sample);
    const { rerender } = renderHook(({ query }) => useSuggestions(query, search), {
      initialProps: { query: 'w' },
    });
    rerender({ query: 'wo' });
    rerender({ query: 'wok' });

    expect(search).not.toHaveBeenCalled();

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });

    expect(search).toHaveBeenCalledTimes(1);
    expect(search).toHaveBeenCalledWith('wok', expect.any(AbortSignal));
  });

  it('populates suggestions once the debounced search resolves', async () => {
    const search = vi.fn().mockResolvedValue(sample);
    const { result } = renderHook(() => useSuggestions('wok', search));

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    await waitFor(() => expect(result.current.suggestions).toEqual(sample));
  });

  it('aborts the in-flight request when the query changes again before it resolves', async () => {
    const search = vi.fn().mockResolvedValue(sample);
    const { rerender } = renderHook(({ query }) => useSuggestions(query, search), {
      initialProps: { query: 'wok' },
    });

    await act(async () => {
      await vi.advanceTimersByTimeAsync(250);
    });
    expect(search).toHaveBeenCalledTimes(1);
    const firstSignal = search.mock.calls[0][1] as AbortSignal;

    rerender({ query: 'alt' });
    expect(firstSignal.aborted).toBe(true);
  });
});
