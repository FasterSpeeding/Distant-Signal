import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import IncidentsPage from './page';
import * as api from '@/lib/api';
import type { LineSummary, Suggestion } from '@/lib/types';

vi.mock('@/lib/api', async (importOriginal) => {
  const actual = await importOriginal<typeof import('@/lib/api')>();
  return { ...actual, getAllLines: vi.fn(), getAllTocs: vi.fn() };
});

const TEST_LINES: LineSummary[] = [
  { id: 'south-western', name: 'South Western Main Line', category: 'main', operators: ['SW'], source: 'catalogue' },
];
const TEST_TOCS: Suggestion[] = [{ code: 'SW', name: 'South Western Railway' }];

describe('IncidentsPage', () => {
  it('renders the heading and the search form with fetched reference data', async () => {
    vi.mocked(api.getAllLines).mockResolvedValue(TEST_LINES);
    vi.mocked(api.getAllTocs).mockResolvedValue(TEST_TOCS);

    renderWithMantine(await IncidentsPage({ searchParams: Promise.resolve({}) }));

    expect(screen.getByText('Incident Archive')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Search' })).toBeInTheDocument();
  });

  it('degrades to empty reference-data lists if either fetch fails, rather than crashing the page', async () => {
    vi.mocked(api.getAllLines).mockRejectedValue(new Error('network error'));
    vi.mocked(api.getAllTocs).mockRejectedValue(new Error('network error'));

    renderWithMantine(await IncidentsPage({ searchParams: Promise.resolve({}) }));

    expect(screen.getByText('Incident Archive')).toBeInTheDocument();
  });

  it('passes searchParams through as initial filter values', async () => {
    vi.mocked(api.getAllLines).mockResolvedValue(TEST_LINES);
    vi.mocked(api.getAllTocs).mockResolvedValue(TEST_TOCS);

    renderWithMantine(
      await IncidentsPage({
        searchParams: Promise.resolve({ operator: 'SW,VT', line: 'south-western' }),
      }),
    );

    // The MultiSelect renders its selected values as removable pills with
    // this exact label text -- confirms initialOperator was parsed and
    // passed through rather than dropped. Scoped to the pill label class:
    // Mantine also keeps its (closed, `display: none`) options list mounted
    // in the DOM with the same "CODE — Name" text, so an unscoped
    // `getByText` matches both.
    expect(
      screen.getByText('SW — South Western Railway', { selector: '.mantine-Pill-label' }),
    ).toBeInTheDocument();
  });
});
