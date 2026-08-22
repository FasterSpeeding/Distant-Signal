import { describe, it, expect } from 'vitest';
import { screen, fireEvent, within } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { IssueList } from './IssueList';
import type { LineStatus } from '@/lib/types';

const NOW = Date.now();
const now = new Date(NOW).toISOString();
const future = new Date(NOW + 86400000).toISOString();

const minorNow: LineStatus = {
  statusSeverity: 9,
  statusSeverityDescription: 'Minor Delays',
  reason: 'Signal failure',
  dataQuality: 'knowledgebase',
  validityPeriods: [{ fromDate: now, toDate: null, isNow: true }],
};

const severePlanned: LineStatus = {
  statusSeverity: 4,
  statusSeverityDescription: 'Planned Closure',
  reason: 'Engineering works',
  dataQuality: 'planned',
  validityPeriods: [{ fromDate: future, toDate: null, isNow: false }],
};

const inferredNow: LineStatus = {
  statusSeverity: 6,
  statusSeverityDescription: 'Severe Delays',
  reason: '10 of 12 sampled services delayed.',
  dataQuality: 'ldbws-inferred',
  validityPeriods: [{ fromDate: now, toDate: null, isNow: true }],
};

const plannedRange: LineStatus = {
  statusSeverity: 4,
  statusSeverityDescription: 'Planned Closure',
  reason: 'Scheduled maintenance',
  dataQuality: 'planned',
  validityPeriods: [{ fromDate: now, toDate: future, isNow: false }],
};

const all = [minorNow, severePlanned, inferredNow];

