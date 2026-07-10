# Outage Page Redesign + HTML Rendering Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure `/lines/{id}` and `/stations/{crs}` around a status header, a representative-info block, and a filterable/expandable issue list, and fix `disruption.description` rendering as escaped HTML instead of formatted content.

**Architecture:** Two new presentational/interactive components (`RepresentativeInfo`, a server-safe function; `IssueList`, a Client Component owning filter/expand state) replace the inline `.map()` loop currently duplicated across both pages. `worstStatus` is extracted from `LineStatusCard.tsx` into `lib/severity.ts` so the new status header and the existing card share one implementation. `DisruptionDetail` becomes a Client Component that sanitizes HTML with `isomorphic-dompurify` before rendering it.

**Tech Stack:** Next.js App Router (Server + Client Components), Mantine, TypeScript, `isomorphic-dompurify`.

## Global Constraints

- `sampleStats`-derived "representative info" is shown only when at least one status in the set carries `sampleStats` — omitted entirely (not zeroed out) otherwise.
- The issue list filters by: severity (dynamic, derived from what's present), source type (`dataQuality`, all 4 fixed values always offered), and active-vs-upcoming (`validityPeriods[].isNow` for active; `fromDate` in the future for upcoming). Filtering is client-side against already-fetched data — no re-fetch on filter change.
- HTML sanitization allowlist: `p`, `br`, `strong`, `b`, `em`, `i`, `ul`, `ol`, `li`, `a` (href only) — every surviving `<a>` gets `target="_blank" rel="noopener"` forced on regardless of what the source HTML specified.
- `/stations/{crs}` can return multiple `LineStatusReport`s (a station can sit on several lines). This plan keeps the existing per-line grouping (line name + divider) and applies the new status-header/representative-info/issue-list structure *within* each line's section, rather than flattening all lines' issues into one undifferentiated list — preserving line attribution, which the design doc doesn't explicitly address for the multi-report case.
- This plan runs after `2026-07-10-frontend-personalization.md` — `app/stations/[crs]/page.tsx`'s full-replacement steps in this plan include that earlier plan's `PinToggle`/`getPreferences` additions, since this plan replaces the whole file again.

---

### Task 1: Extract `worstStatus` into `lib/severity.ts`

**Files:**
- Modify: `frontend/lib/severity.ts`
- Modify: `frontend/lib/severity.test.ts`
- Modify: `frontend/components/LineStatusCard.tsx`

**Interfaces:**
- Produces: `export function worstStatus(report: LineStatusReport): LineStatus | { statusSeverity: number; reason: string }` in `lib/severity.ts`. Consumed by Task 5 and Task 6.
- `LineStatusCard.test.tsx` needs no changes — it only asserts rendered output, never imports `worstStatus` directly (confirmed by reading the file).

- [ ] **Step 1: Write the failing test**

Add to `frontend/lib/severity.test.ts`. First add the needed import at the top (the file currently imports only `severityColor, severityLabel` from `./severity`):

```typescript
import { describe, it, expect } from 'vitest';
import { severityColor, severityLabel, worstStatus } from './severity';
import type { LineStatusReport } from './types';
```

Then add this new `describe` block at the end of the file:

```typescript
describe('worstStatus', () => {
  const baseReport: LineStatusReport = {
    $type: 'NRStatus.LineStatusReport',
    id: 'wcml',
    name: 'West Coast Main Line',
    modeName: 'national-rail',
    operators: ['AW'],
    lineStatuses: [],
  };

  it('returns Good Service when there are no statuses', () => {
    const worst = worstStatus(baseReport);
    expect(worst.statusSeverity).toBe(10);
  });

  it('picks the most severe status by rank, not the lowest statusSeverity number', () => {
    const report: LineStatusReport = {
      ...baseReport,
      lineStatuses: [
        { statusSeverity: 10, statusSeverityDescription: 'Good Service', reason: '', dataQuality: 'knowledgebase', validityPeriods: [] },
        { statusSeverity: 21, statusSeverityDescription: 'Diverted', reason: 'Diverted', dataQuality: 'knowledgebase', validityPeriods: [] },
      ],
    };
    const worst = worstStatus(report);
    expect(worst.statusSeverity).toBe(21);
    expect(worst.reason).toBe('Diverted');
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run lib/severity.test.ts`
Expected: FAIL to compile — `worstStatus` doesn't exist in `./severity` yet.

- [ ] **Step 3: Implement**

In `frontend/lib/severity.ts`, add the import and the function at the end of the file:

```typescript
import type { LineStatusReport } from './types';
```

```typescript
/** Picks the most severe status on a report by true severity rank (see
 * `severityRank`), not by the raw `statusSeverity` number. Returns a
 * synthetic Good-Service-shaped object when the report has no statuses at
 * all. */
export function worstStatus(report: LineStatusReport) {
  if (report.lineStatuses.length === 0) {
    return { statusSeverity: 10, reason: '' };
  }
  return report.lineStatuses.reduce((worst, current) =>
    severityRank(current.statusSeverity) > severityRank(worst.statusSeverity) ? current : worst,
  );
}
```

In `frontend/components/LineStatusCard.tsx`, remove the private `worstStatus` function and the now-unused `severityRank` import, and import `worstStatus` from `lib/severity` instead:

```tsx
'use client';

import { Card, Group, Text, Stack } from '@mantine/core';
import Link from 'next/link';
import { StatusBadge } from './StatusBadge';
import { worstStatus } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';

export function LineStatusCard({ report }: { report: LineStatusReport }) {
  const worst = worstStatus(report);
  return (
    <Card withBorder shadow="sm" padding="lg" component={Link} href={`/lines/${report.id}`}>
      <Stack gap="xs">
        <Group justify="space-between">
          <Text fw={600}>{report.name}</Text>
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
        <Text size="sm" c="dimmed">
          {worst.reason}
        </Text>
      </Stack>
    </Card>
  );
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cd frontend && npx vitest run lib/severity.test.ts components/LineStatusCard.test.tsx`
Expected: PASS — both files' full suites, including the 2 new tests and all 5 pre-existing `LineStatusCard` tests unaffected.

- [ ] **Step 5: Commit**

```bash
git add frontend/lib/severity.ts frontend/lib/severity.test.ts frontend/components/LineStatusCard.tsx
git commit -m "Extract worstStatus into lib/severity.ts as a shared helper"
```

---

### Task 2: Fix `disruption.description` HTML rendering

**Files:**
- Modify: `frontend/package.json`
- Modify: `frontend/components/DisruptionDetail.tsx`
- Modify: `frontend/components/DisruptionDetail.test.tsx`

**Interfaces:** `DisruptionDetail`'s props (`{ disruption: Disruption }`) are unchanged — only its rendering and client/server nature change. No other file imports it in a way that depends on it being a Server Component.

- [ ] **Step 1: Add the dependency**

In `frontend/package.json`, add to `dependencies` (alphabetical, matching the existing list's order):

```json
    "isomorphic-dompurify": "^2.19.0",
```

Run: `cd frontend && npm install`
Expected: installs cleanly, `package-lock.json` updated.

- [ ] **Step 2: Write the failing tests**

Replace `frontend/components/DisruptionDetail.test.tsx` entirely with (existing tests preserved, three new ones added):

```typescript
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { DisruptionDetail } from './DisruptionDetail';
import type { Disruption } from '@/lib/types';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

const sample: Disruption = {
  category: 'RealTime',
  description: 'Signal failure at Woking',
  affectedStops: ['WOK', 'WAT'],
  affectedRoutes: [{ from: 'WAT', to: 'WOK' }],
  source: 'knowledgebase-incident-123',
};

describe('DisruptionDetail', () => {
  it('renders the description', () => {
    renderWithProvider(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('Signal failure at Woking')).toBeInTheDocument();
  });

  it('renders each affected stop', () => {
    renderWithProvider(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('WOK')).toBeInTheDocument();
    expect(screen.getByText('WAT')).toBeInTheDocument();
  });

  it('renders the affected route range', () => {
    renderWithProvider(<DisruptionDetail disruption={sample} />);
    expect(screen.getByText('WAT → WOK')).toBeInTheDocument();
  });

  it('renders nothing extra when affectedRoutes is empty', () => {
    renderWithProvider(
      <DisruptionDetail disruption={{ ...sample, affectedRoutes: [] }} />,
    );
    expect(screen.queryByText(/→/)).not.toBeInTheDocument();
  });

  it('renders safe HTML tags as actual elements, not escaped text', () => {
    const withHtml = { ...sample, description: '<p>Signal failure</p><br/><strong>at Woking</strong>' };
    renderWithProvider(<DisruptionDetail disruption={withHtml} />);
    expect(screen.getByText('Signal failure').tagName).toBe('P');
    expect(screen.getByText('at Woking').tagName).toBe('STRONG');
  });

  it('strips script tags and event handler attributes', () => {
    const malicious = { ...sample, description: '<p onclick="alert(1)">Safe text</p><script>alert(2)</script>' };
    const { container } = renderWithProvider(<DisruptionDetail disruption={malicious} />);
    expect(container.querySelector('script')).not.toBeInTheDocument();
    expect(screen.getByText('Safe text')).not.toHaveAttribute('onclick');
  });

  it('forces target=_blank and rel=noopener on links', () => {
    const withLink = { ...sample, description: '<a href="https://example.com">More info</a>' };
    renderWithProvider(<DisruptionDetail disruption={withLink} />);
    const link = screen.getByText('More info');
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', 'noopener');
  });
});
```

- [ ] **Step 3: Run tests to verify the new ones fail**

Run: `cd frontend && npx vitest run components/DisruptionDetail.test.tsx`
Expected: the 4 pre-existing tests PASS unchanged; the 3 new tests FAIL (description currently renders as escaped text, so `<p>`/`<strong>`/`<a>` never become real elements).

- [ ] **Step 4: Implement**

Replace `frontend/components/DisruptionDetail.tsx` entirely with:

```tsx
'use client';

import DOMPurify from 'isomorphic-dompurify';
import { Stack, Text, Badge, Group } from '@mantine/core';
import type { Disruption } from '@/lib/types';

// Registered once at module load. `disruption.description` comes from the
// Darwin/Knowledgebase feed already fully HTML-entity-decoded by the time
// it reaches the frontend (see poller-incidents' quick_xml parsing) — it's
// real markup, not escaped/serialized XML needing re-parsing. DOMPurify's
// ALLOWED_ATTR strips `target`/`rel` by default since they're not in the
// allowlist below; this hook adds them back on every surviving `<a>` so
// external links don't inherit this page's window/referrer.
DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank');
    node.setAttribute('rel', 'noopener');
  }
});

const ALLOWED_TAGS = ['p', 'br', 'strong', 'b', 'em', 'i', 'ul', 'ol', 'li', 'a'];
const ALLOWED_ATTR = ['href'];

function sanitizeDescription(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR });
}

export function DisruptionDetail({ disruption }: { disruption: Disruption }) {
  return (
    <Stack gap="xs">
      <div dangerouslySetInnerHTML={{ __html: sanitizeDescription(disruption.description) }} />
      {disruption.affectedStops.length > 0 && (
        <Group gap="xs">
          {disruption.affectedStops.map((crs) => (
            <Badge key={crs} variant="outline" color="gray">
              {crs}
            </Badge>
          ))}
        </Group>
      )}
      {disruption.affectedRoutes.map((route, i) => (
        <Text key={i} size="sm" c="dimmed">
          {route.from} → {route.to}
        </Text>
      ))}
    </Stack>
  );
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cd frontend && npx vitest run components/DisruptionDetail.test.tsx`
Expected: PASS — all 7 tests.

- [ ] **Step 6: Commit**

```bash
git add frontend/package.json frontend/package-lock.json frontend/components/DisruptionDetail.tsx frontend/components/DisruptionDetail.test.tsx
git commit -m "Render disruption.description as sanitized HTML instead of escaped text"
```

---

### Task 3: `RepresentativeInfo` component

**Files:**
- Create: `frontend/components/RepresentativeInfo.tsx`
- Create: `frontend/components/RepresentativeInfo.test.tsx`

**Interfaces:**
- Produces: `RepresentativeInfo({ statuses: LineStatus[] })` — a Server Component (no interactivity, no `'use client'` needed). Consumed by Task 5 and Task 6.

- [ ] **Step 1: Write the failing test**

Create `frontend/components/RepresentativeInfo.test.tsx`:

```typescript
import { describe, it, expect } from 'vitest';
import { render, screen } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { RepresentativeInfo } from './RepresentativeInfo';
import type { LineStatus } from '@/lib/types';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

function baseStatus(overrides: Partial<LineStatus> = {}): LineStatus {
  return {
    statusSeverity: 9,
    statusSeverityDescription: 'Minor Delays',
    reason: 'Signal failure',
    dataQuality: 'knowledgebase',
    validityPeriods: [],
    ...overrides,
  };
}

describe('RepresentativeInfo', () => {
  it('renders nothing when no status has sampleStats', () => {
    const { container } = renderWithProvider(<RepresentativeInfo statuses={[baseStatus()]} />);
    expect(container).toBeEmptyDOMElement();
  });

  it('renders the sample stats summary when present', () => {
    const withStats = baseStatus({
      sampleStats: { total: 160, delayed: 142, cancelled: 3, avgDelayMinutes: 12.4 },
    });
    renderWithProvider(<RepresentativeInfo statuses={[withStats]} />);
    expect(screen.getByText(/142 of 160 sampled services delayed/)).toBeInTheDocument();
    expect(screen.getByText(/avg 12\.4 min late/)).toBeInTheDocument();
  });

  it('uses the first status carrying sampleStats when multiple statuses exist', () => {
    const withoutStats = baseStatus();
    const withStats = baseStatus({
      reason: 'Different issue',
      sampleStats: { total: 20, delayed: 5, cancelled: 0, avgDelayMinutes: 4 },
    });
    renderWithProvider(<RepresentativeInfo statuses={[withoutStats, withStats]} />);
    expect(screen.getByText(/5 of 20 sampled services delayed/)).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/RepresentativeInfo.test.tsx`
Expected: FAIL — `./RepresentativeInfo` doesn't exist yet.

- [ ] **Step 3: Implement**

Create `frontend/components/RepresentativeInfo.tsx`:

```tsx
import { Card, Text } from '@mantine/core';
import type { LineStatus } from '@/lib/types';

/** Shown only when at least one status carries `sampleStats` — the
 * aggregator attaches the same sample-derived stats to every status on a
 * line's report, so the first one found is representative of all of them.
 * Omitted entirely (not zeroed out) when none do. */
export function RepresentativeInfo({ statuses }: { statuses: LineStatus[] }) {
  const withStats = statuses.find((status) => status.sampleStats);
  if (!withStats?.sampleStats) return null;

  const { total, delayed, avgDelayMinutes } = withStats.sampleStats;

  return (
    <Card withBorder padding="sm">
      <Text size="sm">
        {delayed} of {total} sampled services delayed, avg {avgDelayMinutes.toFixed(1)} min late.
      </Text>
    </Card>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run components/RepresentativeInfo.test.tsx`
Expected: PASS — all 3 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/RepresentativeInfo.tsx frontend/components/RepresentativeInfo.test.tsx
git commit -m "Add RepresentativeInfo component"
```

---

### Task 4: `IssueList` component

**Files:**
- Create: `frontend/components/IssueList.tsx`
- Create: `frontend/components/IssueList.test.tsx`

**Interfaces:**
- Consumes: `StatusBadge` (existing), `DisruptionDetail` (Task 2).
- Produces: `IssueList({ statuses: LineStatus[] })` — a Client Component. Consumed by Task 5 and Task 6.

- [ ] **Step 1: Write the failing test**

Create `frontend/components/IssueList.test.tsx`:

```typescript
import { describe, it, expect } from 'vitest';
import { render, screen, fireEvent } from '@testing-library/react';
import { MantineProvider } from '@mantine/core';
import { IssueList } from './IssueList';
import type { LineStatus } from '@/lib/types';

function renderWithProvider(ui: React.ReactElement) {
  return render(<MantineProvider>{ui}</MantineProvider>);
}

const now = new Date().toISOString();
const future = new Date(Date.now() + 86400000).toISOString();

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

const all = [minorNow, severePlanned, inferredNow];

describe('IssueList', () => {
  it('renders one row per status, collapsed by default', () => {
    renderWithProvider(<IssueList statuses={all} />);
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
  });

  it('filters by severity', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByText('Minor Delays'));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
    expect(screen.queryByText('10 of 12 sampled services delayed.')).not.toBeInTheDocument();
  });

  it('filters by source type', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByText('Planned'));
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
    expect(screen.queryByText('10 of 12 sampled services delayed.')).not.toBeInTheDocument();
  });

  it('filters to active only', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByText('Active'));
    expect(screen.getByText('Signal failure')).toBeInTheDocument();
    expect(screen.getByText('10 of 12 sampled services delayed.')).toBeInTheDocument();
    expect(screen.queryByText('Engineering works')).not.toBeInTheDocument();
  });

  it('filters to upcoming only', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByText('Upcoming'));
    expect(screen.getByText('Engineering works')).toBeInTheDocument();
    expect(screen.queryByText('Signal failure')).not.toBeInTheDocument();
  });

  it('shows a message when no issues match the filters', () => {
    renderWithProvider(<IssueList statuses={all} />);
    fireEvent.click(screen.getByText('Minor Delays'));
    fireEvent.click(screen.getByText('Planned'));
    expect(screen.getByText('No issues match the current filters.')).toBeInTheDocument();
  });

  it('expands an entry to reveal its detail on click', () => {
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
    renderWithProvider(<IssueList statuses={[withDisruption]} />);
    expect(screen.queryByText('Full details here')).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Signal failure'));
    expect(screen.getByText('Full details here')).toBeInTheDocument();
  });
});
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cd frontend && npx vitest run components/IssueList.test.tsx`
Expected: FAIL — `./IssueList` doesn't exist yet.

- [ ] **Step 3: Implement**

Create `frontend/components/IssueList.tsx`:

```tsx
'use client';

