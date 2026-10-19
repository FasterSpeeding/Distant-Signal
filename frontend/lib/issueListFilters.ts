import type { LineStatus } from '@/lib/types';
import type { IssueBucket } from '@/lib/validity';

/** The issue list's tab: every issue, or one validity bucket. */
export type IssueTab = 'all' | IssueBucket;

/** An IssueList's filters as its page's URL carries them, under the list's
 * own prefix (`issueTab`, `issueSeverity`, `issueSource`) so they can't
 * clash with the page's other params (the line page's `dir`, `at`, `from`,
 * `to` and `view`), or with a second list on the same page (the line
 * page's TfL list is `tflIssue`). */
export interface IssueListFilters {
  tab: IssueTab | null;
  /** `statusSeverity` numbers, as strings. */
  severities: string[];
  sources: string[];
}

/** The URL keys for a list with `prefix`. */
export function issueListParamKeys(prefix: string) {
  return { tab: `${prefix}Tab`, severity: `${prefix}Severity`, source: `${prefix}Source` } as const;
}

const SOURCES: readonly LineStatus['dataQuality'][] = [
  'knowledgebase',
  'ldbws-inferred',
  'trust-inferred',
  'planned',
  'tfl',
  'transit',
];

/** Reads a list's filters from the page's query params. Anything malformed
 * is dropped; a well-formed severity the report doesn't have matches
 * nothing until cleared, which the list's empty state explains. */
export function parseIssueListFilters(params: URLSearchParams, prefix: string): IssueListFilters {
  const keys = issueListParamKeys(prefix);
  const tab = params.get(keys.tab);
  return {
    tab: tab === 'all' || tab === 'active' || tab === 'upcoming' || tab === 'ended' ? tab : null,
    severities: params.getAll(keys.severity).filter((entry) => /^\d+$/.test(entry)),
    sources: params.getAll(keys.source).filter((entry) => (SOURCES as readonly string[]).includes(entry)),
  };
}

/** The line page's issue lists, by URL prefix: the main list and TfL's. */
export const LINE_PAGE_ISSUE_LISTS = ['issue', 'tflIssue'] as const;

/** Every URL key the lists under `prefixes` use. */
function issueListKeys(prefixes: readonly string[]): string[] {
  return prefixes.flatMap((prefix) => Object.values(issueListParamKeys(prefix)));
}

/** The issue list params in `current` (the page's live query params), as
 * `[key, value]` pairs in their URL order. */
export function issueListEntries(
  current: URLSearchParams,
  prefixes: readonly string[] = LINE_PAGE_ISSUE_LISTS,
): [string, string][] {
  const keys = issueListKeys(prefixes);
  return [...current.entries()].filter(([key]) => keys.includes(key));
}

/** `href` with its issue list params replaced by those in `current`,
 * keeping its other params and its hash. A link built on the server only
 * knows the filters the page was opened with; this carries the ones the
 * visitor has set since. */
export function withIssueListParams(
  href: string,
  current: URLSearchParams,
  prefixes: readonly string[] = LINE_PAGE_ISSUE_LISTS,
): string {
  const url = new URL(href, 'http://localhost');
  for (const key of issueListKeys(prefixes)) url.searchParams.delete(key);
  for (const [key, value] of issueListEntries(current, prefixes)) url.searchParams.append(key, value);
  const search = url.searchParams.toString();
  return `${url.pathname}${search ? `?${search}` : ''}${url.hash}`;
}
