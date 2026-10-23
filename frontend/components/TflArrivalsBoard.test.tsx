import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { RUSSELL_SQUARE_ARRIVALS } from '@/test/tflStopFixtures';
import { arrivalGroupTitle, platformLabel, TflArrivalsBoard } from './TflArrivalsBoard';

describe('TflArrivalsBoard', () => {
  it('lists each platform under its own heading, soonest first', () => {
    renderWithMantine(<TflArrivalsBoard arrivals={RUSSELL_SQUARE_ARRIVALS} />);

    const east = screen.getByRole('list', { name: 'Piccadilly, Eastbound - Platform 1' });
    const rows = within(east).getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    expect(rows[0]!.textContent).toContain('Cockfosters');
    expect(rows[0]!.textContent).toContain('Due');
    expect(rows[0]!.textContent).toContain('due now');
    // The expected time is UK time; where the train is now follows it.
    expect(rows[0]!.textContent).toContain('Expected 10:00 · At Holborn');
    expect(rows[1]!.textContent).toContain('Arnos Grove');
    expect(rows[1]!.textContent).toContain('6 min');
    expect(rows[1]!.textContent).toContain('in 6 minutes');

    const west = screen.getByRole('list', { name: 'Piccadilly, Westbound - Platform 2' });
    expect(within(west).getByText('Heathrow Terminal 5')).toBeInTheDocument();
    expect(screen.getAllByRole('heading', { level: 3 }).map((h) => h.textContent)).toEqual([
      'Piccadilly, Eastbound - Platform 1',
      'Piccadilly, Westbound - Platform 2',
    ]);
  });

  it('says so when nothing is predicted', () => {
    renderWithMantine(
      <TflArrivalsBoard
        arrivals={{
          ...RUSSELL_SQUARE_ARRIVALS,
          groups: [{ ...RUSSELL_SQUARE_ARRIVALS.groups[0]!, arrivals: [] }],
        }}
      />,
    );
    expect(screen.getByRole('status').textContent).toBe('No arrivals are predicted here right now.');
    expect(screen.queryByRole('list')).not.toBeInTheDocument();
  });

  it('names a bare platform number as a platform, and a group with none by its line', () => {
    expect(platformLabel('2')).toBe('Platform 2');
    expect(platformLabel('Westbound - Platform 1')).toBe('Westbound - Platform 1');
    expect(arrivalGroupTitle({ lineName: 'DLR', platformName: '3' })).toBe('DLR, Platform 3');
    expect(arrivalGroupTitle({ lineName: 'Tram', platformName: '' })).toBe('Tram');
  });
});
