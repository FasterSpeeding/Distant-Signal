import { Card, Text } from '@mantine/core';
import { CardLink } from './CardLink';
import { tokens } from '@/lib/theme';

/** A count over its label: the /status strip of five severity groups
 * (round 2, item 3).
 *
 * Five tiles never fill a two-column phone grid, so below `sm` each tile
 * is a compact row (count, label, then "View lines" at the far end) in a
 * one-column list, and from `sm` up it is the familiar column of count
 * over label, five across. `.statusTile*` in `globals.css` switches the
 * orientation; the card itself carries no padding so the body can change
 * it with the layout.
 *
 * With `href` (a non-zero count) the tile is one link, raised like every
 * clickable card, and `ariaLabel` names where it goes. A zero tile is flat
 * and dimmed: there is nothing to view. */
export function StatusTile({
  count,
  label,
  href,
  ariaLabel,
}: {
  count: number;
  label: string;
  href?: string;
  ariaLabel?: string;
}) {
  const muted = href === undefined;
  const body = (
    <div className="statusTile" data-status-tile>
      <Text
        size={tokens.statusTile.valueSize}
        fw={tokens.statusTile.valueWeight}
        className="statusTile__count"
        {...(muted && { c: 'dimmed' })}
      >
        {count}
      </Text>
      <Text size={tokens.statusTile.labelSize} c="dimmed" className="statusTile__label">
        {label}
      </Text>
      {!muted && (
        // The link's own name already says where it goes; this is the
        // sighted cue, in link colour.
        <Text size="xs" c="var(--mantine-color-anchor)" className="statusTile__cue" aria-hidden>
          View lines
        </Text>
      )}
    </div>
  );
  if (muted) {
    return (
      <Card {...tokens.card.static} padding={0} h="100%">
        {body}
      </Card>
    );
  }
  return (
    <CardLink href={href} padding={0} ariaLabel={ariaLabel} fill>
      {body}
    </CardLink>
  );
}
