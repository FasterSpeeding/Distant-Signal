'use client';

import { useMemo } from 'react';
import {
  Accordion,
  AccordionControl,
  AccordionItem,
  AccordionPanel,
  Badge,
  Code,
  Group,
  Stack,
  Text,
  Title,
  Typography,
} from '@mantine/core';
import {
  ACCESSIBILITY_CATEGORIES,
  computeAtAGlance,
  dedupeAcrossSection,
  hasRenderableValue,
  hostLabel,
  humanizeKey,
  isEmptyNode,
  renderAccessibilityValue,
  sortEntriesByKind,
  type AccessibilityNode,
  type AtAGlanceFact,
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
/** Review §3.5.5's chrome-stripping direction: no border, no background, no
 * extra horizontal padding, so the control sits flush in the text column at
 * its label's own indent rather than reading as a divider/card. The
 * `landmark-unique` reasoning in this component's own doc comment (below)
 * is unaffected -- these overrides are visual only. */
const DISCLOSURE_STYLES = {
  item: { border: 'none', backgroundColor: 'transparent' },
  control: { padding: 0, paddingBlock: 'var(--mantine-spacing-xs)' },
  chevron: { marginInlineEnd: 'var(--mantine-spacing-xs)' },
  panel: { paddingInlineStart: 'var(--mantine-spacing-md)' },
} as const;

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
    <Accordion chevronPosition="left" keepMounted={false} styles={DISCLOSURE_STYLES}>
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

/** review §3.5.5: the on-screen control text names WHAT there are several
 * of ("13 toilet locations"), not just how many ("13 items") -- the
 * question a reader actually has before deciding whether to open it.
 * `noun`, when given, is the field's own humanized label; lowercased so it
 * reads as a plural noun phrase rather than a re-capitalised heading. Falls
 * back to bare "item(s)" when no more specific noun is available (the
 * fallback list branch's numbered "Car parks 1", "Car parks 2" children,
 * for instance, have no singular English noun of their own to lend). */
function describeCount(count: number, noun?: string): string {
  return `${count} ${noun ? noun.toLowerCase() : count === 1 ? 'item' : 'items'}`;
}

/** review §3.5.5: a collection this short is not worth hiding behind a
 * disclosure at all -- "which lift serves platform 8" is exactly the
 * question a reader has, and making them click through unlabelled chrome to
 * find out is the itself the defect. Six of the section's nine disclosures
 * are three items or fewer. Never true for the pluralised "N items" wording
 * `describeCount` produces (this fires on the count itself, before that
 * wording exists) -- and a single item never reaches an accordion either,
 * satisfying the plan's separate "never render '1 item' as an accordion"
 * requirement as a consequence of the same threshold. */
const INLINE_ITEM_THRESHOLD = 3;

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
 * stacks.
 *
 * `stroke="currentColor"` picks up `--ds-color-good-icon`/
 * `--ds-color-bad-icon` (`app/globals.css`) from the wrapping `<span>`'s
 * inline `color` -- review §3.5.6's "colour the glyph", matching
 * `StatusBadge`'s own green/red resolution. Colour is applied ONLY here,
 * never to the adjacent text: the icon is decorative reinforcement, and
 * this component's own test (`states availability in words, with the icon
 * purely decorative`) already asserts the fact survives with every
 * `aria-hidden` node stripped, so the text itself must keep the ordinary
 * body-text contrast guarantee rather than inherit an unaudited colour. */
function AvailabilityIcon({ available }: { available: boolean }) {
  return (
    <span
      style={{
        display: 'inline-flex',
        color: available ? 'var(--ds-color-good-icon)' : 'var(--ds-color-bad-icon)',
      }}
    >
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
    </span>
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
    <Stack gap={4}>
      <Text size="sm" fw={500}>
        {field.label}
      </Text>
      <Stack gap={4} pl="sm">
        <AccessibilityNodeView node={field.node} label={field.label} path={childPath} />
      </Stack>
    </Stack>
  );
}

function FieldsView({ fields, path }: { fields: LabelledNode[]; path?: string }) {
  return (
    <Stack gap={4}>
      {fields.map((field, index) => (
        // Two different source keys can humanize to the same label, and an
        // unlabelled sentence has no key at all, so neither is a safe React
        // key.
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
    case 'sentence': {
      // review §3.5.6: a plain boolean field (`wheelchairsAvailable: true`)
      // used to print as a bare "Yes"/"No" word -- one visual language for
      // a fact that, everywhere else in this section, gets the tick/cross
      // + colour treatment a `facility`'s own availability line uses.
      // `primitiveText` (`stationAccessibility.ts`) is the only producer of
      // these two exact strings on a `text`-kind node, so matching on them
      // here unifies the two renderings without a new node kind, and
      // without touching the existing lib-level tests that assert
      // `renderAccessibilityValue(true)` stays `{kind:'text', text:'Yes'}`.
      if (node.kind === 'text' && (node.text === 'Yes' || node.text === 'No')) {
        const available = node.text === 'Yes';
        return (
          <span style={{ display: 'inline-flex', alignItems: 'center', gap: 6 }}>
            <AvailabilityIcon available={available} />
            <Text size="sm" span>
              {node.text}
            </Text>
          </span>
        );
      }
      return <Text size="sm">{node.text}</Text>;
    }

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
            <Badge key={`${token}-${index}`} variant="light" color="gray" tt="none" fw={400}>
              {token}
            </Badge>
          ))}
        </Group>
      );

    case 'facility':
      return (
        <Stack gap={4}>
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
            <Stack gap={4} pl="sm">
              <FieldsView fields={node.parts} path={path} />
            </Stack>
          )}
        </Stack>
      );

    case 'openingTimes':
      return (
        <Stack gap={0}>
          {node.entries.map((entry, index) => (
            <Text key={index} size="sm">
              {/* Joined from whichever halves exist, not with a fixed
                  comma: an entry whose `openingStatus` the feed left blank
                  and which carries no period has no hours to print, and
                  "Mon-Fri, " would be a dangling comma. Every one of the
                  sample's 304 entries has both. */}
              {[entry.days, entry.hours].filter((part) => part !== '').join(', ')}
            </Text>
          ))}
        </Stack>
      );

    case 'contact':
    case 'fields':
      return <FieldsView fields={node.fields} path={path} />;

    case 'bullets':
      return (
        // A real `<ul>`, so the item's sentences are a list to a screen
        // reader rather than a run of paragraphs. Deliberately NOT Mantine's
        // `List`/`ListItem`: `ListItem` wraps its children in a `<span>`
        // (see `@mantine/core`'s ListItem.cjs), and every child this branch
        // can produce -- `Text` is a `<p>`, rich text is a `<div>` -- is
        // flow content, which is not valid inside phrasing content. The two
        // declarations below are what Mantine's own List root sets.
        <ul
          style={{
            margin: 0,
            paddingInlineStart: 'var(--mantine-spacing-lg)',
            listStylePosition: 'outside',
          }}
        >
          {node.items.map((item, index) => (
            <li key={index}>
              <AccessibilityNodeView node={item} path={path} />
            </li>
          ))}
        </ul>
      );

    case 'collection': {
      // Counted from what will actually be shown, not from the source
      // array's length: an item that renders to nothing is dropped, and a
      // control reading "2 items" over one visible block would be a lie.
      const visible = node.items.filter(
        (item) => item.label.trim() !== '' || item.link || !isEmptyNode(item.body),
      );
      const items = (
        <Stack gap="sm">
          {visible.map((item, index) => (
            <Stack key={index} gap={4}>
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
                <Stack gap={4} pl="sm">
                  <AccessibilityNodeView
                    node={item.body}
                    path={path ? `${path} ${item.label}` : item.label}
                  />
                </Stack>
              )}
            </Stack>
          ))}
        </Stack>
      );
      // review §3.5.5: a handful of items is shown directly, not hidden
      // behind a disclosure -- see `INLINE_ITEM_THRESHOLD`'s own comment.
      if (visible.length <= INLINE_ITEM_THRESHOLD) return items;
      return (
        <Disclosure label={describeCount(visible.length, label)} qualifier={path}>
          {items}
        </Disclosure>
      );
    }

    case 'list': {
      const visible = node.items.filter((item) => !isEmptyNode(item));
      const items = (
        <Stack gap="sm">
          {visible.map((item, index) => (
            <AccessibilityNodeView
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
      );
      if (visible.length <= INLINE_ITEM_THRESHOLD) return items;
      return (
        <Disclosure label={describeCount(visible.length, label)} qualifier={path}>
          {items}
        </Disclosure>
      );
    }

    case 'link': {
      // review §3.5.9: an anchor whose visible text is just its own href
      // ("https://www.nationalrail.co.uk/...") reads as noise, not a
      // destination -- swap in the host, keep the full URL reachable via
      // `title`. Only when the two are exactly identical, so an
      // intentionally different link text (a station name, "click here")
      // is left alone.
      const raw = node.text.trim() === node.href.trim();
      const host = raw ? hostLabel(node.href) : null;
      return (
        <TextLink
          href={node.href}
          underline="always"
          title={host ? node.href : undefined}
          target={node.external ? '_blank' : undefined}
          rel={node.external ? 'noopener noreferrer' : undefined}
        >
          {host ?? node.text}
        </TextLink>
      );
    }

    case 'raw':
      return (
        <Disclosure label="Raw data" qualifier={path}>
          <Code block>{node.json}</Code>
        </Disclosure>
      );
  }
}

/** A facility record that is simply "not available" and has nothing
 * further to say about it -- no location, no notes, no contact, no
 * opening hours. `isEmptyNode` never treats a facility as empty (the
 * availability line is content in itself), so this is a separate,
 * narrower check: only these trivial negatives are candidates for
 * §3.5.6's fold. */
function isTrivialNegativeFacility(node: AccessibilityNode): boolean {
  return node.kind === 'facility' && !node.available && node.parts.length === 0;
}

interface RenderableEntry {
  key: keyof StationAccessibilityData;
  node: AccessibilityNode;
}

interface RenderableGroup {
  heading: string;
  entries: RenderableEntry[];
  // Trivially-unavailable facility keys, humanized, folded into one
  // dimmed line instead of each claiming a full icon+bold-label row
  // (review §3.5.6).
  notAvailable: string[];
}

/** Every category group that has at least one key rendering to something,
 * or at least one folded "not available" fact. Computed before any JSX so
 * the section can tell "present, and here it is" from "present, but every
 * value the feed published was empty" -- the latter reads as the same fact
 * as a `200 {}` and gets the same sentence, rather than a heading with
 * nothing under it. */
function renderableGroups(data: StationAccessibilityData): RenderableGroup[] {
  // Shared across every group and key, in this fixed top-to-bottom order --
  // see `dedupeAcrossSection`'s own doc comment for why order determines
  // which copy of a repeated fact survives.
  const seen = new Set<string>();

  return ACCESSIBILITY_CATEGORIES.map((category) => {
    // A key whose value renders to nothing at all (an empty object, an
    // empty array, an object whose every own value was null) is skipped
    // outright rather than printing a label with blank space under it --
    // Decision 7's "don't invent a row for data that isn't there", one
    // level below the group it applies to. Load-bearing: `null` is
    // pervasive in this feed (design §2.5).
    const rawEntries: RenderableEntry[] = category.keys
      .filter((key) => hasRenderableValue(data[key]))
      .map((key) => ({ key, node: renderAccessibilityValue(data[key], key) }))
      .map((entry) => ({ key: entry.key, node: dedupeAcrossSection(entry.node, seen) }))
      .filter((entry) => !isEmptyNode(entry.node));

    // Folding only kicks in for two or more -- a single "not available"
    // facility reads better with its own icon and word than as a
    // one-item dimmed list.
    const trivialNegative = rawEntries.filter((entry) => isTrivialNegativeFacility(entry.node));
    const fold = trivialNegative.length >= 2;
    const entries = fold
      ? rawEntries.filter((entry) => !isTrivialNegativeFacility(entry.node))
      : rawEntries;

    return {
      heading: category.heading,
      entries: sortEntriesByKind(entries),
      notAvailable: fold ? trivialNegative.map((entry) => humanizeKey(entry.key)) : [],
    };
  }).filter((group) => group.entries.length > 0 || group.notAvailable.length > 0);
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
/** review §3.5.3's at-a-glance strip, rendered as a row of chips above the
 * full section: the handful of facts a reader most often scrolls all
 * 4,500px to find, surfaced without asking them to. Purely additive -- the
 * full section below still carries every fact in full, this is a shortcut
 * to the ones most worth one. */
function AtAGlanceStrip({ facts }: { facts: AtAGlanceFact[] }) {
  if (facts.length === 0) return null;
  return (
    <Group gap="xs" wrap="wrap" role="list" aria-label="At a glance">
      {facts.map((fact) => (
        <Badge key={fact.label} variant="light" color="gray" tt="none" fw={500} size="lg" role="listitem">
          {fact.label}: {fact.value}
        </Badge>
      ))}
    </Group>
  );
}

export function StationAccessibilitySection({ result }: StationAccessibilitySectionProps) {
  // Memoized because it is not free: classifying a whole payload runs the
  // sanitizer over every markup-bearing string (693 of them across the 31
  // surveyed stations), which measures at 12-36 ms per station. That is
  // paid once on the server and once more on hydration; it must not also be
  // paid on every unrelated re-render of this client component.
  const groups = useMemo(
    () => (result.coverage === 'present' ? renderableGroups(result.data) : []),
    [result],
  );
  const atAGlance = useMemo(
    () => (result.coverage === 'present' ? computeAtAGlance(result.data) : []),
    [result],
  );
  // A `200` whose every allowlisted value turned out to be `{}`/`[]` is the
  // same fact as a `200 {}` from the reader's point of view -- the station
  // has published nothing -- so it gets the same sentence rather than an
  // empty section under a heading.
  const nothingPublished =
    result.coverage === 'empty' || (result.coverage === 'present' && groups.length === 0);

  return (
    // review §3.5.10: this section's prose (rich-text notes, opening-hours
    // lists) previously ran the whole ~1,100px page container, well past
    // the ~50-75 characters a line reads comfortably at. 70ch applies to
    // the section as a whole rather than per text node -- every child here
    // is a single text column already, so one wrapper does the job the
    // design asks for without touching every `Text`/`Typography` site
    // individually.
    <Stack gap="xs" style={{ maxWidth: '70ch' }}>
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
      <AtAGlanceStrip facts={atAGlance} />
      {groups.map((group) => (
        <Stack key={group.heading} gap={4}>
          {/* review §3.5.2: promoted from a bold `<p>` to a real heading --
              four of these under the section's own `h2`, twelve (well,
              fewer now several keys are grouped rather than each claiming
              its own line -- see the `entry.node.kind !== 'facility'`
              guard below) `h4` key labels under each. `size="sm"` holds the
              same visual weight the bold paragraph had; only the semantics
              change, which is the whole point of an `h2`->`h3` step
              existing for axe's `heading-order` to reward. */}
          <Title order={3} size="sm" fw={700}>
            {group.heading}
          </Title>
          {group.entries.map((entry) => (
            <Stack key={entry.key} gap={4} pl="sm">
              {/* A Pattern A key says its own name on its availability line
                  ("Lifts — Available"), so a separate label above it would
                  print the name twice. */}
              {entry.node.kind !== 'facility' && (
                <Title order={4} size="sm" fw={500}>
                  {humanizeKey(entry.key)}
                </Title>
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
          {/* review §3.5.6: several simply-unavailable facilities folded
              into one dimmed line instead of each claiming a full
              icon+bold-label row of its own. */}
          {group.notAvailable.length > 0 && (
            <Text size="sm" c="dimmed" pl="sm">
              Not available: {group.notAvailable.join(', ')}
            </Text>
          )}
        </Stack>
      ))}
    </Stack>
  );
}