import { useState } from 'react';
import { Accordion, Badge, Chip, Group, SegmentedControl, Stack, Text } from '@mantine/core';
import { StatusBadge } from './StatusBadge';
import { DisruptionDetail } from './DisruptionDetail';
import type { LineStatus } from '@/lib/types';

type ActiveFilter = 'all' | 'active' | 'upcoming';

const DATA_QUALITY_LABELS: Record<LineStatus['dataQuality'], string> = {
  knowledgebase: 'Knowledgebase',
  'ldbws-inferred': 'LDBWS-inferred',
  'trust-inferred': 'Trust-inferred',
  planned: 'Planned',
};

function isUpcoming(status: LineStatus): boolean {
  const period = status.validityPeriods[0];
  if (!period) return false;
  return !period.isNow && new Date(period.fromDate).getTime() > Date.now();
}

function isActive(status: LineStatus): boolean {
  return status.validityPeriods.some((period) => period.isNow);
}

export function IssueList({ statuses }: { statuses: LineStatus[] }) {
  const severityOptions = Array.from(new Set(statuses.map((status) => status.statusSeverityDescription)));
  const [severityFilter, setSeverityFilter] = useState<string[]>([]);
  const [sourceFilter, setSourceFilter] = useState<string[]>([]);
  const [activeFilter, setActiveFilter] = useState<ActiveFilter>('all');

  const filtered = statuses.filter((status) => {
    if (severityFilter.length > 0 && !severityFilter.includes(status.statusSeverityDescription)) return false;
    if (sourceFilter.length > 0 && !sourceFilter.includes(status.dataQuality)) return false;
    if (activeFilter === 'active' && !isActive(status)) return false;
    if (activeFilter === 'upcoming' && !isUpcoming(status)) return false;
    return true;
  });

  return (
    <Stack gap="md">
      <Stack gap="xs">
        <Chip.Group multiple value={severityFilter} onChange={setSeverityFilter}>
          <Group gap="xs">
            {severityOptions.map((option) => (
              <Chip key={option} value={option} size="xs">
                {option}
              </Chip>
            ))}
          </Group>
        </Chip.Group>
        <Chip.Group multiple value={sourceFilter} onChange={setSourceFilter}>
          <Group gap="xs">
            {Object.entries(DATA_QUALITY_LABELS).map(([value, label]) => (
              <Chip key={value} value={value} size="xs">
                {label}
              </Chip>
            ))}
          </Group>
        </Chip.Group>
        <SegmentedControl
          value={activeFilter}
          onChange={(value) => setActiveFilter(value as ActiveFilter)}
          data={[
            { label: 'All', value: 'all' },
            { label: 'Active', value: 'active' },
            { label: 'Upcoming', value: 'upcoming' },
          ]}
        />
      </Stack>

      {filtered.length === 0 && <Text c="dimmed">No issues match the current filters.</Text>}

      <Accordion multiple>
        {filtered.map((status, i) => (
          <Accordion.Item key={i} value={String(i)}>
            <Accordion.Control>
              <Group justify="space-between" wrap="nowrap">
                <Group gap="xs" wrap="nowrap">
                  <StatusBadge severity={status.statusSeverity} />
                  <Text size="sm">{status.reason}</Text>
                </Group>
                <Badge variant="outline" size="sm">
                  {DATA_QUALITY_LABELS[status.dataQuality]}
                </Badge>
              </Group>
            </Accordion.Control>
            <Accordion.Panel>
              {status.disruption ? (
                <DisruptionDetail disruption={status.disruption} />
              ) : (
                <Text c="dimmed" size="sm">
                  No further detail available.
                </Text>
              )}
            </Accordion.Panel>
          </Accordion.Item>
        ))}
      </Accordion>
    </Stack>
  );
}
```

- [ ] **Step 4: Run test to verify it passes**

Run: `cd frontend && npx vitest run components/IssueList.test.tsx`
Expected: PASS — all 7 tests.

- [ ] **Step 5: Commit**

```bash
git add frontend/components/IssueList.tsx frontend/components/IssueList.test.tsx
git commit -m "Add IssueList component with severity/source/active filters"
```

---

### Task 5: Restructure `/lines/{id}` page

**Files:**
- Modify: `frontend/app/lines/[id]/page.tsx`

**Interfaces:**
- Consumes: `worstStatus` (Task 1), `RepresentativeInfo` (Task 3), `IssueList` (Task 4).

No automated test for this page — no `app/**/*.test.tsx` file exists anywhere in this codebase (confirmed by search); Server Component pages with data fetching aren't unit tested here. Verified manually in Step 2.

- [ ] **Step 1: Implement**

Replace `frontend/app/lines/[id]/page.tsx` entirely with:

```tsx
import { notFound } from 'next/navigation';
import { Stack, Title, Text, Group } from '@mantine/core';
import Link from 'next/link';
import { ApiNotFoundError, getLineStatus } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { worstStatus } from '@/lib/severity';

