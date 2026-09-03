import { describe, it, expect } from 'vitest';
import { screen, fireEvent, within } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { theme } from '@/lib/theme';
import { renderWithMantine } from '@/test/render';
import { IssueList } from './IssueList';
import type { LineStatus } from '@/lib/types';
import type { IssueItem } from '@/lib/stationIssues';

/** Every pre-existing test here predates the `IssueItem` prop shape and
 * only cares about statuses, not per-line attribution — wrap each in a
 * bare `{ status }` item, same as the line detail page's real call site
 * does for its own single-line issues. */
function toItems(statuses: LineStatus[]): IssueItem[] {
  return statuses.map((status) => ({ status }));
}

const NOW = Date.now();
const now = new Date(NOW).toISOString();
const future = new Date(NOW + 86400000).toISOString();

const minorNow: LineStatus = {
  statusSeverity: 9,
  statusSeverityDescription: 'Minor Delays',
  reason: 'Signal failure',
  dataQuality: 'knowledgebase',
  sampleAvailability: { state: 'no-coverage' },
  fullCoverageAvailability: { state: 'not-enabled' },
  validityPeriods: [{ fromDate: now, toDate: null, isNow: true }],
};

const severePlanned: LineStatus = {
  statusSeverity: 4,
  statusSeverityDescription: 'Planned Closure',
  reason: 'Engineering works',
  dataQuality: 'planned',
  sampleAvailability: { state: 'no-coverage' },
  fullCoverageAvailability: { state: 'not-enabled' },
  validityPeriods: [{ fromDate: future, toDate: null, isNow: false }],
};

const inferredNow: LineStatus = {
  statusSeverity: 6,
  statusSeverityDescription: 'Severe Delays',
  reason: '10 of 12 sampled services delayed.',
  dataQuality: 'ldbws-inferred',
  sampleAvailability: { state: 'no-coverage' },
  fullCoverageAvailability: { state: 'not-enabled' },
  validityPeriods: [{ fromDate: now, toDate: null, isNow: true }],
};

const plannedRange: LineStatus = {
  statusSeverity: 4,
  statusSeverityDescription: 'Planned Closure',
  reason: 'Scheduled maintenance',
  dataQuality: 'planned',
  sampleAvailability: { state: 'no-coverage' },
  fullCoverageAvailability: { state: 'not-enabled' },
  validityPeriods: [{ fromDate: now, toDate: future, isNow: false }],
};

const all = [minorNow, severePlanned, inferredNow];

