import { describe, it, expect, vi, beforeEach } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import TrackedTrainByUidPage from './page';
import * as api from '@/lib/api';
import { ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return { ...actual, getTrackedTrainByUidAndDate: vi.fn() };
});
// Mocked so a real Next.js `notFound()` throw (its actual behaviour) doesn't
// require an app-router tree to render -- same pattern as
// app/lines/[id]/page.test.tsx.
const notFoundMock = vi.fn(() => {
  throw new Error('NEXT_NOT_FOUND');
});
vi.mock('next/navigation', () => ({
  notFound: () => notFoundMock(),
}));

async function renderPage(uid = 'W12345', date = '2026-08-31') {
  const element = await TrackedTrainByUidPage({ params: Promise.resolve({ uid, date }) });
  return renderWithMantine(element);
}

describe('TrackedTrainByUidPage error handling', () => {
  beforeEach(() => {
    notFoundMock.mockClear();
  });

  it('shows a login prompt linking to /api/auth/login on ApiUnauthorizedError', async () => {
    vi.mocked(api.getTrackedTrainByUidAndDate).mockRejectedValue(new ApiUnauthorizedError('unauthorized'));
    await renderPage();
    const link = screen.getByRole('link', { name: 'Log in to view this tracked train' });
    expect(link).toHaveAttribute('href', '/api/auth/login');
    expect(notFoundMock).not.toHaveBeenCalled();
  });

  it('still calls notFound() on ApiNotFoundError, unswallowed by the new branch', async () => {
    vi.mocked(api.getTrackedTrainByUidAndDate).mockRejectedValue(new ApiNotFoundError('not found'));
    await expect(renderPage()).rejects.toThrow('NEXT_NOT_FOUND');
    expect(notFoundMock).toHaveBeenCalled();
  });

  it('still propagates a bare Error uncaught', async () => {
    vi.mocked(api.getTrackedTrainByUidAndDate).mockRejectedValue(new Error('boom'));
    await expect(renderPage()).rejects.toThrow('boom');
    expect(notFoundMock).not.toHaveBeenCalled();
  });
});
