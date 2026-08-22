'use client';

import { useState } from 'react';
import { Text, Tooltip } from '@mantine/core';
import { useInterval, useMounted } from '@mantine/hooks';
import { relativeTime } from '@/lib/relativeTime';
import { formatDateTime } from '@/lib/dateFormat';

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
 * timezone); only once `useMounted()` flips true does it switch to the live
 * relative string. `useInterval` just forces a re-render every 30s so that
 * relative string stays fresh — the actual "now" is recomputed at render
 * time, not cached in state. */
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
  const exact = formatDateTime(date);
  const mounted = useMounted();
  const [, forceTick] = useState(0);
  useInterval(() => forceTick((tick) => tick + 1), RELATIVE_TIME_TICK_MS, { autoInvoke: true });

  const displayed = mounted ? relativeTime(date, new Date()) : exact;
  const text = (
    <Text size="xs" c="dimmed">
      {label} {displayed}
    </Text>
  );

  return withTooltip ? (
    <Tooltip label={exact} events={{ hover: true, focus: true, touch: true }}>
      {text}
    </Tooltip>
  ) : (
    text
  );
}
