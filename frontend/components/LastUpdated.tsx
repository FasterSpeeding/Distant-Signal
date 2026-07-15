'use client';

import { useEffect, useState } from 'react';
import { Text, Tooltip } from '@mantine/core';
import { relativeTime } from '@/lib/relativeTime';

const EXACT_TIME_FORMAT = new Intl.DateTimeFormat('en-GB', {
  timeZone: 'Europe/London',
  dateStyle: 'medium',
  timeStyle: 'short',
});

const RELATIVE_TIME_TICK_MS = 30_000;

/** Shows "{label} Xm ago", with the exact time in a tooltip (or plain,
 * with `withTooltip={false}`, for reuse inside another tooltip's content —
 * see `DataFreshnessInfo`, which nests three of these inside one outer
 * `Tooltip` rather than each showing its own).
 *
 * A relative "time ago" string depends on `Date.now()` at render time, so
 * it can't be computed identically during SSR and the client's
 * pre-hydration render — the same class of bug fixed in `ThemeToggle` (see
 * that component's comment). Before mount, this always shows a fixed
 * absolute time (deterministic regardless of server/client locale or
 * timezone); only after the `useEffect` below fires does it switch to the
 * live relative string, re-computed every 30s. */
export function LastUpdated({
  timestamp,
  label = 'Updated',
  withTooltip = true,
}: {
  timestamp: string;
  label?: string;
  withTooltip?: boolean;
}) {
  const date = new Date(timestamp);
  const exact = EXACT_TIME_FORMAT.format(date);
  const [now, setNow] = useState<Date | null>(null);

  useEffect(() => {
    setNow(new Date());
    const id = setInterval(() => setNow(new Date()), RELATIVE_TIME_TICK_MS);
    return () => clearInterval(id);
  }, []);

  const displayed = now === null ? exact : relativeTime(date, now);
  const text = (
    <Text size="xs" c="dimmed">
      {label} {displayed}
    </Text>
  );

  return withTooltip ? <Tooltip label={exact}>{text}</Tooltip> : text;
}
