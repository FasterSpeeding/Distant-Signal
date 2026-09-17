'use client';

import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Badge,
  Code,
  Group,
  List,
  ListItem,
  Stack,
  Text,
  Title,
  Typography,
} from '@mantine/core';
import {
  ACCESSIBILITY_CATEGORIES,
  hasRenderableValue,
  humanizeKey,
  isEmptyNode,
  renderAccessibilityValue,
  type AccessibilityNode,
  type LabelledNode,
} from '@/lib/stationAccessibility';
import type { StationAccessibilityData } from '@/lib/types';
import { TextLink } from './TextLink';

export interface StationAccessibilitySectionProps {
  result:
    | { coverage: 'unavailable' }
    | { coverage: 'empty' }
    | { coverage: 'present'; data: StationAccessibilityData };
}

/** One collapsed-by-default disclosure, used for every item collection and
 * for the raw-JSON last resort (structured-rendering design §4.5: a large
 * or deeply nested array must not produce a wall of text -- the sample has
 * arrays up to 20 platforms and 18 lifts).
 *
 * Deliberately `Accordion` + `keepMounted={false}` rather than the
 * `<Spoiler maxHeight={0}>` the original spec sketched. Spoiler decides
 * whether to render its own show/hide control by measuring the rendered
 * content (`maxHeight < height`, `@mantine/core`'s Spoiler.tsx), and it
 * keeps the collapsed content mounted and merely clipped -- so the control
 * is absent wherever content has no measured height, and the hidden markup
 * stays in the accessibility tree of a page `e2e/accessibility.spec.ts`
 * sweeps with axe-core. `Accordion` is this codebase's own established
 * answer for collapsed-by-default content (`IssueList.tsx`, whose comment
 * documents the same `keepMounted={false}` reasoning), renders a real
 * `aria-expanded` button, and unmounts what it hides.
 *
 * `qualifier` exists because `Accordion`'s panel is a `role="region"`
 * landmark named (via `aria-labelledby`) by its own control. A station page
 * renders several of these, and their visible labels repeat by nature --
 * "1 item" under Lifts and "1 item" under Car parks. Two landmarks with the
 * same accessible name is an axe `landmark-unique` failure, and, more to
 * the point, a screen reader's landmark list then offers several identical
 * "1 item" regions with nothing to choose between them. Caught by a
 * full-ruleset axe run against `/stations/PAD` with every disclosure
 * expanded -- neither the previous five-rule spec nor an unexpanded page
 * could see it.
 *
 * The qualifier goes on `aria-label`, not into the visible text: on screen
 * the label sits directly under the heading that already says which field
 * it belongs to, so repeating it would be noise, while the accessible name
 * has no such context to lean on. WCAG 2.5.3 (Label in Name) is satisfied
 * because the accessible name still *contains* the visible label verbatim
 * -- "Car parks: 1 item" starts a voice-control match on "1 item" just as
 * well. */
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

/** The tick/cross in front of a Pattern A availability line.
 *
 * `aria-hidden`, and never the only carrier of the fact: the words
 * "Available" / "Not available" always sit beside it (design §4.2). No
 * axe rule can check that -- an icon with no accessible name is not a
 * violation, it is just silent -- so the component's own test asserts it
 * (§8's "two rules it still cannot enforce").
 *
 * Inline SVG rather than a glyph or an icon package, matching
 * `InfoIcon.tsx`'s reasoning: `@tabler/icons-react` is not a dependency,
 * and "✓"/"✗" render as broken-looking emoji fallbacks in some font
 * stacks. */
function AvailabilityIcon({ available }: { available: boolean }) {
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width="14"
      height="14"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="3"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      style={{ flexShrink: 0 }}
    >
      {available ? (
        <polyline points="20 6 9 17 4 12" />
      ) : (
        <>
          <line x1="18" y1="6" x2="6" y2="18" />
          <line x1="6" y1="6" x2="18" y2="18" />
        </>
      )}
    </svg>
  );
}

/** True for the node kinds that read correctly on the same line as their
 * label ("Phone: 0345 077 4224"). Everything else -- a nested collection's
 * disclosure, a contact block, an opening-times list -- gets the label on
 * its own line with the value indented under it, which is the only way a
 * key/value row can hold a multi-line value without the two running
 * together. */
function isInline(node: AccessibilityNode): boolean {
  return node.kind === 'text' || node.kind === 'sentence' || node.kind === 'link';
}

function LabelledNodeView({ field, path }: { field: LabelledNode; path?: string }) {
  const childPath = field.label && path ? `${path} ${field.label}` : (field.label ?? path);
  if (!field.label) {
    return <AccessibilityNodeView node={field.node} path={path} />;
  }
  // A Pattern A record states its own subject on its availability line
  // ("Wi-fi — Available"), so a label above it would print the name twice.
  if (field.node.kind === 'facility') {
    return <AccessibilityNodeView node={field.node} label={field.label} path={childPath} />;
  }
  if (isInline(field.node)) {
    return (
      <Group gap="xs" wrap="wrap">
        <Text size="sm" fw={500}>
          {field.label}:
        </Text>
        <AccessibilityNodeView node={field.node} path={childPath} />
      </Group>
    );
  }
  return (
    <Stack gap={2}>
      <Text size="sm" fw={500}>
        {field.label}
      </Text>
      <Stack gap={2} pl="sm">
        <AccessibilityNodeView node={field.node} path={childPath} />
      </Stack>
    </Stack>
  );
}

