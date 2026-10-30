'use client';

import { useId, useState, type ReactNode } from 'react';
import { AccordionChevron, Button, Group, TableTbody, TableTd, TableTr, Text } from '@mantine/core';

/** One collapsed part of a journey leg's calling points in
 * `JourneyTimeline`: the train's stops before the traveller boards, or
 * after they alight. Collapsed by default, behind a real button with
 * `aria-expanded` that names what it holds ("Earlier stops from London
 * Kings Cross (5)").
 *
 * Table rows can't go in an `Accordion` or `Collapse` (both render
 * `div`s, which a `tbody` can't hold), so the control is a full-width row
 * of its own and the rows it shows sit in a `tbody` of their own, which
 * the button names with `aria-controls`. Its rows are the same live rows
 * the rest of the table renders, so times, platforms, cancellations and
 * delays keep updating while it's open.
 *
 * `hint` is a short line shown beside the control while the train is in
 * this part of the route (before the traveller boards, or after they get
 * off), so they can see where it is without opening it. */
export function LegStopsDisclosure({
  label,
  colSpan,
  hint,
  children,
}: {
  label: string;
  colSpan: number;
  hint?: string | null | undefined;
  children: ReactNode;
}) {
  const [open, setOpen] = useState(false);
  const id = useId();
  return (
    <>
      <TableTbody>
        <TableTr data-leg-stops-toggle>
          <TableTd colSpan={colSpan}>
            <Group gap="xs" wrap="wrap">
              <Button
                variant="subtle"
                size="compact-sm"
                aria-expanded={open}
                aria-controls={id}
                onClick={() => setOpen((value) => !value)}
                leftSection={
                  <AccordionChevron
                    size={14}
                    // eslint-disable-next-line react/forbid-component-props -- the chevron turns with the open state
                    style={{ transform: open ? 'rotate(0deg)' : 'rotate(-90deg)', transition: 'transform 150ms ease' }}
                  />
                }
              >
                {label}
              </Button>
              {hint && (
                <Text size="sm" c="dimmed" data-leg-stops-hint>
                  {hint}
                </Text>
              )}
            </Group>
          </TableTd>
        </TableTr>
      </TableTbody>
      <TableTbody id={id} data-leg-stops-section>
        {open ? children : null}
      </TableTbody>
    </>
  );
}