describe('IssueList', () => {
  it('renders one row per status, collapsed by default', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    fireEvent.click(screen.getByText('All (3)'));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
  });

  it('defaults to the Active tab, hiding non-active issues until All/Upcoming is picked', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });

  it('shows a "Now" validity summary on the collapsed row for active statuses', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    expect(screen.getAllByText('Now')).toHaveLength(2);
  });

  it('shows the full validity period in the expanded panel', async () => {
    renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} />);
    fireEvent.click(screen.getByText('Signal failure'));
    expect(await screen.findByText(/Valid:/)).toBeInTheDocument();
  });

  it('shows a date-range validity summary when a period has both a start and end date', () => {
    // Deliberately upcoming rather than spanning `now`: a period that spans
    // `now` is active per `periodIsActive`, and the collapsed-row summary
    // now says "Now" for any active period (see the in-progress-window
    // test below) rather than a date range, so this test needs a period
    // that is unambiguously *not* active to exercise the range formatting.
    const upcomingRange: LineStatus = {
      ...severePlanned,
      validityPeriods: [{ fromDate: future, toDate: new Date(NOW + 2 * 86400000).toISOString(), isNow: false }],
    };
    renderWithMantine(<IssueList items={toItems([upcomingRange])} now={NOW} />);
    fireEvent.click(screen.getByText(/^All/));
    expect(screen.getByText(/–/)).toBeInTheDocument();
  });

  it('shows the same date range in the expanded panel', async () => {
    renderWithMantine(<IssueList items={toItems([plannedRange])} now={NOW} />);
    fireEvent.click(screen.getByText(/^All/));
    fireEvent.click(screen.getByText('Scheduled maintenance'));
    const validityLine = await screen.findByText(/Valid:/);
    expect(validityLine.textContent).toContain('–');
  });

  it('filters by severity', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    fireEvent.click(screen.getByText(/^All/));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Minor Delays' }));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
    expect(screen.queryByText('10 of 12 sampled services delayed.')).not.toBeInTheDocument();
  });

  it('filters by source type', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    fireEvent.click(screen.getByText(/^All/));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
    expect(screen.queryByText('10 of 12 sampled services delayed.')).not.toBeInTheDocument();
  });

  it('filters to active only', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    fireEvent.click(screen.getByText('Active (2)'));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });

  it('filters to upcoming only', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    fireEvent.click(screen.getByText('Upcoming (1)'));
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
  });

  it('shows all/active/upcoming counts on the filter control', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    expect(screen.getByText('All (3)')).toBeInTheDocument();
    expect(screen.getByText('Active (2)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (1)')).toBeInTheDocument();
  });

  it('counts reflect the severity/source chip filters but not the active/upcoming filter itself', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
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
      <IssueList items={toItems([upcomingLater, activeLater, upcomingSooner, activeSooner])} now={NOW} />,
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
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Minor Delays' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    // The whole pool is filtered away regardless of tab — there's no
    // sibling tab to send the user to, only the filters to blame.
    const message = screen.getByText(/No issues match the selected .*filters/i);
    expect(message.textContent).not.toMatch(/listed under/i);
  });

  it('says the line is clear when it has no issues at all, without blaming filters', () => {
    renderWithMantine(<IssueList items={toItems([])} now={NOW} />);
    const message = screen.getByText(/No issues reported on this line/i);
    expect(message.textContent).not.toMatch(/filter/i);
  });

  it('points at the tab that holds the issues when the selected tab is empty', () => {
    renderWithMantine(<IssueList items={toItems([severePlanned])} now={NOW} />);
    fireEvent.click(screen.getByText('Active (0)'));
    // Active is empty for a structural reason (nothing is active right
    // now), not because of a chip filter — the message must not blame one.
    const message = screen.getByText(/listed under Upcoming/i);
    expect(message.textContent).not.toMatch(/filter/i);
  });

  it('points back at Active when the Upcoming tab is the empty one', () => {
    renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} />);
    fireEvent.click(screen.getByText('Upcoming (0)'));
    const message = screen.getByText(/listed under Active/i);
    expect(message.textContent).not.toMatch(/filter/i);
  });

  it('mentions the filters only when a chip is genuinely narrowing the result', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
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
        impactType: null,
      },
    };
    renderWithMantine(<IssueList items={toItems([withDisruption])} now={NOW} />);
    expect(screen.queryByText('Full details here')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Signal failure'));
    expect(await screen.findByText('Full details here')).toBeInTheDocument();
  });

  it('surfaces the "View full incident details" link when a status disruption is knowledgebase-sourced', async () => {
    const withKnowledgebaseSource: LineStatus = {
      ...minorNow,
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: 'knowledgebase-incident-123',
        impactType: null,
      },
    };
    renderWithMantine(<IssueList items={toItems([withKnowledgebaseSource])} now={NOW} />);
    fireEvent.click(screen.getByText('Signal failure'));
    expect(await screen.findByRole('link', { name: 'View full incident details' })).toBeInTheDocument();
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
    renderWithMantine(<IssueList items={toItems([severePlanned, endedRange])} now={NOW} />);
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.getByText('Scheduled maintenance')).toBeInTheDocument();
  });

  it('still lands on the Active tab when at least one issue is active', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });

  it('does not move the user off the Active tab when a chip filter empties it', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    // Active is now (0), but the landing tab is chosen once on mount, not
    // re-derived: re-deriving would yank the user to All mid-interaction
    // and reveal severePlanned, which they just filtered towards.
    expect(screen.getByText('Active (0)')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });
  it('labels what each chip row filters, and says so when nothing is narrowed', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    expect(screen.getByText('Severity — showing all')).toBeInTheDocument();
    expect(screen.getByText('Source — showing all')).toBeInTheDocument();
  });

  it('reports how many chips are selected in each row label', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    fireEvent.click(screen.getByRole('checkbox', { name: 'Minor Delays' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Severe Delays' }));
    fireEvent.click(screen.getByRole('checkbox', { name: 'Planned' }));
    expect(screen.getByText('Severity — 2 selected')).toBeInTheDocument();
    expect(screen.getByText('Source — 1 selected')).toBeInTheDocument();
  });

  it('renders selected chips in a visually distinct variant from unselected ones', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    const minor = screen.getByRole('checkbox', { name: 'Minor Delays' });
    const planned = screen.getByRole('checkbox', { name: 'Planned' });
    expect(minor.closest('[data-variant]')).toHaveAttribute('data-variant', 'outline');
    fireEvent.click(minor);
    expect(minor.closest('[data-variant]')).toHaveAttribute('data-variant', 'filled');
    // The other row is untouched, so it must still read as "off".
    expect(planned.closest('[data-variant]')).toHaveAttribute('data-variant', 'outline');
  });

  it('associates each chip row with its label for screen readers', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    expect(screen.getByRole('group', { name: 'Severity — showing all' })).toBeInTheDocument();
    expect(screen.getByRole('group', { name: 'Source — showing all' })).toBeInTheDocument();
  });
  it('keeps the row badges at full size and lets the description text absorb the squeeze', () => {
    renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} />);
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
    renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} />);
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

  it('renders the impact-type badge on the collapsed row when set', () => {
    const withImpactType: LineStatus = {
      ...minorNow,
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: null,
        impactType: 'rail_replacement_bus',
      },
    };
    renderWithMantine(<IssueList items={toItems([withImpactType])} now={NOW} />);
    expect(screen.getByText('Rail Replacement Bus')).toBeInTheDocument();
  });

  it('renders no impact-type badge on the collapsed row for the common null case', () => {
    renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} />);
    expect(screen.queryByText('Rail Replacement Bus')).not.toBeInTheDocument();
    expect(screen.queryByText('No Scheduled Service')).not.toBeInTheDocument();
    expect(screen.queryByText('Diversion')).not.toBeInTheDocument();
  });

  it('places the impact-type badge before the data-quality badge in the meta group', () => {
    const withImpactType: LineStatus = {
      ...minorNow,
      disruption: {
        category: 'RealTime',
        description: 'Full details here',
        affectedStops: [],
        affectedRoutes: [],
        source: null,
        impactType: 'no_scheduled_service',
      },
    };
    renderWithMantine(<IssueList items={toItems([withImpactType])} now={NOW} />);
    const description = screen.getByText('Signal failure');
    const control = description.closest('button') as HTMLElement;
    const meta = control.querySelector('.issueRow__meta') as HTMLElement;
    const badgeTexts = Array.from(meta.querySelectorAll('.mantine-Badge-root')).map((el) => el.textContent);
    expect(badgeTexts.indexOf('No Scheduled Service')).toBeLessThan(badgeTexts.indexOf('Knowledgebase'));
  });

  it('marks up the collapsed row so it can stack on narrow viewports', () => {
    const { container } = renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} />);
    const row = container.querySelector('.issueRow');
    expect(row).not.toBeNull();
    expect(row!.querySelector('.issueRow__badge')).not.toBeNull();
    expect(row!.querySelector('.issueRow__reason')).not.toBeNull();
    expect(row!.querySelector('.issueRow__meta')).not.toBeNull();
  });

  it('does not pin the row with an inline nowrap that a media query cannot override', () => {
    const { container } = renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} />);
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
      sampleAvailability: { state: 'no-coverage' },
      fullCoverageAvailability: { state: 'not-enabled' },
      validityPeriods: [
        {
          fromDate: new Date(NOW - 86400000).toISOString(),
          toDate: new Date(NOW + 86400000).toISOString(),
          isNow: false,
        },
      ],
    };
    renderWithMantine(<IssueList items={toItems([inProgress])} now={NOW} />);
    expect(screen.getByText('Active (1)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (0)')).toBeInTheDocument();
  });

  it('makes the tab counts add up', () => {
    const ended: LineStatus = {
      statusSeverity: 9,
      statusSeverityDescription: 'Minor Delays',
      reason: 'Finished works',
      dataQuality: 'planned',
      sampleAvailability: { state: 'no-coverage' },
      fullCoverageAvailability: { state: 'not-enabled' },
      validityPeriods: [
        {
          fromDate: new Date(NOW - 2 * 86400000).toISOString(),
          toDate: new Date(NOW - 86400000).toISOString(),
          isNow: false,
        },
      ],
    };
    renderWithMantine(<IssueList items={toItems([...all, ended])} now={NOW} />);
    expect(screen.getByText('All (4)')).toBeInTheDocument();
    expect(screen.getByText('Active (2)')).toBeInTheDocument();
    expect(screen.getByText('Upcoming (1)')).toBeInTheDocument();
    expect(screen.getByText('Ended (1)')).toBeInTheDocument();
  });

  it('hides the Ended tab entirely when nothing has ended', () => {
    renderWithMantine(<IssueList items={toItems(all)} now={NOW} />);
    expect(screen.queryByText(/^Ended/)).not.toBeInTheDocument();
  });

  it('formats validity dates as unambiguous UK dates', () => {
    const dated: LineStatus = {
      statusSeverity: 4,
      statusSeverityDescription: 'Planned Closure',
      reason: 'Station improvement work',
      dataQuality: 'planned',
      sampleAvailability: { state: 'no-coverage' },
      fullCoverageAvailability: { state: 'not-enabled' },
      validityPeriods: [
        { fromDate: '2026-05-10T00:00:00Z', toDate: '2026-10-11T00:00:00Z', isNow: false },
      ],
    };
    renderWithMantine(<IssueList items={toItems([dated])} now={Date.parse('2026-12-01T00:00:00Z')} />);
    expect(screen.getByText('10 May 2026 – 11 Oct 2026')).toBeInTheDocument();
  });

  it('names the affected lines on a row reported on more than one', async () => {
    renderWithMantine(
      <IssueList
        items={[
          {
            status: minorNow,
            lines: [
              { id: 'a', name: 'Portsmouth Direct Line' },
              { id: 'b', name: 'South West Main Line' },
            ],
          },
        ]}
        now={NOW}
      />,
    );
    expect(screen.getByText('2 lines')).toBeInTheDocument();
    fireEvent.click(screen.getByText('Signal failure'));
    // Like every other panel-content assertion in this file (see "shows the
    // full validity period in the expanded panel" above), the accordion
    // panel mounts asynchronously, so this needs findByText rather than the
    // brief's literal getByText.
    expect(await screen.findByText(/Portsmouth Direct Line, South West Main Line/)).toBeInTheDocument();
  });

  it('says nothing about lines when an issue only affects one', () => {
    renderWithMantine(
      <IssueList items={[{ status: minorNow, lines: [{ id: 'a', name: 'Alton Line' }] }]} now={NOW} />,
    );
    expect(screen.queryByText(/lines$/)).not.toBeInTheDocument();
  });

  it('replaces the filter chrome with one sentence when the line is simply fine', () => {
    const goodService: LineStatus = {
      statusSeverity: 10,
      statusSeverityDescription: 'Good Service',
      reason: 'Good Service',
      dataQuality: 'ldbws-inferred',
      sampleAvailability: { state: 'no-coverage' },
      fullCoverageAvailability: { state: 'not-enabled' },
      validityPeriods: [{ fromDate: new Date(NOW).toISOString(), toDate: null, isNow: true }],
    };
    renderWithMantine(<IssueList items={[{ status: goodService }]} now={NOW} />);
    expect(screen.getByText('Good service — no issues reported on this line.')).toBeInTheDocument();
    expect(screen.queryByText(/^All \(/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Severity —/)).not.toBeInTheDocument();
  });

  it('also collapses the chrome for TfL\'s No Issues (25), not just NR\'s Good Service (10)', () => {
    // Both severities are classified 'good' in lib/severity.ts's
    // SEVERITY_TABLE; the allGood check has to go through that
    // classification rather than hardcoding statusSeverity === 10, or a
    // TfL line whose only status is 25 would incorrectly show the full
    // filter chrome as if something were wrong.
    const noIssues: LineStatus = {
      statusSeverity: 25,
      statusSeverityDescription: 'No Issues',
      reason: 'No Issues',
      dataQuality: 'tfl',
      sampleAvailability: { state: 'no-coverage' },
      fullCoverageAvailability: { state: 'not-enabled' },
      validityPeriods: [{ fromDate: new Date(NOW).toISOString(), toDate: null, isNow: true }],
    };
    renderWithMantine(<IssueList items={[{ status: noIssues }]} now={NOW} />);
    expect(screen.getByText('Good service — no issues reported on this line.')).toBeInTheDocument();
    expect(screen.queryByText(/^All \(/)).not.toBeInTheDocument();
    expect(screen.queryByText(/Severity —/)).not.toBeInTheDocument();
  });

  it('says the station is clear (not "the line") when subject="station" and there are no issues at all', () => {
    renderWithMantine(<IssueList items={toItems([])} now={NOW} subject="station" />);
    const message = screen.getByText(/No issues reported on this station/i);
    expect(message.textContent).not.toMatch(/this line/i);
  });

  it('says the station has good service (not "the line") when subject="station" and every status is Good Service', () => {
    const goodService: LineStatus = {
      statusSeverity: 10,
      statusSeverityDescription: 'Good Service',
      reason: 'Good Service',
      dataQuality: 'ldbws-inferred',
      sampleAvailability: { state: 'no-coverage' },
      fullCoverageAvailability: { state: 'not-enabled' },
      validityPeriods: [{ fromDate: new Date(NOW).toISOString(), toDate: null, isNow: true }],
    };
    renderWithMantine(<IssueList items={[{ status: goodService }]} now={NOW} subject="station" />);
    expect(screen.getByText('Good service — no issues reported on this station.')).toBeInTheDocument();
  });

  it('says "this station" (not "this line") in the empty Active tab when subject="station"', () => {
    // severePlanned is upcoming only, so the component lands on the "All"
    // tab by default (see the landing-tab comment in IssueList.tsx) — click
    // "Active (0)" explicitly to reach the empty Active tab, same as the
    // pre-existing "points at the tab that holds the issues" test above.
    renderWithMantine(<IssueList items={toItems([severePlanned])} now={NOW} subject="station" />);
    fireEvent.click(screen.getByText('Active (0)'));
    const message = screen.getByText(/Nothing is affecting this station right now/i);
    expect(message.textContent).not.toMatch(/this line/i);
  });

  it('says "this station" (not "this line") in the empty Upcoming tab when subject="station"', () => {
    renderWithMantine(<IssueList items={toItems([minorNow])} now={NOW} subject="station" />);
    fireEvent.click(screen.getByText('Upcoming (0)'));
    const message = screen.getByText(/No issues are scheduled for later on this station/i);
    expect(message.textContent).not.toMatch(/this line/i);
  });

  // No equivalent "empty Ended tab, not chip-narrowed" case: the Ended tab
  // button in the SegmentedControl only renders at all when the current
  // chip-filtered pool has `endedCount > 0` (see the `data` array above),
  // and `chipsNarrowing` is only false when no chips are excluding
  // anything from that same pool — so an un-narrowed, genuinely empty
  // Ended tab is structurally unreachable through the UI (there is no
  // sequence of clicks that produces it), the same way the sibling
  // `earliestFromDate` NaN case documents itself as reachable only
  // defensively. The Ended lead string still reads `this ${subject}` in
  // the source identically to the Active/Upcoming leads covered above.

  it('defaults to line-centric wording when subject is omitted', () => {
    renderWithMantine(<IssueList items={toItems([severePlanned])} now={NOW} />);
    fireEvent.click(screen.getByText('Active (0)'));
    expect(screen.getByText(/Nothing is affecting this line right now/i)).toBeInTheDocument();
  });

  it('still shows the full list when a Good Service status sits alongside a real issue', () => {
    const goodService: LineStatus = { ...minorNow, statusSeverity: 10, statusSeverityDescription: 'Good Service', reason: 'Good Service' };
    renderWithMantine(
      <IssueList items={[goodService, minorNow].map((status) => ({ status }))} now={NOW} />,
    );
    expect(screen.getByText(/^All \(2\)/)).toBeInTheDocument();
  });

  /** The `allGood` early return in IssueList.tsx has to sit after every one
   * of the component's hook calls (2x `useId`, `useMemo` for
   * `statuses`/`linesByStatus`, `useState` for `severityFilter` and
   * `sourceFilter`, `useMemo` for `buckets`, `useState` for
   * `activeFilter`) — not "immediately after the statuses/linesByStatus
   * memos", which is where it was first drafted. An early return placed
   * there would make React call a different number of hooks depending on
   * whether every status is Good Service, which is a Rules-of-Hooks
   * violation: React requires the same mounted instance to call the same
   * hooks, in the same order, on every render.
   *
   * A fresh `renderWithMantine` call per test — which is what every other
   * test in this file does — cannot catch that class of bug: a brand-new
   * mount is always free to call whatever hooks it likes. Only rerendering
   * the *same* instance with different `items` exposes a hook-count
   * mismatch between renders, so these two tests deliberately use RTL's
   * `rerender` (re-wrapped in a fresh `MantineProvider`, since
   * `renderWithMantine`'s wrapper isn't preserved across a bare
   * `rerender` call) instead of two separate mounts.
   *
   * This repo has no `eslint-plugin-react-hooks` (no ESLint config at
   * all), so there is no lint-level safety net for this — these tests are
   * the only thing that would catch a regression if the early return ever
   * moved back above one of the hooks it now follows. */
  it('does not break React hook ordering when a mounted instance goes from all Good Service to a mixed set', () => {
    const goodService: LineStatus = {
      statusSeverity: 10,
      statusSeverityDescription: 'Good Service',
      reason: 'Good Service',
      dataQuality: 'ldbws-inferred',
      sampleAvailability: { state: 'no-coverage' },
      fullCoverageAvailability: { state: 'not-enabled' },
      validityPeriods: [{ fromDate: now, toDate: null, isNow: true }],
    };
    const { rerender } = renderWithMantine(<IssueList items={toItems([goodService])} now={NOW} />);
    expect(screen.getByText('Good service — no issues reported on this line.')).toBeInTheDocument();

    rerender(
      <MantineProvider theme={theme}>
        <IssueList items={toItems([goodService, minorNow])} now={NOW} />
      </MantineProvider>,
    );
    expect(screen.getByText(/^All \(2\)/)).toBeInTheDocument();
    expect(screen.queryByText('Good service — no issues reported on this line.')).not.toBeInTheDocument();
  });

  it('does not break React hook ordering when a mounted instance goes from a mixed set to all Good Service', () => {
    const goodService: LineStatus = {
      statusSeverity: 10,
      statusSeverityDescription: 'Good Service',
      reason: 'Good Service',
      dataQuality: 'ldbws-inferred',
      sampleAvailability: { state: 'no-coverage' },
      fullCoverageAvailability: { state: 'not-enabled' },
      validityPeriods: [{ fromDate: now, toDate: null, isNow: true }],
    };
    const { rerender } = renderWithMantine(
      <IssueList items={toItems([goodService, minorNow])} now={NOW} />,
    );
    expect(screen.getByText(/^All \(2\)/)).toBeInTheDocument();

    rerender(
      <MantineProvider theme={theme}>
        <IssueList items={toItems([goodService])} now={NOW} />
      </MantineProvider>,
    );
    expect(screen.getByText('Good service — no issues reported on this line.')).toBeInTheDocument();
    expect(screen.queryByText(/^All \(/)).not.toBeInTheDocument();
  });
});
