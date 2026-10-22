'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Box,
  Button,
  Card,
  Group,
  Stack,
  Text,
  Title,
} from '@mantine/core';
import { ButtonSpinner, LoadingText } from './LoadingText';
import { StateBadge } from './StateBadge';
import { formatDateTime } from '@/lib/dateFormat';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import { BOOKED_BADGE, fateBadge, revisionLegLine, revisionTitle } from '@/lib/journeyBreak';
import type { Revision, RevisionsResponse } from '@/lib/types';

const DISCLOSURE_STYLES = {
  item: { border: 'none', backgroundColor: 'transparent' },
  control: { padding: 0, paddingBlock: 'var(--mantine-spacing-xs)' },
  chevron: { marginInlineEnd: 'var(--mantine-spacing-xs)' },
  panel: { paddingInlineStart: 0 },
  content: { paddingInline: 0 },
} as const;

/** "Earlier plans (2)" at the end of the journey page, for the owner (spec
 * §8.2): collapsed, and read from `GET .../revisions` only when opened.
 * Each plan before the current one lists its legs with what became of
 * them ("Cancelled", "Missed", "Not used"), the booked plan badged
 * "Booked", and "Go back to this plan" while its trains still work. */
export function EarlierPlans({ journeyId, revisionCount }: { journeyId: number; revisionCount: number }) {
  const [revisions, setRevisions] = useState<Revision[] | null>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const earlier = Math.max(revisionCount - 1, 0);
  if (earlier === 0) return null;

  async function load() {
    setLoading(true);
    setError(null);
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/revisions`);
      if (!response.ok) {
        setError(describeFailure('load', 'earlier plans', response.status));
        return;
      }
      const body = (await response.json()) as RevisionsResponse;
      setRevisions(body.revisions);
    } catch {
      setError(describeFailure('load', 'earlier plans'));
    } finally {
      setLoading(false);
    }
  }

  return (
    <Accordion
      chevronPosition="left"
      keepMounted={false}
      styles={DISCLOSURE_STYLES}
      order={2}
      onChange={(value) => {
        if (value !== null && revisions === null && !loading) void load();
      }}
    >
      <AccordionItem value="earlier-plans">
        <AccordionControl>Earlier plans ({earlier})</AccordionControl>
        <AccordionPanel>
          {loading && (
            <Box role="status" aria-busy="true">
              <LoadingText>Loading earlier plans</LoadingText>
            </Box>
          )}
          {error && (
            <Text c="var(--ds-color-error-text)" role="alert">
              {error}
            </Text>
          )}
          {revisions !== null && (
            <Stack gap="sm">
              {/* Newest first: the first is the plan the page shows. */}
              {revisions.slice(1).map((revision) => (
                <EarlierPlan key={revision.revisionNo} journeyId={journeyId} revision={revision} />
              ))}
            </Stack>
          )}
        </AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}

/** One earlier plan: its heading and when it was made, its legs with
 * their fates, and "Go back to this plan" when it can be. */
function EarlierPlan({ journeyId, revision }: { journeyId: number; revision: Revision }) {
  const router = useRouter();
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function revert() {
    setBusy(true);
    setError(null);
    try {
      const response = await fetch(`/api/Journeys/${journeyId}/revisions/${revision.revisionNo}/revert`, {
        method: 'POST',
      });
      if (!response.ok) {
        setError(await failureFromResponse('go back to', 'this plan', response));
        setBusy(false);
        return;
      }
      // Stays busy: the refresh replaces this section.
      router.refresh();
    } catch {
      setError(describeFailure('go back to', 'this plan'));
      setBusy(false);
    }
  }

  return (
    <Card withBorder padding="sm" data-revision={revision.revisionNo}>
      <Stack gap="xs">
        <Group gap="xs" wrap="wrap">
          <Title order={3} size="sm">
            {revisionTitle(revision)}
          </Title>
          {revision.reason === 'booked' && <StateBadge tone={BOOKED_BADGE.tone}>{BOOKED_BADGE.label}</StateBadge>}
          <Text size="xs" c="dimmed">
            {formatDateTime(revision.createdAt)}
          </Text>
        </Group>
        <Stack gap={4} component="ul" className="ds-plain-list">
          {revision.legs.map((leg) => {
            const fate = fateBadge(leg.fate);
            return (
              <Group key={leg.position} component="li" gap="xs" wrap="wrap">
                <Text size="sm">{revisionLegLine(leg)}</Text>
                {fate && <StateBadge tone={fate.tone}>{fate.label}</StateBadge>}
              </Group>
            );
          })}
        </Stack>
        {revision.canRevert && (
          <Group>
            <Button
              size="sm"
              variant="default"
              onClick={() => void revert()}
              disabled={busy}
              {...(busy && { leftSection: <ButtonSpinner /> })}
            >
              {busy ? 'Going back' : 'Go back to this plan'}
            </Button>
          </Group>
        )}
        {error && (
          <Text c="var(--ds-color-error-text)" role="alert">
            {error}
          </Text>
        )}
      </Stack>
    </Card>
  );
}
