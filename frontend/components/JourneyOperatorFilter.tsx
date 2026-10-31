'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Group, Stack, Text } from '@mantine/core';
import { OperatorFilterField } from './PlanTripAdvancedOptions';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import {
  journeyOperatorFilterLine,
  operatorName,
  type OperatorMode,
  type SavedOperatorFilter,
} from '@/lib/plannerOperators';
import type { JourneyOperatorFilter as Filter, PlannerOperatorCatalogue } from '@/lib/types';
import { trackFeature } from '@/lib/usageAnalytics';

/** The `/plan` operator filter a journey keeps for its replans (operator
 * filtering spec 2026-10-31 §6.9), on the owner's journey page: "Planned
 * using only Avanti West Coast", with Change (the planner's own operator
 * field) and Remove (`PUT /Journeys/{id}/operator-filter`). A change
 * re-plans nothing until the next replan. */
export function JourneyOperatorFilter({ journeyId, filter }: { journeyId: number; filter: Filter }) {
  const router = useRouter();
  const [catalogue, setCatalogue] = useState<PlannerOperatorCatalogue | null>(null);
  const [editing, setEditing] = useState(false);
  const [mode, setMode] = useState<OperatorMode | undefined>(filter.mode);
  const [operators, setOperators] = useState<string[]>(filter.entries);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  // The names (and Change's choices): the planner's catalogue. Without it
  // the line names operators by code.
  useEffect(() => {
    let cancelled = false;
    fetch('/api/Trips/operators')
      .then(async (response) => (response.ok ? ((await response.json()) as PlannerOperatorCatalogue) : null))
      .then((loaded) => {
        if (!cancelled) setCatalogue(loaded);
      })
      .catch(() => undefined);
    return () => {
      cancelled = true;
    };
  }, []);

  const nameOf = (entry: string) => operatorName(entry, catalogue);

  async function save(next: SavedOperatorFilter | null) {
    setSaving(true);
    setError(null);
    try {
      const response = await fetch(`/api/Journeys/${String(journeyId)}/operator-filter`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ operatorFilter: next }),
      });
      if (!response.ok) {
        setError(await failureFromResponse('save', 'the operator filter', response));
        return;
      }
      trackFeature('plan_filters', next ? 'journey_operator_filter_changed' : 'journey_operator_filter_removed');
      setEditing(false);
      router.refresh();
    } catch {
      setError(describeFailure('save', 'the operator filter'));
    } finally {
      setSaving(false);
    }
  }

  function startEditing() {
    setMode(filter.mode);
    setOperators(filter.entries);
    setError(null);
    setEditing(true);
  }

  return (
    <Stack gap={6} data-journey-operator-filter>
      <Group gap="sm">
        <Text size="sm" c="dimmed">
          {journeyOperatorFilterLine(filter, nameOf)}. Suggestions after a disruption follow it.
        </Text>
        {!editing && (
          <Group gap={6}>
            <Button size="xs" variant="default" onClick={startEditing} disabled={saving}>
              Change
            </Button>
            <Button size="xs" variant="default" onClick={() => void save(null)} disabled={saving}>
              Remove
            </Button>
          </Group>
        )}
      </Group>
      {editing && (
        <Stack gap="xs">
          <OperatorFilterField
            mode={mode}
            operators={operators}
            onChange={(nextMode, nextOperators) => {
              setMode(nextMode);
              setOperators(nextOperators);
            }}
            catalogue={catalogue}
            operatorName={nameOf}
          />
          <Group gap={6}>
            <Button
              size="xs"
              onClick={() => void save(mode && operators.length > 0 ? { mode, entries: operators } : null)}
              loading={saving}
            >
              Save
            </Button>
            <Button size="xs" variant="default" onClick={() => setEditing(false)} disabled={saving}>
              Cancel
            </Button>
          </Group>
        </Stack>
      )}
      {error && (
        <Text size="sm" c="var(--ds-color-error-text)" role="alert">
          {error}
        </Text>
      )}
    </Stack>
  );
}
