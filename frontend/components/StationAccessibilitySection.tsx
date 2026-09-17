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
 * `aria-expanded` button, and unmounts what it hides.
 *
 * `qualifier` exists because `Accordion`'s panel is a `role="region"`
 * landmark named (via `aria-labelledby`) by its own control. A station page
 * renders several of these, and their visible labels repeat by nature --
 * "1 item" under Lifts and "1 item" under Car parks, "Raw data" under both
 * of two unmodelled keys. Two landmarks with the same accessible name is an
 * axe `landmark-unique` failure, and, more to the point, a screen reader's
 * landmark list then offers several identical "1 item" regions with nothing
 * to choose between them. Caught by a full-ruleset axe run against
 * `/stations/PAD` with every disclosure expanded -- neither the previous
 * five-rule spec nor an unexpanded page could see it.
 *
 * The qualifier goes on `aria-label`, not into the visible text: on screen
 * the label sits directly under the `humanizeKey(...)` heading that already
 * says which field it belongs to, so repeating it would be noise, while the
 * accessible name has no such context to lean on. WCAG 2.5.3 (Label in
 * Name) is satisfied because the accessible name still *contains* the
 * visible label verbatim -- "Car parks: 1 item" starts a voice-control
 * match on "1 item" just as well. */
function Disclosure({
  label,
  qualifier,
  children,
}: {
  label: string;
  qualifier?: string;
  children: React.ReactNode;
}) {
  return (
    <Accordion chevronPosition="left" keepMounted={false}>
      <AccordionItem value="disclosure">
        {/* A bare string, not a `<Text>`: `AccordionControl` renders its
            children inside a `<button>`, and Mantine's `<Text>` is a `<p>`,
            which is not valid button content. */}
        <AccordionControl aria-label={qualifier ? `${qualifier}: ${label}` : undefined}>
          {label}
        </AccordionControl>
        <AccordionPanel>{children}</AccordionPanel>
      </AccordionItem>
    </Accordion>
  );
}

/** Every category group that has at least one key rendering to something.
 * Computed before any JSX so the section can tell "present, and here it
 * is" from "present, but every value the feed published was empty" -- the
 * latter reads as the same fact as a `200 {}` and gets the same sentence,
 * rather than a heading with nothing under it. */
function renderableGroups(data: StationAccessibilityData) {
  return ACCESSIBILITY_CATEGORIES.map((category) => ({
    heading: category.heading,
    // A key whose value renders to nothing at all (an empty object, an
    // empty array, an object whose every own value was null) is skipped
    // outright rather than printing a label with blank space under it --
    // Decision 7's "don't invent a row for data that isn't there", one
    // level below the group it applies to.
    entries: category.keys
      .filter((key) => hasRenderableValue(data[key]))
      .map((key) => ({ key, value: renderAccessibilityValue(data[key]) }))
      .filter((entry) => !isEmptyRenderable(entry.value)),
  })).filter((group) => group.entries.length > 0);
}

/** Renders one already-computed `RenderableValue` -- see
 * `frontend/lib/stationAccessibility.ts`'s `renderAccessibilityValue` for
 * the shape-detection rules this only displays. The recursion here is
 * bounded by that function's own depth limit: an `'items'` entry's children
 * are only ever `'text'`, `'rows'` or `'raw'`, never another `'items'`. */
function AccessibilityValue({ value, name }: { value: RenderableValue; name?: string }) {
  if (value.kind === 'text') {
    return <Text size="sm">{value.text}</Text>;
  }
  if (value.kind === 'rows') {
    return (
      <Stack gap={2}>
        {value.rows.map((row, index) => (
          // Two different source keys can humanize to the same label, so
          // the label alone is not a safe key.
          // eslint-disable-next-line react/no-array-index-key -- see above
          <Group key={`${row.label}-${index}`} gap="xs" wrap="wrap">
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
    // Counted from what will actually be shown, not from `value.count` (the
    // source array's length): an entry that renders to nothing is dropped,
    // and a control reading "2 items" over one visible row would be a lie.
    const visible = value.items.filter((item) => !isEmptyRenderable(item));
    // No "Show" verb: the control keeps one static accessible name in both
    // states, and the chevron plus `aria-expanded` carry open/closed. A
    // button still reading "Show 2 items" while the items are on screen
    // would contradict its own `aria-expanded="true"`.
    const label = visible.length === 1 ? '1 item' : `${visible.length} items`;
    return (
      <Disclosure label={label} qualifier={name}>
        <Stack gap="sm">
          {visible.map((item, index) => (
            // A nested `'raw'` child would otherwise be another bare "Raw
            // data" region, colliding with its siblings inside this very
            // list -- number them so each stays distinguishable.
            // eslint-disable-next-line react/no-array-index-key -- items have no stable id in this genuinely-unknown-shape data
            <AccessibilityValue key={index} value={item} name={name ? `${name} ${index + 1}` : undefined} />
          ))}
        </Stack>
      </Disclosure>
    );
  }
  return (
    <Disclosure label="Raw data" qualifier={name}>
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
  const groups = result.coverage === 'present' ? renderableGroups(result.data) : [];
  // A `200` whose every allowlisted value turned out to be `{}`/`[]` is the
  // same fact as a `200 {}` from the reader's point of view -- the station
  // has published nothing -- so it gets the same sentence rather than an
  // empty section under a heading.
  const nothingPublished =
    result.coverage === 'empty' || (result.coverage === 'present' && groups.length === 0);

  return (
    <Stack gap="xs">
      <Title order={2} size="h4">
        Accessibility &amp; facilities
      </Title>
      {result.coverage === 'unavailable' && (
        <Text c="dimmed">We don&apos;t have station reference data for this station yet.</Text>
      )}
      {nothingPublished && (
        <Text c="dimmed">
          No accessibility or facilities details have been published for this station.
        </Text>
      )}
      {groups.map((group) => (
        <Stack key={group.heading} gap={4}>
          <Text size="sm" fw={700}>
            {group.heading}
          </Text>
          {group.entries.map((entry) => (
            <Stack key={entry.key} gap={2} pl="sm">
              <Text size="sm" fw={500}>
                {humanizeKey(entry.key)}
              </Text>
              {/* The same humanized key is passed down as the disclosure's
                  `qualifier` -- see `Disclosure`'s own doc comment for why
                  a bare "1 item" / "Raw data" landmark name is both an axe
                  `landmark-unique` failure and useless to navigate by. */}
              <AccessibilityValue value={entry.value} name={humanizeKey(entry.key)} />
            </Stack>
          ))}
        </Stack>
      ))}
    </Stack>
  );
}
