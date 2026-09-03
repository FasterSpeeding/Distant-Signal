'use client';

import { useMounted } from '@mantine/hooks';
import { formatDateTime, formatLocalDateTime } from '@/lib/dateFormat';

/** Renders one instant as bare text in the **viewer's own** timezone --
 * "20 Aug 2026, 03:56" for a reader in Tokyo where a reader in London sees
 * "19 Aug 2026, 19:56". No element and no Mantine wrapper: the call site
 * owns the styling (see `TicketSummary.tsx`'s `<Text size="xs" c="dimmed">`),
 * so this returns a fragment and nothing here can drift from it.
 *
 * The server has no idea what zone the viewer is in -- the Node process
 * rendering the page is the container's UTC -- so formatting locally in the
 * render path would emit different server and client markup for the same
 * timestamp, the exact bug class `lib/dateFormat.ts`'s header comment
 * describes as having already been hit once. `useMounted()` is the gate:
 * before mount this renders the `Europe/London` string, which is
 * deterministic on both sides regardless of which process produces it, so
 * React's first client render matches the server output byte for byte; only
 * afterwards does the host-zone formatting take over. Same shape as
 * `LastUpdated` (which likewise falls back to `formatDateTime` pre-mount)
 * and `ThemeToggle`, and the same trade-off both already accept: a
 * non-UK viewer sees a brief flash from London time to their own. For the
 * designed audience -- a UK viewer of a UK rail app -- the two strings are
 * identical and there is no visible flash at all.
 *
 * **Viewer-relative timestamps only.** This is for values about the
 * viewer's own relationship to the app; anything stating a fact about the
 * rail network's clock keeps calling `formatDateTime` directly and stays
 * London. See
 * docs/superpowers/specs/2026-09-02-client-local-timezone-display-research.md's
 * Finding 1 for the call-site categorisation. */
export function LocalDateTime({ value }: { value: string }) {
  const mounted = useMounted();
  return <>{mounted ? formatLocalDateTime(value) : formatDateTime(value)}</>;
}
