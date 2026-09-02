import { describe, it, expect, vi } from 'vitest';
import { renderHook } from '@testing-library/react';
import { useLoginHref } from './useLoginHref';

const mockUsePathname = vi.fn();
const mockUseSearchParams = vi.fn();
vi.mock('next/navigation', () => ({
  usePathname: () => mockUsePathname(),
  useSearchParams: () => mockUseSearchParams(),
}));

describe('useLoginHref', () => {
  it('returns return_to = pathname alone when there is no query string', () => {
    mockUsePathname.mockReturnValue('/lines/some-line');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    const { result } = renderHook(() => useLoginHref());
    expect(result.current).toBe('/api/auth/login?return_to=%2Flines%2Fsome-line');
  });

  it('appends pathname + query string, URL-encoded, when a query string is present', () => {
    mockUsePathname.mockReturnValue('/lines/some-line');
    mockUseSearchParams.mockReturnValue(new URLSearchParams('tab=history'));
    const { result } = renderHook(() => useLoginHref());
    expect(result.current).toBe('/api/auth/login?return_to=%2Flines%2Fsome-line%3Ftab%3Dhistory');
  });

  it('renders the root path correctly', () => {
    mockUsePathname.mockReturnValue('/');
    mockUseSearchParams.mockReturnValue(new URLSearchParams(''));
    const { result } = renderHook(() => useLoginHref());
    expect(result.current).toBe('/api/auth/login?return_to=%2F');
  });
});
