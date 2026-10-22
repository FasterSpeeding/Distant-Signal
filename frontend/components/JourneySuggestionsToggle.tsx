'use client';

import { useEffect, useState } from 'react';
import { Stack, Switch, Text } from '@mantine/core';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import type { NotificationPreferences } from '@/lib/types';

/** "Suggest other trains when a journey breaks" (mid-journey changes spec
 * 2026-10-22 §6.5): the account's `journeySuggestions` preference, read on
 * mount and saved on change. A failed save puts the switch back and says
 * so. Disabled until the saved value is known, so it never shows a guess. */
export function JourneySuggestionsToggle() {
  const [value, setValue] = useState<boolean | null>(null);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    // A function, not the flag itself: TypeScript would keep the flag
    // narrowed to `false` across the awaits below.
    const isCancelled = () => cancelled;
    void (async () => {
      try {
        const response = await fetch('/api/preferences/notifications');
        if (isCancelled()) return;
        if (!response.ok) {
          setError(describeFailure('load', 'this setting', response.status));
          return;
        }
        const body = (await response.json()) as NotificationPreferences;
        if (!isCancelled()) setValue(body.journeySuggestions);
      } catch {
        if (!isCancelled()) setError(describeFailure('load', 'this setting'));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  async function change(next: boolean) {
    const previous = value;
    setValue(next);
    setSaving(true);
    setError(null);
    try {
      const response = await fetch('/api/preferences/notifications', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ journeySuggestions: next } satisfies NotificationPreferences),
      });
      if (!response.ok) {
        setValue(previous);
        setError(await failureFromResponse('save', 'this setting', response));
      }
    } catch {
      setValue(previous);
      setError(describeFailure('save', 'this setting'));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Stack gap={4}>
      <Switch
        label="Suggest other trains when a journey breaks"
        description="A notification with other ways to get there when a train is cancelled or a connection is missed."
        checked={value ?? false}
        disabled={value === null || saving}
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
