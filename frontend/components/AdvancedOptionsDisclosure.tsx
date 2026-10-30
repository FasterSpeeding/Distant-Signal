'use client';

import type { ReactNode } from 'react';
import { Accordion, AccordionControl, AccordionItem, AccordionPanel, Text } from '@mantine/core';

/** Flush, chrome-free disclosure, as `StationAccessibilitySection.tsx`'s
 * and `WorkingTimetable.tsx`'s. */
const DISCLOSURE_STYLES = {
  item: { border: 'none', backgroundColor: 'transparent' },
  control: { padding: 0, paddingBlock: 'var(--mantine-spacing-xs)' },
  chevron: { marginInlineEnd: 'var(--mantine-spacing-xs)' },
  panel: { paddingInlineStart: 0 },
  content: { paddingInline: 0 },
} as const;

/** The station lists the planner's "Advanced options" and the `/trains`
 * boards' share, word for word: each picker's label, help text and row
 * noun (`StationListPicker`'s `label`, `description` and `itemNoun`). */
export const STOP_LIST_FIELDS = {
  passThrough: {
    label: 'Pass through (in order)',
    description:
      'Every route goes through these stations, in this order, whether or not the train stops there. Unlike "Call at", you don\'t need to stop. A group such as "Any of the London Terminals" is passed through any one of its stations.',
    itemNoun: 'pass-through station',
  },
  avoid: {
    label: 'Avoid completely',
    description: 'Never stop at or travel through these.',
    itemNoun: 'avoided station',
  },
  avoidStop: {
    label: "Don't stop at",
    description: 'Trains may run through these, but never stop there.',
    itemNoun: 'no-stop station',
  },
} as const;

/** "Advanced options", collapsed by default: a flush `Accordion` like the
 * station page's disclosures (a real `aria-expanded` button), shared by
 * the planner (`PlanTripAdvancedOptions`) and the `/trains` boards
 * (`TrainsAdvancedOptions`). Its control says how many options are set
 * ("2 set") and how many need fixing, and a dimmed line under it lists
 * them (`summary`), so a restored or shared search never hides a
 * constraint. The parent controls whether it is open, so it can open it on
 * an error inside. */
export function AdvancedOptionsDisclosure({
  count,
  problems,
  summary,
  opened,
  onOpenedChange,
  children,
}: {
  count: number;
  problems: number;
  summary: string;
  opened: boolean;
  onOpenedChange: (opened: boolean) => void;
  children: ReactNode;
}) {
  const status = [count > 0 ? `${count} set` : null, problems > 0 ? `${problems} to fix` : null]
    .filter(Boolean)
    .join(', ');

  return (
    <Accordion
      chevronPosition="left"
      keepMounted={false}
      styles={DISCLOSURE_STYLES}
      value={opened ? 'advanced' : null}
      onChange={(value) => onOpenedChange(value === 'advanced')}
    >
      <AccordionItem value="advanced">
        <AccordionControl>
          Advanced options
          {status && (
            <Text span size="sm" c={problems > 0 ? 'var(--ds-color-error-text)' : 'dimmed'} ml={6}>
              ({status})
            </Text>
          )}
        </AccordionControl>
        {!opened && summary && (
          <Text size="xs" c="dimmed" pb="xs">
            {summary}
          </Text>
        )}
        <AccordionPanel>{children}</AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}
