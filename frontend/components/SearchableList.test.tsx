import { describe, it, expect, vi, afterEach } from 'vitest';
import { act, fireEvent, screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { matchingItems, SearchableList } from './SearchableList';

const ITEMS = [
  { key: 'SW', text: 'South Western Railway SW', node: <span>South Western Railway</span> },
  { key: 'GW', text: 'Great Western Railway GW', node: <span>Great Western Railway</span> },
  { key: 'XR', text: 'Elizabeth line XR', node: <span>Elizabeth line</span> },
];

afterEach(() => {
  vi.useRealTimers();
  window.history.replaceState(null, '', '/');
});

describe('matchingItems', () => {
  it('matches every word, in any order, ignoring case', () => {
    expect(matchingItems(ITEMS, 'railway WEST').map((item) => item.key)).toEqual(['SW', 'GW']);
    expect(matchingItems(ITEMS, 'rail south').map((item) => item.key)).toEqual(['SW']);
    expect(matchingItems(ITEMS, 'xr').map((item) => item.key)).toEqual(['XR']);
    expect(matchingItems(ITEMS, '   ')).toHaveLength(3);
  });
});

describe('SearchableList', () => {
  it('filters in place, says how many match, and shows them all again', () => {
    renderWithMantine(
      <SearchableList
        items={ITEMS}
        searchLabel="Find an operator"
        noun={{ one: 'operator', other: 'operators' }}
        titlePrefix="Operators"
        layout="grid"
      />,
    );
    expect(screen.queryByRole('button', { name: /^Filters/ })).not.toBeInTheDocument();
    fireEvent.change(screen.getByRole('textbox', { name: 'Find an operator' }), { target: { value: 'western' } });
    expect(screen.getByText('South Western Railway')).toBeInTheDocument();
    expect(screen.queryByText('Elizabeth line')).not.toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('2 operators match.');

    fireEvent.click(screen.getByRole('button', { name: 'Show all operators' }));
    expect(screen.getByText('Elizabeth line')).toBeInTheDocument();
  });

  it('says so when nothing matches', () => {
    renderWithMantine(
      <SearchableList
        items={ITEMS}
        searchLabel="Find a stop"
        noun={{ one: 'stop', other: 'stops' }}
        titlePrefix="Metrolink stops"
        initialQuery="zzz"
        layout="list"
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('No stops match.');
    expect(screen.queryAllByRole('listitem')).toHaveLength(0);
  });

  it('restores the search from the URL and keeps it there as q', () => {
    vi.useFakeTimers();
    window.history.replaceState(null, '', '/transit/metrolink?q=pic');
    renderWithMantine(
      <SearchableList
        items={ITEMS}
        searchLabel="Find a stop"
        noun={{ one: 'stop', other: 'stops' }}
        titlePrefix="Metrolink stops"
        initialQuery="pic"
        layout="list"
      />,
    );
    expect(screen.getByRole('textbox', { name: 'Find a stop' })).toHaveValue('pic');
    fireEvent.change(screen.getByRole('textbox', { name: 'Find a stop' }), { target: { value: 'eliz' } });
    act(() => {
      vi.advanceTimersByTime(400);
    });
    expect(window.location.search).toBe('?q=eliz');
    expect(screen.getAllByRole('listitem')).toHaveLength(1);
    expect(document.title).toContain('Metrolink stops: "eliz"');
  });
});