export default async function LineDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;

  let reports;
  try {
    reports = await getLineStatus([id], true);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  const report = reports[0];
  const worst = worstStatus(report);

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>{report.name}</Title>
        <StatusBadge severity={worst.statusSeverity} />
      </Group>
      <Text c="dimmed">Operators: {report.operators.join(', ')}</Text>
      {/* Plain `<Link>` wrapping `Text` rather than `component={Link}` on a
          Mantine polymorphic prop: this page is a Server Component, and
          that pattern previously broke `next build`'s Server/Client
          boundary check (see LineStatusCard fix). */}
      <Link href={`/lines/${id}/history`} style={{ textDecoration: 'none' }}>
        <Text c="blue">View history</Text>
      </Link>
      <RepresentativeInfo statuses={report.lineStatuses} />
      <IssueList statuses={report.lineStatuses} />
    </Stack>
  );
}
```

- [ ] **Step 2: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

Visit `http://localhost:3000/lines/wcml` (or any real line id). Confirm: a status badge appears next to the title reflecting the worst status; the issue list shows one collapsed row per status with working severity/source/active-upcoming filters; expanding a row with a real incident shows properly-rendered HTML (not escaped markup) in its description; the representative-info block appears only for lines currently carrying `sampleStats`.

- [ ] **Step 3: Commit**