describe('IssueList', () => {
  it('renders one row per status, collapsed by default', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    fireEvent.click(screen.getByText('All (3)'));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
  });

  it('defaults to the Active tab, hiding non-active issues until All/Upcoming is picked', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });

  it('shows a "Now" validity summary on the collapsed row for active statuses', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    expect(screen.getAllByText('Now')).toHaveLength(2);
  });

  it('shows the full validity period in the expanded panel', async () => {
    renderWithMantine(<IssueList statuses={[minorNow]} now={NOW} />);
    fireEvent.click(screen.getByText('Signal failure'));
    expect(await screen.findByText(/Valid:/)).toBeInTheDocument();
  });

  it('shows a date-range validity summary when a period has both a start and end date', () => {
    renderWithMantine(<IssueList statuses={[plannedRange]} now={NOW} />);
    // plannedRange spans `now` (fromDate: now, toDate: future), so it lands
    // on the default Active tab already; clicking All is harmless and keeps
    // this test about date-range formatting, not the active/upcoming filter.
    fireEvent.click(screen.getByText(/^All/));
    expect(screen.getByText(/–/)).toBeInTheDocument();
  });

  it('shows the same date range in the expanded panel', async () => {
    renderWithMantine(<IssueList statuses={[plannedRange]} now={NOW} />);
    fireEvent.click(screen.getByText(/^All/));
    fireEvent.click(screen.getByText('Scheduled maintenance'));
    const validityLine = await screen.findByText(/Valid:/);
    expect(validityLine.textContent).toContain('–');
  });

  it('filters by severity', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    fireEvent.click(screen.getByText(/^All/));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Minor Delays' }));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
    expect(screen.queryByText('10 of 12 sampled services delayed.')).not.toBeInTheDocument();
  });

  it('filters by source type', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    fireEvent.click(screen.getByText(/^All/));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
    expect(screen.queryByText('10 of 12 sampled services delayed.')).not.toBeInTheDocument();
  });

  it('filters to active only', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    fireEvent.click(screen.getByText('Active (2)'));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });

  it('filters to upcoming only', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    fireEvent.click(screen.getByText('Upcoming (1)'));
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
  });

  it('shows all/active/upcoming counts on the filter control', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    expect(screen.getByText('All (3)')).toBeInTheDocument();
    expect(screen.getByText('Active (2)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (1)')).toBeInTheDocument();
  });

  it('counts reflect the severity/source chip filters but not the active/upcoming filter itself', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    // Select the Active tab, then narrow to "Planned Closure" severity —
    // which matches only severePlanned, an *upcoming* status with zero
    // overlap with "Active". A tab-dependent (buggy) count implementation
    // would compute counts from the already-active-only `filtered` array,
    // landing on All(0)/Active(0)/Upcoming(0); the correct, tab-independent
    // implementation computes them from the chip-filtered-only pool, so
    // Active should read 0 while Upcoming still reads 1.
    fireEvent.click(screen.getByText('Active (2)'));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned Closure' }));
    expect(screen.getByText('All (1)')).toBeInTheDocument();
    expect(screen.getByText('Active (0)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (1)')).toBeInTheDocument();
  });

  it('sorts active issues before upcoming issues, each ordered by start date ascending', () => {
    const activeSooner: LineStatus = {
      ...minorNow,
      reason: 'Active started sooner',
      validityPeriods: [{ fromDate: new Date(Date.now() + 3600_000).toISOString(), toDate: null, isNow: true }],
    };
    const activeLater: LineStatus = {
      ...minorNow,
      reason: 'Active started later',
      validityPeriods: [{ fromDate: new Date(Date.now() + 7200_000).toISOString(), toDate: null, isNow: true }],
    };
    const upcomingSooner: LineStatus = {
      ...severePlanned,
      reason: 'Upcoming starts sooner',
      validityPeriods: [{ fromDate: new Date(Date.now() + 86_400_000).toISOString(), toDate: null, isNow: false }],
    };
    const upcomingLater: LineStatus = {
      ...severePlanned,
      reason: 'Upcoming starts later',
      validityPeriods: [{ fromDate: new Date(Date.now() + 172_800_000).toISOString(), toDate: null, isNow: false }],
    };
    // Deliberately scrambled input order — the component must sort it.
    renderWithMantine(
      <IssueList statuses={[upcomingLater, activeLater, upcomingSooner, activeSooner]} now={NOW} />,
    );
    // The upcoming items are hidden under the default Active-only tab —
    // this test is about sort order across both groups, so switch to All.
    fireEvent.click(screen.getByText(/^All/));

    const rows = screen.getAllByText(/^(Active|Upcoming) (started|starts)/);
    expect(rows.map((el) => el.textContent)).toEqual([
      'Active started sooner',
      'Active started later',
      'Upcoming starts sooner',
      'Upcoming starts later',
    ]);
  });

  it('shows a message when no issues match the filters', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Minor Delays' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    // The whole pool is filtered away regardless of tab — there's no
    // sibling tab to send the user to, only the filters to blame.
    const message = screen.getByText(/No issues match the selected .*filters/i);
    expect(message.textContent).not.toMatch(/listed under/i);
  });

  it('says the line is clear when it has no issues at all, without blaming filters', () => {
    renderWithMantine(<IssueList statuses={[]} now={NOW} />);
    const message = screen.getByText(/No issues reported on this line/i);
    expect(message.textContent).not.toMatch(/filter/i);
  });

  it('points at the tab that holds the issues when the selected tab is empty', () => {
    renderWithMantine(<IssueList statuses={[severePlanned]} now={NOW} />);
    fireEvent.click(screen.getByText('Active (0)'));
    // Active is empty for a structural reason (nothing is active right
    // now), not because of a chip filter — the message must not blame one.
    const message = screen.getByText(/listed under Upcoming/i);
    expect(message.textContent).not.toMatch(/filter/i);
  });

  it('points back at Active when the Upcoming tab is the empty one', () => {
    renderWithMantine(<IssueList statuses={[minorNow]} now={NOW} />);
    fireEvent.click(screen.getByText('Upcoming (0)'));
    const message = screen.getByText(/listed under Active/i);
    expect(message.textContent).not.toMatch(/filter/i);
  });

  it('mentions the filters only when a chip is genuinely narrowing the result', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    // Stays on the Active tab (see 2a) with only severePlanned in the pool,
    // so the tab is empty *because of* the chip — filters are fair to blame,
    // unlike the structurally-empty case above which points at the same
    // sibling tab but must not mention filters.
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    const message = screen.getByText(/listed under Upcoming/i);
    expect(message.textContent).toMatch(/filter/i);
  });

  it('expands an entry to reveal its detail on click', async () => {
    const withDisruption: LineStatus = {
      ...minorNow,
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: null,
      },
    };
    renderWithMantine(<IssueList statuses={[withDisruption]} now={NOW} />);
    expect(screen.queryByText('Full details here')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Signal failure'));
    expect(await screen.findByText('Full details here')).toBeInTheDocument();
  });
  it('lands on the All tab when no issue is active', () => {
    // severePlanned is upcoming and endedRange has already finished (both
    // dates in the past), so Active reads (0) while All reads (2): landing
    // on Active would show "nothing" next to tabs that say there is
    // something. (Unlike plannedRange, which spans `now` and is correctly
    // bucketed Active by the fix this test predates.)
    const endedRange: LineStatus = {
      ...plannedRange,
      validityPeriods: [
        {
          fromDate: new Date(NOW - 2 * 86400000).toISOString(),
          toDate: new Date(NOW - 86400000).toISOString(),
          isNow: false,
        },
      ],
    };
    renderWithMantine(<IssueList statuses={[severePlanned, endedRange]} now={NOW} />);
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.getByText('Scheduled maintenance')).toBeInTheDocument();
  });

  it('still lands on the Active tab when at least one issue is active', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });

  it('does not move the user off the Active tab when a chip filter empties it', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    // Active is now (0), but the landing tab is chosen once on mount, not
    // re-derived: re-deriving would yank the user to All mid-interaction
    // and reveal severePlanned, which they just filtered towards.
    expect(screen.getByText('Active (0)')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });
  it('labels what each chip row filters, and says so when nothing is narrowed', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    expect(screen.getByText('Severity — showing all')).toBeInTheDocument();
    expect(screen.getByText('Source — showing all')).toBeInTheDocument();
  });

  it('reports how many chips are selected in each row label', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Minor Delays' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Severe Delays' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    expect(screen.getByText('Severity — 2 selected')).toBeInTheDocument();
    expect(screen.getByText('Source — 1 selected')).toBeInTheDocument();
  });

  it('renders selected chips in a visually distinct variant from unselected ones', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    const minor = screen.getByRole('checkbox', { name: 'Minor Delays' });
    const planned = screen.getByRole('checkbox', { name: 'Planned' });
    expect(minor.closest('[data-variant]')).toHaveAttribute('data-variant', 'outline');
    fireEvent.click(minor);
    expect(minor.closest('[data-variant]')).toHaveAttribute('data-variant', 'filled');
    // The other row is untouched, so it must still read as "off".
    expect(planned.closest('[data-variant]')).toHaveAttribute('data-variant', 'outline');
  });

  it('associates each chip row with its label for screen readers', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    expect(screen.getByRole('group', { name: 'Severity — showing all' })).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Source — showing all' })).toBeInTheDocument();
  });
  it('keeps the row badges at full size and lets the description text absorb the squeeze', () => {
    renderWithMantine(<IssueList statuses={[minorNow]} now={NOW} />);
    const description = screen.getByText('Signal failure');
    const control = description.closest('button') as HTMLElement;
    // The two badges classify the row — they must sit in boxes that refuse
    // to shrink, rather than truncating to "MINOR DEL…" / a circled letter.
    // They now use class-based layout from app/globals.css instead of inline styles.
    expect(within(control).getByText('Minor Delays').closest('.issueRow__badge')).not.toBeNull();
    expect(within(control).getByText('Knowledgebase').closest('.issueRow__meta')).not.toBeNull();
    // The description is the element that gives way instead.
    expect(description).toHaveClass('issueRow__reason');
  });

  it('renders the data-quality badge as neutral gray, not the brand colour', () => {
    renderWithMantine(<IssueList statuses={[minorNow]} now={NOW} />);
    const description = screen.getByText('Signal failure');
    const control = description.closest('button') as HTMLElement;
    // Provenance is metadata, not brand — it must not ride the theme's
    // primaryColor (grape) fallback, which would make it read as branded
    // or interactive. Checked via the CSS var Mantine's outline variant
    // resolves the colour into, since asserting an exact rendered shade
    // would be brittle.
    const badge = within(control).getByText('Knowledgebase').closest('.mantine-Badge-root') as HTMLElement;
    expect(badge.getAttribute('style')).toContain('--mantine-color-gray-outline');
    expect(badge.getAttribute('style')).not.toContain('--mantine-color-grape-outline');
  });

  it('marks up the collapsed row so it can stack on narrow viewports', () => {
    const { container } = renderWithMantine(<IssueList statuses={[minorNow]} now={NOW} />);
    const row = container.querySelector('.issueRow');
    expect(row).not.toBeNull();
    expect(row!.querySelector('.issueRow__badge')).not.toBeNull();
    expect(row!.querySelector('.issueRow__reason')).not.toBeNull();
    expect(row!.querySelector('.issueRow__meta')).not.toBeNull();
  });

  it('does not pin the row with an inline nowrap that a media query cannot override', () => {
    const { container } = renderWithMantine(<IssueList statuses={[minorNow]} now={NOW} />);
    const row = container.querySelector('.issueRow') as HTMLElement;
    expect(row.style.flexWrap).toBe('');
  });

  it('counts an in-progress dated window as Active even though isNow is false', () => {
    // The exact shape that produced "All (1) / Active (0) / Upcoming (0)".
    const inProgress: LineStatus = {
      statusSeverity: 4,
      statusSeverityDescription: 'Planned Closure',
      reason: 'Station improvement work',
      dataQuality: 'planned',
      validityPeriods: [
        {
          fromDate: new Date(NOW - 86400000).toISOString(),
          toDate: new Date(NOW + 86400000).toISOString(),
          isNow: false,
        },
      ],
    };
    renderWithMantine(<IssueList statuses={[inProgress]} now={NOW} />);
    expect(screen.getByText('Active (1)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (0)')).toBeInTheDocument();
  });

  it('makes the tab counts add up', () => {
    const ended: LineStatus = {
      statusSeverity: 9,
      statusSeverityDescription: 'Minor Delays',
      reason: 'Finished works',
      dataQuality: 'planned',
      validityPeriods: [
        {
          fromDate: new Date(NOW - 2 * 86400000).toISOString(),
          toDate: new Date(NOW - 86400000).toISOString(),
          isNow: false,
        },
      ],
    };
    renderWithMantine(<IssueList statuses={[...all, ended]} now={NOW} />);
    expect(screen.getByText('All (4)')).toBeInTheDocument();
    expect(screen.getByText('Active (2)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (1)')).toBeInTheDocument();
    expect(screen.getByText('Ended (1)')).toBeInTheDocument();
  });

  it('hides the Ended tab entirely when nothing has ended', () => {
    renderWithMantine(<IssueList statuses={all} now={NOW} />);
    expect(screen.queryByText(/^Ended/)).not.toBeInTheDocument();
  });
});
