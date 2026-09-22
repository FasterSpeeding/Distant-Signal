import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import JourneyTemplateDetailPage from './page';
import * as api from '@/lib/api';
import type { JourneyTemplateDetail } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getJourneyTemplate: vi.fn(),
  };
});

vi.mock('next/navigation', () => ({
  notFound: () => {
    throw new Error('NEXT_NOT_FOUND');
  },
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/journeys/templates/167',
  useSearchParams: () => new URLSearchParams(''),
}));

function template(overrides: Partial<JourneyTemplateDetail> = {}): JourneyTemplateDetail {
  return {
    id: 167,
    customName: null,
    createdAt: '2026-09-22T00:00:00Z',
    updatedAt: '2026-09-22T00:00:00Z',
    daysOfWeek: null,
    active: true,
    startsOn: null,
    endsOn: null,
    defaultMatchMode: 'manual',
    autoCommitRule: null,
    legs: [
      {
        id: 1,
        originCrs: 'KGX',
        originName: 'London Kings Cross',
        destinationCrs: 'EDB',
        destinationName: 'Edinburgh',
        departAfter: '09:00',
        departBefore: null,
        arriveAfter: null,
        arriveBefore: null,
      },
    ],
    ...overrides,
  };
}

async function renderPage(id = '167') {
  return renderWithMantine(await JourneyTemplateDetailPage({ params: Promise.resolve({ id }) }));
}

describe('JourneyTemplateDetailPage', () => {
  it('shows a login prompt on a 401 (ApiUnauthorizedError)', async () => {
    vi.mocked(api.getJourneyTemplate).mockRejectedValue(
      new api.ApiUnauthorizedError('API request failed: 401'),
    );
    await renderPage();
    expect(
      screen.getByRole('link', { name: 'Log in to view this template' }),
    ).toBeInTheDocument();
  });

  it('calls notFound() on a 404 (ApiNotFoundError)', async () => {
    vi.mocked(api.getJourneyTemplate).mockRejectedValue(
      new api.ApiNotFoundError('API request failed: 404'),
    );
    await expect(renderPage()).rejects.toThrow('NEXT_NOT_FOUND');
  });

  it('renders the title, the leg editor pre-filled with the template legs, and both header buttons', async () => {
    vi.mocked(api.getJourneyTemplate).mockResolvedValue(
      template({ customName: 'My commute' }),
    );
    await renderPage();

    expect(screen.getByRole('heading', { level: 1, name: 'My commute' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Run now' })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Delete template' })).toBeInTheDocument();
    expect(screen.getByLabelText('Origin CRS')).toHaveValue('KGX');
    expect(screen.getByLabelText('Destination CRS')).toHaveValue('EDB');
    expect(screen.getByLabelText('Earliest departure (optional)')).toHaveValue('09:00');
  });

  it('falls back to a generic title when there is no custom name', async () => {
    vi.mocked(api.getJourneyTemplate).mockResolvedValue(template({ customName: null }));
    await renderPage();
    expect(screen.getByRole('heading', { level: 1, name: 'Journey template' })).toBeInTheDocument();
  });
});
