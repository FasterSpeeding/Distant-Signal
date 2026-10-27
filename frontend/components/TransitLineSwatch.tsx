import { ColorSwatch } from '@mantine/core';
import { lineColour } from '@/lib/transitStops';
import type { TransitLine } from '@/lib/types';
import classes from './TransitLineSwatch.module.css';

/** A transit line's colour, as the network page's line cards
 * (`TransitLineList`) and a planned tram leg show it, or a neutral grey
 * when the line has none. Decorative: the line's name always sits beside
 * it. `inline` sets it in running text, before the name. */
export function TransitLineSwatch({
  line,
  size = 14,
  inline = false,
}: {
  line: Pick<TransitLine, 'colour'> | null | undefined;
  size?: number;
  inline?: boolean;
}) {
  return (
    <ColorSwatch
      component="span"
      color={lineColour(line)}
      size={size}
      withShadow={false}
      aria-hidden="true"
      className={inline ? classes.inline : undefined}
      data-line-swatch
    />
  );
}
