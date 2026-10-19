'use client';

import Link from 'next/link';
import { useSearchParams } from 'next/navigation';
import type { ComponentProps } from 'react';
import { issueListEntries, withIssueListParams } from '@/lib/issueListFilters';
import { TextLink } from './TextLink';

/** The address bar's query params, live, or `null` outside the app router
 * (unit tests). Next.js keeps `useSearchParams` in step with the issue
 * lists' `history.replaceState` writes (`useUrlFilterSync`). */
function useLiveSearchParams(): URLSearchParams | null {
  // Typed non-null, but `null` outside the app router's context.
  const params = useSearchParams() as ReturnType<typeof useSearchParams> | null;
  return params ? new URLSearchParams(params.toString()) : null;
}

/** `href` carrying the page's current issue list filters. */
function useKeptHref(href: string): string {
  const params = useLiveSearchParams();
  return params ? withIssueListParams(href, params) : href;
}

/** A `next/link` back to the line page that keeps the issue lists' filters
 * (`issueTab`, `tflIssueSeverity`, ...). The links on "Trains on this line"
 * are built on the server, which only knows the filters the page was
 * opened with. The server render already has those, so it works without
 * JavaScript. */
export function IssueFiltersLink({ href, ...props }: ComponentProps<typeof Link> & { href: string }) {
  return <Link {...props} href={useKeptHref(href)} />;
}

/** `TextLink`, keeping the issue lists' filters (see `IssueFiltersLink`). */
export function IssueFiltersTextLink({ href, ...props }: ComponentProps<typeof TextLink>) {
  return <TextLink {...props} href={useKeptHref(href)} />;
}

/** Hidden inputs for a GET form on the line page, so submitting it keeps
 * the issue lists' filters (see `IssueFiltersLink`). */
export function IssueFiltersHiddenInputs() {
  const params = useLiveSearchParams();
  if (!params) return null;
  return (
    <>
      {issueListEntries(params).map(([key, value], index) => (
        <input key={`${key}-${index}`} type="hidden" name={key} value={value} />
      ))}
    </>
  );
}