function FieldsView({ fields, path }: { fields: LabelledNode[]; path?: string }) {
  return (
    <Stack gap={2}>
      {fields.map((field, index) => (
        // Two different source keys can humanize to the same label, and an
        // unlabelled sentence has no key at all, so neither is a safe React
        // key.
        // eslint-disable-next-line react/no-array-index-key -- see above
        <LabelledNodeView key={index} field={field} path={path} />
      ))}
    </Stack>
  );
}

/** Renders one already-classified `AccessibilityNode` -- see
 * `frontend/lib/stationAccessibility.ts` for the shape-detection rules this
 * only displays.
 *
 * Two different naming props, because they answer two different questions.
 * `label` is the node's own subject, shown on screen and only meaningful
 * for Pattern A ("Wi-fi — Available"). `path` is the accumulated humanized
 * trail to this node, never shown, and exists purely so nested
 * `Disclosure`s get distinct `role="region"` accessible names.
 *
 * Nothing here emits a heading. The section owns exactly one `h2` and the
 * page one `h1`; a pattern component adding an `h3`/`h4` of its own would
 * put feed-shaped structure into the page outline, and headings arriving
 * *inside* sanitized note copy are demoted to `<p><strong>` by
 * `sanitizeRichText` for the same reason (design §4.7). */
function AccessibilityNodeView({
  node,
  label,
  path,
}: {
  node: AccessibilityNode;
  label?: string;
  path?: string;
}) {
  switch (node.kind) {
    case 'text':
    case 'sentence':
      return <Text size="sm">{node.text}</Text>;

    case 'richText':
      // Already sanitized in the lib, by `sanitizeRichText`, before this
      // node was ever constructed -- a component cannot forget to do it,
      // and the string that reaches the DOM has been through DOMPurify on
      // whichever side rendered it (this component is `'use client'` but
      // its first render is Next's server pass, exactly as
      // `DisruptionDetail.tsx` has worked since the incident-detail page).
      //
      // `Typography` is Mantine 9's name for the design's
      // `TypographyStylesProvider` (§4.7) -- it restores `p`/`ul`/`li`/
      // `strong` styling inside a Mantine reset that otherwise flattens
      // them, which matters far more here than for incident copy: this feed
      // uses 53 `ul`s and 136 `li`s. `data-rich-text` is the CSS hook for
      // `app/globals.css`'s `[data-rich-text] a` rule -- these anchors come
      // from external markup, so they carry no Mantine class and would
      // otherwise render browser-default blue against a grape theme.
      return (
        <Typography fz="sm" data-rich-text>
          <div dangerouslySetInnerHTML={{ __html: node.html }} />
        </Typography>
      );

    case 'tokens':
      return (
        <Group gap={4} wrap="wrap">
          {node.tokens.map((token, index) => (
            // eslint-disable-next-line react/no-array-index-key -- feed tokens are not unique
            <Badge key={`${token}-${index}`} variant="light" color="gray" tt="none" fw={400}>
              {token}
            </Badge>
          ))}
        </Group>
      );

    case 'facility':
      return (
        <Stack gap={2}>
          <Group gap={6} wrap="nowrap" align="center">
            <AvailabilityIcon available={node.available} />
            {/* The words, always -- the icon is decorative and carries none
                of this (design §4.2). */}
            <Text size="sm" fw={500}>
              {label ? `${label} — ` : ''}
              {node.available ? 'Available' : 'Not available'}
            </Text>
          </Group>
          {node.parts.length > 0 && (
            <Stack gap={2} pl="sm">
              <FieldsView fields={node.parts} path={path} />
            </Stack>
          )}
        </Stack>
      );

    case 'openingTimes':
      return (
        <Stack gap={0}>
          {node.entries.map((entry, index) => (
            // eslint-disable-next-line react/no-array-index-key -- entries have no id
            <Text key={index} size="sm">
              {entry.days === '' ? entry.hours : `${entry.days}, ${entry.hours}`}
            </Text>
          ))}
        </Stack>
      );

    case 'contact':
    case 'fields':
      return <FieldsView fields={node.fields} path={path} />;

    case 'bullets':
      return (
        <List size="sm" spacing={2}>
          {node.items.map((item, index) => (
            // eslint-disable-next-line react/no-array-index-key -- bullets have no id
            <ListItem key={index}>
              <AccessibilityNodeView node={item} path={path} />
            </ListItem>
          ))}
        </List>
      );

    case 'collection': {
      // Counted from what will actually be shown, not from the source
      // array's length: an item that renders to nothing is dropped, and a
      // control reading "2 items" over one visible block would be a lie.
      const visible = node.items.filter(
        (item) => item.label.trim() !== '' || item.link || !isEmptyNode(item.body),
      );
      // No "Show" verb: the control keeps one static accessible name in
      // both states, and the chevron plus `aria-expanded` carry
      // open/closed. A button still reading "Show 2 items" while the items
      // are on screen would contradict its own `aria-expanded="true"`.
      const count = visible.length === 1 ? '1 item' : `${visible.length} items`;
      return (
        <Disclosure label={count} qualifier={path}>
          <Stack gap="sm">
            {visible.map((item, index) => (
              // eslint-disable-next-line react/no-array-index-key -- feed items have no stable id
              <Stack key={index} gap={2}>
                {item.link ? (
                  <TextLink
                    href={item.link.href}
                    underline="always"
                    target={item.link.external ? '_blank' : undefined}
                    rel={item.link.external ? 'noopener noreferrer' : undefined}
                  >
                    {item.label}
                  </TextLink>
                ) : (
                  <Text size="sm" fw={600}>
                    {item.label}
                  </Text>
                )}
                {!isEmptyNode(item.body) && (
                  <Stack gap={2} pl="sm">
                    <AccessibilityNodeView
                      node={item.body}
                      path={path ? `${path} ${item.label}` : item.label}
                    />
                  </Stack>
                )}
              </Stack>
            ))}
          </Stack>
        </Disclosure>
      );
    }

    case 'list': {
      const visible = node.items.filter((item) => !isEmptyNode(item));
      const count = visible.length === 1 ? '1 item' : `${visible.length} items`;
      return (
        <Disclosure label={count} qualifier={path}>
          <Stack gap="sm">
            {visible.map((item, index) => (
              <AccessibilityNodeView
                // eslint-disable-next-line react/no-array-index-key -- feed items have no stable id
                key={index}
                node={item}
                // A nested `'raw'` child would otherwise be another bare
                // "Raw data" region, colliding with its siblings inside
                // this very list -- number them so each stays
                // distinguishable.
                path={path ? `${path} ${index + 1}` : undefined}
              />
            ))}
          </Stack>
        </Disclosure>
      );
    }

    case 'link':
      return (
        <TextLink
          href={node.href}
          underline="always"
          target={node.external ? '_blank' : undefined}
          rel={node.external ? 'noopener noreferrer' : undefined}
        >
          {node.text}
        </TextLink>
      );

    case 'raw':
      return (
        <Disclosure label="Raw data" qualifier={path}>
          <Code block>{node.json}</Code>
        </Disclosure>
      );
  }
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
    // level below the group it applies to. Load-bearing: `null` is
    // pervasive in this feed (design §2.5).
    entries: category.keys
      .filter((key) => hasRenderableValue(data[key]))
      .map((key) => ({ key, node: renderAccessibilityValue(data[key]) }))
      .filter((entry) => !isEmptyNode(entry.node)),
  })).filter((group) => group.entries.length > 0);
}

