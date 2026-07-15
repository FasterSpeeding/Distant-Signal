/** Renders the gap between two instants as "just now" / "Nm ago" / "Nh
 * ago" / "Nd ago". A negative gap (clock skew — `from` in the future
 * relative to `to`) is clamped to zero rather than shown as e.g. "-2m
 * ago", matching the same defensive clamp used for poller poll-interval
 * math (see `crates/common/src/ingest.rs`'s `duration_until_next_poll`). */
export function relativeTime(from: Date, to: Date): string {
  const diffMinutes = Math.max(0, Math.floor((to.getTime() - from.getTime()) / 60_000));

  if (diffMinutes < 1) return 'just now';
  if (diffMinutes < 60) return `${diffMinutes}m ago`;

  const diffHours = Math.floor(diffMinutes / 60);
  if (diffHours < 24) return `${diffHours}h ago`;

  const diffDays = Math.floor(diffHours / 24);
  return `${diffDays}d ago`;
}
