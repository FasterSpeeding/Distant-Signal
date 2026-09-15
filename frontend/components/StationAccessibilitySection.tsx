'use client';

import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Code,
  Group,
  Stack,
  Text,
  Title,
} from '@mantine/core';
import {
  ACCESSIBILITY_CATEGORIES,
  hasRenderableValue,
  humanizeKey,
  isEmptyRenderable,
  renderAccessibilityValue,
  type RenderableValue,
} from '@/lib/stationAccessibility';
import type { StationAccessibilityData } from '@/lib/types';

export interface StationAccessibilitySectionProps {
  result:
    | { coverage: 'unavailable' }
    | { coverage: 'empty' }
    | { coverage: 'present'; data: StationAccessibilityData };
}

/** One collapsed-by-default disclosure, used for both the raw-JSON fallback
 * and an array-of-objects item list (design spec Decision 6: a large or
 * deeply nested array must not produce a wall of text).
 *
 * Deliberately `Accordion` + `keepMounted={false}` rather than the
 * `<Spoiler maxHeight={0}>` the spec sketches. Spoiler decides whether to
 * render its own show/hide control by measuring the rendered content
 * (`maxHeight < height`, `@mantine/core`'s Spoiler.tsx), and it keeps the
 * collapsed content mounted and merely clipped -- so the control is absent
 * wherever content has no measured height, and the hidden JSON stays in
 * the accessibility tree of a page `e2e/accessibility.spec.ts` sweeps with
 * axe-core. `Accordion` is this codebase's own established answer for
 * collapsed-by-default content (`IssueList.tsx`, whose comment documents
 * the same `keepMounted={false}` reasoning), renders a real
 * `aria-expanded` button, and unmounts what it hides. */
function Disclosure({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <Accordion chevronPosition="left" keepMounted={false}>
      <AccordionItem value="disclosure">
        <AccordionControl>
          <Text size="sm">{label}</Text>
        </AccordionControl>
        <AccordionPanel>{children}</AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}

/** Renders one already-computed `RenderableValue` -- see
 * `frontend/lib/stationAccessibility.ts`'s `renderAccessibilityValue` for
 * the shape-detection rules this only displays. The recursion here is
 * bounded by that function's own depth limit: an `'items'` entry's children
 * are only ever `'text'`, `'rows'` or `'raw'`, never another `'items'`. */
function AccessibilityValue({ value }: { value: RenderableValue }) {
  if (value.kind === 'text') {
    return <Text size="sm">{value.text}</Text>;
  }
  if (value.kind === 'rows') {
    return (
      <Stack gap={2}>
        {value.rows.map((row) => (
          <Group key={row.label} gap="xs" wrap="wrap">
            <Text size="sm" fw={500}>
              {row.label}:
            </Text>
            <Text size="sm">{row.value}</Text>
          </Group>
        ))}
      </Stack>
    );
  }
  if (value.kind === 'items') {
    const label = value.count === 1 ? 'Show 1 item' : `Show ${value.count} items`;
    return (
      <Disclosure label={label}>
        <Stack gap="sm">
          {value.items.map((item, index) => (
            // eslint-disable-next-line react/no-array-index-key -- items have no stable id in this genuinely-unknown-shape data
            <AccessibilityValue key={index} value={item} />
          ))}
        </Stack>
      </Disclosure>
    );
  }
  return (
    <Disclosure label="Show raw data">
      <Code block>{value.json}</Code>
    </Disclosure>
  );
}

/** Fourth, independent section on `/stations/[crs]` -- see
 * docs/superpowers/specs/2026-09-12-station-accessibility-design.md
 * Decisions 6-9. Heading is deliberately "Accessibility & facilities", not
 * bare "Accessibility", so it reads unambiguously as physical-access
 * information (Decision 8) -- this codebase separately uses
 * "accessibility" for WCAG audits (Correction 4), and this component's own
 * name/copy are the only mitigations for that collision; the wire type name
 * (`StationAccessibilityData`) is deliberately left alone.
 *
 * `'unavailable'` (the route 404'd: no `stations` row for this CRS at all)
 * and `'empty'` (`200 {}`: the row exists but published none of the twelve
 * allowlisted keys) are two genuinely different facts and get two
 * different sentences -- never collapsed into one "no data" message
 * (Correction 5). */
export function StationAccessibilitySection({ result }: StationAccessibilitySectionProps) {
  return (
    <Stack gap="xs">
      <Title order={2} size="h4">
        Accessibility &amp; facilities
      </Title>
      {result.coverage === 'unavailable' && (
        <Text c="dimmed">We don&apos;t have station reference data for this station yet.</Text>
      )}
      {result.coverage === 'empty' && (
        <Text c="dimmed">
          No accessibility or facilities details have been published for this station.
        </Text>
      )}
      {result.coverage === 'present' &&
        ACCESSIBILITY_CATEGORIES.map((category) => {
          // A key whose value renders to nothing at all (an empty object,
          // an empty array, an object whose every own value was null) is
          // skipped outright rather than printing a label with blank space
          // under it -- Decision 7's "don't invent a row for data that
          // isn't there", one level below the group it applies to.
          const rendered = category.keys
            .filter((key) => hasRenderableValue(result.data[key]))
            .map((key) => ({ key, value: renderAccessibilityValue(result.data[key]) }))
            .filter((entry) => !isEmptyRenderable(entry.value));
          if (rendered.length === 0) return null;
          return (
            <Stack key={category.heading} gap={4}>
              <Text size="sm" fw={700}>
                {category.heading}
              </Text>
              {rendered.map((entry) => (
                <Stack key={entry.key} gap={2} pl="sm">
                  <Text size="sm" fw={500}>
                    {humanizeKey(entry.key)}
                  </Text>
                  <AccessibilityValue value={entry.value} />
                </Stack>
              ))}
            </Stack>
          );
        })}
    </Stack>
  );
}
