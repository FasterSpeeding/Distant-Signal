'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Button, Group, Stack, Text } from '@mantine/core';
import { describeFailure, failureFromResponse } from '@/lib/failure';

/** The way back from "Stop suggestions for this journey"
 * (`AlternativesSheet`): shown on the owner's journey page while the
 * journey's suggestion notifications are muted, with one button to turn
 * them back on (`PUT /Journeys/{id}/suggestions-muted` `{muted: false}`).
 * While muted, a broken journey's trains send their plain delay and
 * cancellation notifications instead. */
export function JourneySuggestionsUnmute({ journeyId }: { journeyId: number }) {
  const router = useRouter();
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function unmute() {
    setSaving(true);
    setError(null);
    try {
      const response = await fetch(`/api/Journeys/${String(journeyId)}/suggestions-muted`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ muted: false }),
      });
      if (!response.ok) {
        setError(await failureFromResponse('turn on', 'suggestions', response));
        return;
      }
      router.refresh();
    } catch {
      setError(describeFailure('turn on', 'suggestions'));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Stack gap={4}>
      <Group gap="sm">
        <Text size="sm" c="dimmed">
          Suggestion notifications are off for this journey.
        </Text>
        <Button size="xs" variant="default" onClick={() => void unmute()} disabled={saving}>
          Turn suggestions back on
        </Button>
      </Group>
      {error && (
        <Text size="sm" c="var(--ds-color-error-text)" role="alert">
          {error}
        </Text>
      )}
    </Stack>
  );
}
