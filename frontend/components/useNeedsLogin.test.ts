import { describe, it, expect } from 'vitest';
import { renderHook, act } from '@testing-library/react';
import { useNeedsLogin } from './useNeedsLogin';

describe('useNeedsLogin', () => {
  it('starts false, becomes true after markNeedsLogin, and returns to false after reset', () => {
    const { result } = renderHook(() => useNeedsLogin());
    expect(result.current.needsLogin).toBe(false);

    act(() => {
      result.current.markNeedsLogin();
    });
    expect(result.current.needsLogin).toBe(true);

    act(() => {
      result.current.reset();
    });
    expect(result.current.needsLogin).toBe(false);
  });
});
