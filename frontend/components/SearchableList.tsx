'use client';

import { Fragment, useState, type ReactNode } from 'react';
import { SimpleGrid, Stack, Text } from '@mantine/core';
import { countLabel, ListFilterBar, type ListNoun } from './ListFilterBar';
import { ListResultSummary } from './ListResultSummary';
import { replaceUrlParams, useUrlFilterSync } from '@/lib/useUrlFilterSync';
import { useFilteredDocumentTitle } from '@/lib/useFilteredDocumentTitle';

/** One row of a `SearchableList`: `node` is rendered by the page (it may be
 * a Server Component's output), `text` is what the search matches. */
export interface SearchableListItem {
  key: string;
  text: string;
  node: ReactNode;
}

/** The URL param a search-only list keeps its search in. */
export const SEARCHABLE_LIST_QUERY_PARAM = 'q';

/** Every item whose `text` contains each word of `query`, in any order,
 * ignoring case: "south rail" finds "South Western Railway". */
export function matchingItems<T extends { text: string }>(items: readonly T[], query: string): T[] {
  const words = query.trim().toLowerCase().split(/\s+/).filter(Boolean);
  if (words.length === 0) return [...items];
  return items.filter((item) => {
    const text = item.text.toLowerCase();
    return words.every((word) => text.includes(word));
  });
}

/** A list with the search-only `ListFilterBar` (docs/style-guide.md, "List
 * search and filters"): "Find an operator" above /operators' cards, "Find
 * a stop" above a tram network's stops. Every item is already on the page,
 * so the search filters in place; it is kept in the URL as `?q=` and named
 * in the tab title.
 *
 * `layout` picks the container, here rather than passed in, because a
 * Server Component page can hand this its rendered items but not a render
 * function: `grid` is the cards' 1/2/3-column grid, `list` a plain `<ul>`. */
export function SearchableList({
  items,
  searchLabel,
  noun,
  titlePrefix,
  initialQuery = '',
  layout,
}: {
  items: SearchableListItem[];
  /** "Find an operator". */
  searchLabel: string;
  noun: ListNoun;
  /** The tab title while searching: `${titlePrefix}: "sw"`. */
  titlePrefix: string;
  initialQuery?: string | undefined;
  layout: 'grid' | 'list';
}) {
  const [query, setQuery] = useState(initialQuery);
  const shown = matchingItems(items, query);
  const searching = query.trim() !== '';

  useUrlFilterSync(query.trim(), (current, state) =>
    replaceUrlParams(current, [SEARCHABLE_LIST_QUERY_PARAM], { [SEARCHABLE_LIST_QUERY_PARAM]: state }),
  );
  useFilteredDocumentTitle(searching ? `${titlePrefix}: "${query.trim()}"` : null);

  const summary =
    shown.length === 0
      ? `No ${noun.other} match.`
      : `${countLabel(shown.length, noun)} ${shown.length === 1 ? 'matches' : 'match'}.`;
  const nodes = shown.map((item) =>
    layout === 'list' ? <li key={item.key}>{item.node}</li> : <Fragment key={item.key}>{item.node}</Fragment>,
  );

  return (
    <Stack gap="md">
      <ListFilterBar
        searchLabel={searchLabel}
        query={query}
        onQueryChange={setQuery}
        noun={noun}
        matchCount={shown.length}
      />
      <ListResultSummary
        visible={searching}
        summary={summary}
        resetLabel={`Show all ${noun.other}`}
        onReset={() => setQuery('')}
      />
      {shown.length === 0 ? (
        <Text c="dimmed" size="sm">
          Try a shorter search.
        </Text>
      ) : layout === 'grid' ? (
        <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
          {nodes}
        </SimpleGrid>
      ) : (
        <Stack component="ul" gap={4} className="ds-plain-list">
          {nodes}
        </Stack>
      )}
    </Stack>
  );
}
