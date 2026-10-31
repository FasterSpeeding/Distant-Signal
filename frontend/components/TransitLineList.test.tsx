import { describe, expect, it } from 'vitest';
import { screen, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { MINOR_DELAYS, NETWORK, NOTHING_REPORTED } from '@/test/transitFixtures';
import type { TransitLineStatus } from '@/lib/types';
import { TransitLineList, TransitLineStatusSummary, withoutLinePrefix } from './TransitLineList';

describe('TransitLineList', () => {
  it('lists each line as a row: its link, colour and status', () => {
    renderWithMantine(
      <TransitLineList
        network="metrolink"
        label="Lines"
        lines={[
          { ...NETWORK.lines[0]!, status: MINOR_DELAYS },
          { ...NETWORK.lines[1]!, status: NOTHING_REPORTED },
        ]}
      />,
    );
    const list = screen.getByRole('list', { name: 'Lines' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows).toHaveLength(2);
    expect(within(rows[0]!).getByRole('link', { name: 'Blue Line' }).getAttribute('href')).toBe(
      '/transit/metrolink/lines/blue',
    );
    expect(rows[0]!.querySelector('[data-line-swatch]')).not.toBeNull();
    expect(rows[0]!.textContent).toContain('Minor delays');
    // Why, in a line, only for the line that isn't running well.
    expect(rows[0]!.querySelector('[data-line-summary]')?.textContent).toBe(MINOR_DELAYS.statuses[0]!.reason);
    expect(rows[1]!.querySelector('[data-line-summary]')).toBeNull();
  });

  it('takes the summary slot from the page', () => {
    renderWithMantine(
      <TransitLineList
        network="metrolink"
        label="Lines"
        lines={[NETWORK.lines[0]!]}
        summary={(line) => <span>Summary for {line.name}</span>}
      />,
    );
    expect(screen.getByText('Summary for Blue Line')).toBeTruthy();
  });
});

describe('TransitLineStatusSummary', () => {
  const status = (statuses: TransitLineStatus['statuses'], state: TransitLineStatus['state'] = 'current') => ({
    state,
    asOf: null,
    statuses,
    upcoming: [],
  });

  it('names the first problem and counts the rest', () => {
    renderWithMantine(
      <TransitLineStatusSummary
        status={status([
          { severity: 27, description: 'No Reported Disruption', reason: 'Fine', situationId: null },
          { severity: 9, description: 'Minor Delays', reason: 'Signal failure at Bury', situationId: 'a' },
          { severity: 6, description: 'Severe Delays', reason: ' ', situationId: 'b' },
        ])}
      />,
    );
    expect(screen.getByText('Signal failure at Bury (and 1 more)')).toBeTruthy();
  });

  it('falls back to the description, and says nothing without a current problem', () => {
    const { container, rerender } = renderWithMantine(
      <TransitLineStatusSummary
        status={status([{ severity: 9, description: 'Minor Delays', reason: '', situationId: null }])}
      />,
    );
    expect(container.querySelector('[data-line-summary]')?.textContent).toBe('Minor Delays');
    rerender(<TransitLineStatusSummary status={NOTHING_REPORTED} />);
    expect(container.querySelector('[data-line-summary]')).toBeNull();
    rerender(<TransitLineStatusSummary status={{ ...MINOR_DELAYS, state: 'unavailable' }} />);
    expect(container.querySelector('[data-line-summary]')).toBeNull();
    rerender(<TransitLineStatusSummary status={undefined} />);
    expect(container.querySelector('[data-line-summary]')).toBeNull();
  });
});

describe('withoutLinePrefix', () => {
  it('drops TfL\'s leading line name, in any case, with or without "line"', () => {
    expect(withoutLinePrefix('Bakerloo Line: No service south of Oxford Circus.', 'Bakerloo')).toBe(
      'No service south of Oxford Circus.',
    );
    expect(withoutLinePrefix('PICCADILLY LINE: Saturday 31 October, closed.', 'Piccadilly')).toBe(
      'Saturday 31 October, closed.',
    );
    expect(withoutLinePrefix('Waterloo & City line: weekdays only.', 'Waterloo & City')).toBe('weekdays only.');
    expect(withoutLinePrefix('LONDON TRAMS: no service.', 'London Trams')).toBe('no service.');
  });

  it('keeps any other text whole', () => {
    expect(withoutLinePrefix('Note: delays at Bank.', 'DLR')).toBe('Note: delays at Bank.');
    expect(withoutLinePrefix('Delays between Piccadilly and Eccles', 'Blue Line')).toBe(
      'Delays between Piccadilly and Eccles',
    );
    expect(withoutLinePrefix('Victoria:', 'Victoria')).toBe('Victoria:');
  });
});