/** Fourth, independent section on `/stations/[crs]`.
 *
 * Rendering follows
 * docs/superpowers/specs/2026-09-16-structured-accessibility-rendering-design.md,
 * which replaced the original spec's generic raw-JSON-fallback renderer
 * (Decision 6) on the strength of a 31-station survey of real payloads:
 * seven confirmed structural patterns, dispatched on shape rather than on
 * key name, with a narrower labelled-key/value fallback and `JSON.stringify`
 * only as a last resort. Everything else about the feature -- the twelve-key
 * allowlist, the route, the wire type, the grouping and order below, the
 * three honest states -- is unchanged from
 * docs/superpowers/specs/2026-09-12-station-accessibility-design.md.
 *
 * Heading is deliberately "Accessibility & facilities", not bare
 * "Accessibility", so it reads unambiguously as physical-access information
 * (Decision 8) -- this codebase separately uses "accessibility" for WCAG
 * audits, and this component's own name/copy are the only mitigations for
 * that collision; the wire type name (`StationAccessibilityData`) is
 * deliberately left alone.
 *
 * `'unavailable'` (the route 404'd: no `stations` row for this CRS at all)
 * and `'empty'` (`200 {}`: the row exists but published none of the twelve
 * allowlisted keys) are two genuinely different facts and get two
 * different sentences -- never collapsed into one "no data" message. */
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
              {/* A Pattern A key says its own name on its availability line
                  ("Lifts — Available"), so a separate label above it would
                  print the name twice. */}
              {entry.node.kind !== 'facility' && (
                <Text size="sm" fw={500}>
                  {humanizeKey(entry.key)}
                </Text>
              )}
              {/* The humanized key is passed down as the disclosure
                  `qualifier` -- see `Disclosure`'s own doc comment for why a
                  bare "1 item" / "Raw data" landmark name is both an axe
                  `landmark-unique` failure and useless to navigate by. */}
              <AccessibilityNodeView
                node={entry.node}
                label={humanizeKey(entry.key)}
                path={humanizeKey(entry.key)}
              />
            </Stack>
          ))}
        </Stack>
      ))}
    </Stack>
  );
}
