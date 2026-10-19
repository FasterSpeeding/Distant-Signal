import { describe, it, expect, vi, afterEach } from 'vitest';
import { act, fireEvent, screen, render, waitFor } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { countLabel, ListFilterBar } from './ListFilterBar';
import { ListResultSummary } from './ListResultSummary';
import { replaceUrlParams, useUrlFilterSync } from '@/lib/useUrlFilterSync';
import { useFilteredDocumentTitle } from '@/lib/useFilteredDocumentTitle';

const NOUN = { one: 'incident', other: 'incidents' };

describe('ListFilterBar', () => {
  it('renders a search box and a Filters button whose panel shows the count', () => {
    const onQueryChange = vi.fn();
    renderWithMantine(
      <ListFilterBar
        searchLabel="Find an incident"
        query="sig"
        onQueryChange={onQueryChange}
        noun={NOUN}
        matchCount={3}
      >
        <span>Panel field</span>
      </ListFilterBar>,
    );
    fireEvent.change(screen.getByRole('textbox', { name: 'Find an incident' }), { target: { value: 'signal' } });
    expect(onQueryChange).toHaveBeenCalledWith('signal');
    fireEvent.click(screen.getByRole('button', { name: 'Clear search' }));
    expect(onQueryChange).toHaveBeenCalledWith('');

    fireEvent.click(screen.getByRole('button', { name: /Filters/ }));
    expect(screen.getByRole('dialog', { name: 'Filters' })).toBeInTheDocument();
    expect(screen.getByText('Panel field')).toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Show 3 incidents' })).toBeInTheDocument();
  });

  it('says "Show incidents" while the count is unknown, and Close with no match', () => {
    const { unmount } = renderWithMantine(
      <ListFilterBar searchLabel="Find an incident" noun={NOUN} matchCount={null}>
        <span>Panel field</span>
      </ListFilterBar>,
    );
    fireEvent.click(screen.getByRole('button', { name: /Filters/ }));
    expect(screen.getByRole('button', { name: 'Show incidents' })).toBeInTheDocument();
    unmount();
    renderWithMantine(
      <ListFilterBar searchLabel="Find an incident" noun={NOUN} matchCount={0}>
        <span>Panel field</span>
      </ListFilterBar>,
    );
    fireEvent.click(screen.getByRole('button', { name: /Filters/ }));
    expect(screen.getByRole('button', { name: 'Close' })).toBeInTheDocument();
    expect(screen.getByText('No incidents match these filters.')).toBeInTheDocument();
  });

  it('badges the active filter count and offers Clear filters only when something is on', () => {
    const onClearFilters = vi.fn();
    renderWithMantine(
      <ListFilterBar
        searchLabel="Find an incident"
        noun={NOUN}
        matchCount={2}
        activeCount={2}
        onClearFilters={onClearFilters}
      >
        <span>Panel field</span>
      </ListFilterBar>,
    );
    expect(screen.getByTestId('list-filter-count')).toHaveTextContent('2');
    expect(screen.getByRole('button', { name: /Filters/ })).toHaveAccessibleDescription('2 on');
    fireEvent.click(screen.getByRole('button', { name: /Filters/ }));
    fireEvent.click(screen.getByRole('button', { name: 'Clear filters' }));
    expect(onClearFilters).toHaveBeenCalled();
  });

  it('is search-only without children, and filters-only without a search label', () => {
    const { unmount } = renderWithMantine(<ListFilterBar searchLabel="Find an operator" noun={NOUN} matchCount={1} />);
    expect(screen.getByRole('textbox', { name: 'Find an operator' })).toBeInTheDocument();
    expect(screen.queryByRole('button', { name: /Filters/ })).not.toBeInTheDocument();
    unmount();
    renderWithMantine(
      <ListFilterBar noun={NOUN} matchCount={1} sticky={false} trailing={<span>Tabs</span>}>
        <span>Panel field</span>
      </ListFilterBar>,
    );
    expect(screen.queryByRole('textbox')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Filters/ })).toBeInTheDocument();
    expect(screen.getByText('Tabs')).toBeInTheDocument();
    expect(document.querySelector('.listFilterBar')).not.toHaveAttribute('data-sticky');
    expect(document.querySelector('.listFilterBarSentinel')).toBeNull();
  });

  it('swaps the search glyph for a spinner while pending', () => {
    renderWithMantine(<ListFilterBar searchLabel="Find an incident" noun={NOUN} matchCount={null} pending />);
    expect(screen.getByTestId('list-filter-pending')).toBeInTheDocument();
  });

  it('closes on Escape, but not when Escape only closed an open option list', () => {
    renderWithMantine(
      <ListFilterBar searchLabel="Find an incident" noun={NOUN} matchCount={1}>
        <input role="combobox" aria-label="Combo" aria-expanded="true" aria-controls="combo-list" />
      </ListFilterBar>,
    );
    const button = screen.getByRole('button', { name: /Filters/ });
    fireEvent.click(button);
    fireEvent.keyDown(screen.getByRole('combobox', { name: 'Combo' }), { key: 'Escape' });
    expect(button).toHaveAttribute('aria-expanded', 'true');
    fireEvent.keyDown(screen.getByRole('dialog', { name: 'Filters' }), { key: 'Escape' });
    expect(button).toHaveAttribute('aria-expanded', 'false');
  });
});

