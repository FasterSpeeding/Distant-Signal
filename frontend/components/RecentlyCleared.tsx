import type { ReactNode } from 'react';
import { Stack, Text } from '@mantine/core';
import { SectionTitle } from './SectionTitle';
import { TextLink } from './TextLink';
import { getRecentlyCleared, type RecentlyClearedScope } from '@/lib/api';
import { formatTime } from '@/lib/dateFormat';
import { describeFailure } from '@/lib/failure';
import { clearedTitle, wasExpectedUntil } from '@/lib/recentlyCleared';
import { sanitizeDescription } from '@/lib/sanitizeHtml';
import type { RecentlyClearedIncident } from '@/lib/types';

const HEADING_ID = 'recently-cleared';

/** The section's frame: heading and one line saying what it is. */
function Section({ children }: { children: ReactNode }) {
  return (
    <Stack gap="sm" component="section" aria-labelledby={HEADING_ID} data-recently-cleared>
      <SectionTitle id={HEADING_ID}>Recently cleared</SectionTitle>
      <Text size="sm" c="dimmed">
        Cleared today and still listed by National Rail. For information only. Times in UK local time.
      </Text>
      {children}
    </Stack>
  );
}

/** Incidents National Rail has cleared but still lists (2026-10-12): the
 * title, when it cleared, the stated end it beat if any, and the clearing
 * text as published. Info only, never part of a status. Hidden when there
 * are none. `showLines` names the lines each one affected (the station and
 * status pages; a line page's items are all that line's). */
export function RecentlyClearedList({
  items,
  now,
  showLines = false,
}: {
  items: RecentlyClearedIncident[];
  now: number;
  showLines?: boolean;
}) {
  if (items.length === 0) return null;
  return (
    <Section>
      <Stack gap="md" component="ul" className="ds-plain-list">
        {items.map((item) => {
          const expected = wasExpectedUntil(item, now);
          return (
            <Stack gap={4} component="li" key={item.incidentId}>
              <TextLink href={`/incidents/${encodeURIComponent(item.incidentId)}`}>
                {clearedTitle(item.summary)}
              </TextLink>
              <Text size="xs" c="dimmed">
                Cleared {formatTime(item.clearedAt)}
                {expected !== null && ` · ${expected}`}
              </Text>
              {showLines && item.affectedLines.length > 0 && (
                <Text size="xs" c="dimmed">
                  Lines: {item.affectedLines.map((line) => line.name).join(', ')}
                </Text>
              )}
              <Text
                size="sm"
                component="div"
                data-rich-text
                dangerouslySetInnerHTML={{ __html: sanitizeDescription(item.description) }}
              />
            </Stack>
          );
        })}
      </Stack>
    </Section>
  );
}

/** Fetches and renders `RecentlyClearedList`. A failed fetch says so in the
 * section rather than hiding it, since "none" would be a claim we can't
 * make; it never takes the rest of the page down. Render inside its own
 * `Suspense`. */
export async function RecentlyCleared({
  scope,
  now,
  showLines = false,
}: {
  scope: RecentlyClearedScope;
  now: number;
  showLines?: boolean;
}) {
  let items: RecentlyClearedIncident[];
  try {
    items = await getRecentlyCleared(scope);
  } catch {
    return (
      <Section>
        <Text c="dimmed">{describeFailure('load', 'recently cleared incidents')}</Text>
      </Section>
    );
  }
  return <RecentlyClearedList items={items} now={now} showLines={showLines} />;
}
