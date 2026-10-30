import { beforeEach, describe, expect, it, vi } from 'vitest';
import { fireEvent, render, screen } from '@testing-library/react';
import { Table } from '@mantine/core';
import { trackFeature } from '@/lib/usageAnalytics';
import { renderWithMantine } from '@/test/render';
import { LegStopsDisclosure } from './LegStopsDisclosure';
import { TrackFeatureArea } from './TrackFeatureArea';

vi.mock('@/lib/usageAnalytics', () => ({ trackFeature: vi.fn() }));

beforeEach(() => {
  vi.mocked(trackFeature).mockClear();
});

describe('TrackFeatureArea', () => {
  it('counts the area shown, and a link in it followed, by step only', () => {
    render(
      <TrackFeatureArea feature="shared_journeys" shown="shown" linkOpened="opened">
        <a href="#shared" onClick={(event) => event.preventDefault()}>
          Olive&apos;s trip
        </a>
        <p>Not a link</p>
      </TrackFeatureArea>,
    );
    expect(vi.mocked(trackFeature).mock.calls).toEqual([['shared_journeys', 'shown']]);
    fireEvent.click(screen.getByText('Not a link'));
    expect(trackFeature).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getByRole('link'));
    expect(vi.mocked(trackFeature).mock.calls).toEqual([
      ['shared_journeys', 'shown'],
      ['shared_journeys', 'opened'],
    ]);
  });
});

describe('LegStopsDisclosure usage counts', () => {
  it.each([
    ['earlier', 'earlier_expanded'],
    ['later', 'later_expanded'],
  ] as const)('counts opening the %s stops, not closing them', (part, step) => {
    renderWithMantine(
      <Table>
        <LegStopsDisclosure part={part} label="Stops (2)" colSpan={3}>
          <Table.Tr>
            <Table.Td>York</Table.Td>
          </Table.Tr>
        </LegStopsDisclosure>
      </Table>,
    );
    const toggle = screen.getByRole('button', { name: 'Stops (2)' });
    fireEvent.click(toggle);
    fireEvent.click(toggle);
    expect(vi.mocked(trackFeature).mock.calls).toEqual([['leg_stops', step]]);
  });
});