describe('countLabel', () => {
  it('picks the singular or plural noun', () => {
    expect(countLabel(1, NOUN)).toBe('1 incident');
    expect(countLabel(0, NOUN)).toBe('0 incidents');
    expect(countLabel(12, NOUN)).toBe('12 incidents');
  });
});

describe('ListResultSummary', () => {
  it('announces and shows the summary only when visible, with a reset button', () => {
    const onReset = vi.fn();
    const first = renderWithMantine(
      <ListResultSummary visible={false} summary="3 lines match." resetLabel="Show all lines" onReset={onReset} />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('');
    first.unmount();
    const second = renderWithMantine(
      <ListResultSummary visible summary="3 lines match." resetLabel="Show all lines" onReset={onReset} />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('3 lines match.');
    fireEvent.click(screen.getByRole('button', { name: 'Show all lines' }));
    expect(onReset).toHaveBeenCalled();
    second.unmount();
    renderWithMantine(
      <ListResultSummary
        visible
        summary="3 lines match."
        resetLabel="Show all lines"
        onReset={onReset}
        announcement="Searching"
      />,
    );
    expect(screen.getByRole('status')).toHaveTextContent('Searching');
  });
});

describe('replaceUrlParams', () => {
  it('replaces only the named params, appending arrays and dropping empty values', () => {
    expect(
      replaceUrlParams(
        'https://x.test/lines/a?dir=up&issueTab=all&issueSeverity=6#top',
        ['issueTab', 'issueSeverity'],
        {
          issueTab: 'active',
          issueSeverity: ['6', '9'],
          issueSource: '',
        },
      ),
    ).toBe('/lines/a?dir=up&issueTab=active&issueSeverity=6&issueSeverity=9#top');
    expect(replaceUrlParams('https://x.test/operators?q=sw', ['q'], { q: null })).toBe('/operators');
  });
});

function SyncProbe({ value }: { value: string }) {
  useUrlFilterSync({ value }, (current, state) => replaceUrlParams(current, ['v'], { v: state.value }));
  return null;
}

function TitleProbe({ title }: { title: string | null }) {
  useFilteredDocumentTitle(title);
  return null;
}

describe('useUrlFilterSync', () => {
  afterEach(() => {
    vi.useRealTimers();
    window.history.replaceState(null, '', '/');
  });

  it('never rewrites the URL on mount, then writes it debounced with replaceState', () => {
    vi.useFakeTimers();
    window.history.replaceState(null, '', '/list?keep=1');
    const spy = vi.spyOn(window.history, 'replaceState');
    const { rerender } = render(<SyncProbe value="" />);
    act(() => {
      vi.advanceTimersByTime(1000);
    });
    expect(spy).not.toHaveBeenCalled();
    rerender(<SyncProbe value="a" />);
    rerender(<SyncProbe value="ab" />);
    act(() => {
      vi.advanceTimersByTime(299);
    });
    expect(spy).not.toHaveBeenCalled();
    act(() => {
      vi.advanceTimersByTime(1);
    });
    expect(spy).toHaveBeenCalledTimes(1);
    expect(window.location.search).toBe('?keep=1&v=ab');
    spy.mockRestore();
  });
});

describe('useFilteredDocumentTitle', () => {
  it('names the filter, then restores the title the page loaded with', () => {
    document.title = 'Incidents · Distant Signal';
    const { rerender } = render(<TitleProbe title={'Incidents: "signal"'} />);
    expect(document.title).toContain('Incidents: "signal"');
    rerender(<TitleProbe title={null} />);
    expect(document.title).toBe('Incidents · Distant Signal');
  });

  // DS-53: React re-wrote the metadata <title>'s text after the hook had
  // set the filtered title, so the tab went back to the unfiltered one.
  it("puts the filtered title back when something rewrites the <title>'s text", async () => {
    document.title = 'Operators · Distant Signal';
    render(<TitleProbe title={'Operators: "zzzz"'} />);
    expect(document.title).toBe('Operators: "zzzz" · Distant Signal');

    // What React's late text update does: the text node itself, not the
    // `document.title` setter.
    const text = document.head.querySelector('title')!.firstChild as Text;
    text.data = 'Operators · Distant Signal';
    await waitFor(() => expect(document.title).toBe('Operators: "zzzz" · Distant Signal'));
  });

  it('stops watching once the filter is cleared, or the list is gone', async () => {
    document.title = 'Operators · Distant Signal';
    const { rerender, unmount } = render(<TitleProbe title={'Operators: "zzzz"'} />);
    rerender(<TitleProbe title={null} />);
    document.title = 'Something else';
    await Promise.resolve();
    expect(document.title).toBe('Something else');

    rerender(<TitleProbe title={'Operators: "zzzz"'} />);
    unmount();
    // The next page's own title, rendered as this one goes away.
    document.title = 'Lines · Distant Signal';
    await Promise.resolve();
    expect(document.title).toBe('Lines · Distant Signal');
  });
});
