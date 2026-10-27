'use client';

import { useState } from 'react';
import { Stack, Switch, Text } from '@mantine/core';
import { describeFailure, failureFromResponse } from '@/lib/failure';

/** "Notify me about this train": the owner's per-train push switch
 * (`PATCH /Train/{trackingId}` `{notificationsEnabled}`), on the train page
 * and on a journey's train leg. Off, the train sends no delay, cancellation,
 * skipped-stop or late-start notifications; the page still shows all of it.
 * A failed save puts the switch back and says so. */
export function TrainNotificationsSwitch({
  trackingId,
  initiallyEnabled,
}: {
  trackingId: number;
  initiallyEnabled: boolean;
}) {
  const [enabled, setEnabled] = useState(initiallyEnabled);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function change(next: boolean) {
    setEnabled(next);
    setSaving(true);
    setError(null);
    try {
      const response = await fetch(`/api/Train/${String(trackingId)}`, {
        method: 'PATCH',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ notificationsEnabled: next }),
      });
      if (!response.ok) {
        setEnabled(!next);
        setError(await failureFromResponse('save', 'this setting', response));
      }
    } catch {
      setEnabled(!next);
      setError(describeFailure('save', 'this setting'));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Stack gap={4}>
      <Switch
        label="Notify me about this train"
        description="Delays, cancellations and skipped stops, on every device with notifications on."
        checked={enabled}
        disabled={saving}
        onChange={(event) => void change(event.currentTarget.checked)}
      />
      {error && (
        <Text size="sm" c="var(--ds-color-error-text)" role="alert">
          {error}
        </Text>
      )}
    </Stack>
  );
}
