import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrackedTrainByIdPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getTrackedTrainById: vi.fn() };
});
// Mocked so a real Next.js `notFound()` throw (its actual behaviour) doesn't
// require an app-router tree to render -- same pattern as
// app/lines/[id]/page.test.tsx.
const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
// usePathname()/useSearchParams() stubbed for the same reason as
// AuthStatus.test.tsx/TicketPanel.test.tsx -- the login-error branch now
// renders LoginLink (Task 1).
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
  usePathname: () => '/train/by-id/42',
  useSearchParams: () => new URLSearchParams(''),
}));

async function renderPage(trackingId = '42') {
  const element = await TrackedTrainByIdPage({ params: Promise.resolve({ trackingId }) });
  return renderWithMantine(element);
}

describe('TrackedTrainByIdPage error handling', () => {
  beforeEach(() => {
    notFoundMock.mockClear();
  });

  it('shows a login prompt linking to /api/auth/login on ApiUnauthorizedError', async () => {
    vi.mocked(api.getTrackedTrainById).mockRejectedValue(new ApiUnauthorizedError('unauthorized'));
    await renderPage('42');
    const link = screen.getByRole('link', { name: 'Log in to view this tracked train' });
    expect(link).toHaveAttribute('href', '/api/auth/login?return_to=%2Ftrain%2Fby-id%2F42');
    expect(notFoundMock).not.toHaveBeenCalled();
  });

  it('still calls notFound() on ApiNotFoundError, unswallowed by the new branch', async () => {
    vi.mocked(api.getTrackedTrainById).mockRejectedValue(new ApiNotFoundError('not found'));
    await expect(renderPage('42')).rejects.toThrow('NEXT_NOT_FOUND');
    expect(notFoundMock).toHaveBeenCalled();
  });

  it('still propagates a bare Error uncaught', async () => {
    vi.mocked(api.getTrackedTrainById).mockRejectedValue(new Error('boom'));
    await expect(renderPage('42')).rejects.toThrow('boom');
    expect(notFoundMock).not.toHaveBeenCalled();
  });
});
