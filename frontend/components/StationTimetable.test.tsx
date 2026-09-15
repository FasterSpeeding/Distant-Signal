import { describe, it, expect, vi } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { StationTimetable } from './StationTimetable';

describe('StationTimetable', () => {
  it('renders collapsed by default: the control is present, but no fetch happens and no panel content is in the document', () => {
    const fetchMock = vi.fn();
    vi.stubGlobal('fetch', fetchMock);

    renderWithMantine(<StationTimetable crs="RDG" />);

    expect(screen.getByRole('button', { name: 'Scheduled departures' })).toBeInTheDocument();
    expect(screen.queryByText('Loading scheduled departures…')).not.toBeInTheDocument();
    expect(fetchMock).not.toHaveBeenCalled();
  });
});
