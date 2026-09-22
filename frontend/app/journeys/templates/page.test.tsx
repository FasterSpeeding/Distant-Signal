import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import JourneyTemplatesPage from './page';
import * as api from '@/lib/api';
import type { JourneyTemplateListItem } from '@/lib/types';

vi.mock('@/lib/api', async () => {
  const actual = await vi.importActual<typeof import('@/lib/api')>('@/lib/api');
  return {
    ...actual,
    getMyJourneyTemplates: vi.fn(),
  };
});

vi.mock('next/navigation', () => ({
  useRouter: () => ({ push: vi.fn(), refresh: vi.fn() }),
  usePathname: () => '/journeys/templates',
  useSearchParams: () => new URLSearchParams(''),
}));

function template(overrides: Partial<JourneyTemplateListItem> = {}): JourneyTemplateListItem {
  return {
    id: 1,
    customName: null,
    createdAt: '2026-09-22T00:00:00Z',
    legCount: 1,
    firstOriginCrs: 'KGX',
    firstOriginName: 'London Kings Cross',
    lastDestinationCrs: 'EDB',
    lastDestinationName: 'Edinburgh',
    active: true,
    daysOfWeek: null,
    ...overrides,
  };
}

async function renderPage() {
  return renderWithMantine(await JourneyTemplatesPage());
}

describe('JourneyTemplatesPage', () => {
  it('shows a login prompt when not logged in (getMyJourneyTemplates returns null)', async () => {
    vi.mocked(api.getMyJourneyTemplates).mockResolvedValue(null);
    await renderPage();
    expect(screen.getByRole('heading', { level: 1, name: 'Your journey templates' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Log in to see your journey templates' })).toBeInTheDocument();
  });

  it('shows the empty-state copy when logged in with no templates', async () => {
    vi.mocked(api.getMyJourneyTemplates).mockResolvedValue([]);
    await renderPage();
    expect(
      screen.getByText(/No templates yet\. Open a journey and choose "Make this a template"/),
    ).toBeInTheDocument();
  });

  it('renders one card per template, titled by its custom name when set', async () => {
    vi.mocked(api.getMyJourneyTemplates).mockResolvedValue([
      template({ id: 1, customName: 'My commute' }),
    ]);
    await renderPage();
    const link = screen.getByRole('link', { name: 'My commute' });
    expect(link).toBeInTheDocument();
    expect(link).toHaveAttribute('href', '/journeys/templates/1');
    // The computed route is still shown as a secondary line under a custom name.
    expect(screen.getByText('London Kings Cross (KGX) → Edinburgh (EDB)')).toBeInTheDocument();
  });

  it('falls back to the computed route as the title when there is no custom name', async () => {
    vi.mocked(api.getMyJourneyTemplates).mockResolvedValue([template({ id: 2, customName: null })]);
    await renderPage();
    expect(
      screen.getByRole('link', { name: 'London Kings Cross (KGX) → Edinburgh (EDB)' }),
    ).toBeInTheDocument();
  });

  it('shows the leg count, singular for one leg', async () => {
    vi.mocked(api.getMyJourneyTemplates).mockResolvedValue([template({ legCount: 1 })]);
    await renderPage();
    expect(screen.getByText(/1 leg ·/)).toBeInTheDocument();
  });

  it('shows the leg count, plural for more than one leg', async () => {
    vi.mocked(api.getMyJourneyTemplates).mockResolvedValue([template({ legCount: 3 })]);
    await renderPage();
    expect(screen.getByText(/3 legs ·/)).toBeInTheDocument();
  });

  it('renders one card per template when there are several', async () => {
    vi.mocked(api.getMyJourneyTemplates).mockResolvedValue([
      template({ id: 1, customName: 'Commute' }),
      template({ id: 2, customName: 'Weekend trip' }),
    ]);
    await renderPage();
    expect(screen.getByRole('link', { name: 'Commute' })).toBeInTheDocument();
    expect(screen.getByRole('link', { name: 'Weekend trip' })).toBeInTheDocument();
  });
});