```bash
git add frontend/app/lines/\[id\]/page.tsx
git commit -m "Restructure line detail page around status header, representative info, and issue list"
```

---

### Task 6: Restructure `/stations/{crs}` page

**Files:**
- Modify: `frontend/app/stations/[crs]/page.tsx`

**Interfaces:**
- Consumes: `worstStatus` (Task 1), `RepresentativeInfo` (Task 3), `IssueList` (Task 4), plus `PinToggle`/`getPreferences` (already added by the frontend-personalization plan's Task 11 — this step's full-file replacement includes that prior work, since it replaces the whole file again).

No automated test — same rationale as Task 5.

- [ ] **Step 1: Implement**

Replace `frontend/app/stations/[crs]/page.tsx` entirely with:

```tsx
import { Stack, Title, Text, Group, Divider } from '@mantine/core';
import { getStopPointDisruption, getPreferences } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { PinToggle } from '@/components/PinToggle';
import { worstStatus } from '@/lib/severity';

export default async function StationDisruptionPage({
  params,
}: {
  params: Promise<{ crs: string }>;
}) {
  const { crs } = await params;
  const [reports, preferences] = await Promise.all([getStopPointDisruption(crs), getPreferences()]);

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>Disruptions at {crs}</Title>
        <PinToggle kind="station" id={crs} initiallyPinned={preferences.pinnedStations.includes(crs)} />
      </Group>
      {reports.length === 0 && <Text c="dimmed">No disruptions affecting this station.</Text>}
      {reports.map((report) => {
        const worst = worstStatus(report);
        return (
          <Stack key={report.id} gap="sm">
            <Divider my="sm" />
            <Group justify="space-between">
              <Text fw={600}>{report.name}</Text>
              <StatusBadge severity={worst.statusSeverity} />
            </Group>
            <RepresentativeInfo statuses={report.lineStatuses} />
            <IssueList statuses={report.lineStatuses} />
          </Stack>
        );
      })}
    </Stack>
  );
}
```

- [ ] **Step 2: Verify against the running stack**

```bash
docker compose build --no-cache frontend
docker compose up -d --no-build frontend
```

Visit `http://localhost:3000/stations/WOK` (or any station on a real line). Confirm: each line touching this station gets its own status badge, representative-info block (when applicable), and filterable issue list; the pin toggle from the earlier plan still works.

- [ ] **Step 3: Run the frontend test suite**

Run: `cd frontend && npm test`
Expected: all tests pass (existing suite plus this plan's new/modified test files).

- [ ] **Step 4: Commit**

```bash
git add frontend/app/stations/\[crs\]/page.tsx
git commit -m "Restructure station disruption page around status header, representative info, and issue list"
```
