'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { Alert, Autocomplete, TextInput, TagsInput, Button, Stack, Group, Badge, CloseButton, Text, Collapse, Pill } from '@mantine/core';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { useNeedsLogin } from '@/components/useNeedsLogin';
import { LoginPromptModal } from '@/components/LoginPromptModal';
import { DeleteLineButton } from '@/components/DeleteLineButton';
import type { CustomLineDetail } from '@/lib/types';

/** Posts to the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly.
 * With `existingLine` set, edits that line via PUT instead of creating a
 * new one via POST. `cancelHref` opts into a Cancel action rendered beside
 * the submit button; without it the submit button keeps the Stack's full
 * width, which is what the create-line page wants.
 *
 * `create_line`/`update_line` both require `AuthenticatedUser`
 * (`crates/api/src/routes/lines.rs`), and `/lines/[id]/page.tsx` now only
 * links to this form's edit mode for the line's real owner (see that
 * page's `isOwner` gate). So a `401` here can, in practice, only happen
 * from a session that lapses between page load and this submit — the same
 * narrow race `TicketPanel`'s design already reasoned about (Decision 4,
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md).
 * Matches `PinToggle`'s established `needsLogin` pattern: catch the `401`
 * specifically and show a login prompt, never the raw backend rejection
 * text (`"no session"`) this used to fall through to. */
export function CustomLineForm({ existingLine, cancelHref }: { existingLine?: CustomLineDetail; cancelHref?: string }) {
  const router = useRouter();
  const [name, setName] = useState(existingLine?.name ?? '');
  const [operators, setOperators] = useState<string[]>(existingLine?.operators ?? []);
  const [stationInput, setStationInput] = useState('');
  const [stations, setStations] = useState<string[]>(existingLine?.stations ?? []);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [headcodePrefixes, setHeadcodePrefixes] = useState<string[]>(existingLine?.headcodePrefixes ?? []);
  const [destinationCrsFilter, setDestinationCrsFilter] = useState<string[]>(existingLine?.destinationCrsFilter ?? []);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const needsLoginState = useNeedsLogin();

  const [operatorsQuery, setOperatorsQuery] = useState('');
  const { suggestions: operatorSuggestions } = useSuggestions(operatorsQuery, searchTocs);

  const { suggestions: stationSuggestions } = useSuggestions(stationInput, searchStations);

  const [destinationQuery, setDestinationQuery] = useState('');
  const { suggestions: destinationSuggestions } = useSuggestions(destinationQuery, searchStations);

  // Committed tags only carry a code (`operators`/`destinationCrsFilter`
  // are `string[]`), so once a suggestion "scrolls out" of the current
  // search results there's nowhere left to look up its name from — this
  // cache remembers every code/name pair ever seen across all three
  // suggestion sources (CRS and ATOC codes don't collide) so a pill's
  // title tooltip keeps working long after the dropdown that produced it
  // is gone.
  const [nameByCode, setNameByCode] = useState<Record<string, string>>({});
  useEffect(() => {
    setNameByCode((prev) => {
      const next = { ...prev };
      for (const s of [...operatorSuggestions, ...stationSuggestions, ...destinationSuggestions]) {
        next[s.code] = s.name;
      }
      return next;
    });
  }, [operatorSuggestions, stationSuggestions, destinationSuggestions]);

  function addStation() {
    const trimmed = stationInput.trim();
    if (!trimmed) return;
    // Resolve the typed text the same way selecting a dropdown suggestion
    // would: an exact code or name match first, then the best (first)
    // substring match already returned by the server, and only fall back
    // to the raw text uppercased if nothing matched at all (e.g. a network
    // hiccup) -- mirrors `StationSearchForm`'s "Look up" resolution, so
    // clicking Add after typing a station name (without picking the
    // dropdown option) still resolves to the right CRS code.
    const exactCode = stationSuggestions.find((s) => s.code.toLowerCase() === trimmed.toLowerCase());
    const exactName = stationSuggestions.find((s) => s.name.toLowerCase() === trimmed.toLowerCase());
    const crs = exactCode?.code ?? exactName?.code ?? stationSuggestions[0]?.code ?? trimmed.toUpperCase();
    // The usual dedup/length gate still applies to whatever the above
    // resolved to, whether that came from a suggestion or the raw
    // fallback -- the autocomplete only changes how `crs` is derived, not
    // what counts as a valid one.
    if (crs.length !== 3 || stations.includes(crs)) return;
    setStations([...stations, crs]);
    setStationInput('');
  }

  function removeStation(crs: string) {
    setStations(stations.filter((s) => s !== crs));
  }

  async function handleSubmit() {
    setError(null);
    needsLoginState.reset();
    if (name.trim().length === 0) {
      setError('Name is required.');
      return;
    }
    if (stations.length < 2) {
      setError('Add at least 2 stations.');
      return;
    }
    setSubmitting(true);
    try {
      const url = existingLine ? `/api/lines/${existingLine.id}` : '/api/lines';
      const method = existingLine ? 'PUT' : 'POST';
      const response = await fetch(url, {
        method,
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name, operators, stations, headcodePrefixes, destinationCrsFilter }),
      });
      if (!response.ok) {
        // A 401's body is the backend's plain-text rejection ("no
        // session") -- never shown to the user as-is (see this
        // component's own doc comment). Every other non-ok status still
        // falls through to the generic error text, unchanged from before.
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      // Both create and edit now navigate to a route different from
      // wherever this form is rendered (`/lines/new` for create,
      // `/lines/{id}/edit` for edit) -- App Router remounts this
      // component on the way there either way, so `submitting` and every
      // field reset for free, with no manual work needed here.
      router.push(existingLine ? `/lines/${existingLine.id}` : '/lines');
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="sm" maw={480}>
      {/* Task 3.4.3: neither the create nor the edit page said what a
          custom line actually is -- rendered here, once, rather than
          duplicated across `app/lines/new/page.tsx` and
          `app/lines/[id]/edit/page.tsx`, since both mount this form
          immediately below their own `<h1>` with nothing in between. */}
      <Text size="sm" c="dimmed">
        A custom line groups any stations and operators you choose into one line you can track status for — it&apos;s
        private to you, and appears in your own All Lines table.
      </Text>
      {/* Task 3.4.13: create-only -- an owner reaching the edit form is
          already signed in (the route 404s a non-owner before this ever
          renders), so the hint would be both pointless and misleading
          there. Deliberately does NOT promise the in-progress entry
          survives the trip through the OIDC login redirect: verified
          against this component (no sessionStorage/localStorage anywhere
          in it) that it does not, the same finding Task 1.15 made for its
          own page -- an honest "you'll be sent to log in" beats a false
          "your entries are kept". This also replaces the near-identical
          plain `Text` `app/lines/new/page.tsx` used to render itself,
          upgraded to an `Alert` for more visual weight per the review, not
          duplicated alongside it. */}
      {!existingLine && (
        <Alert color="blue" variant="light">
          Creating a line needs a Distant Signal account — you&apos;ll be sent to log in when you save if you
          aren&apos;t already signed in.
        </Alert>
      )}
      <TextInput label="Name" withAsterisk value={name} onChange={(event) => setName(event.currentTarget.value)} />
      <TagsInput
        label="Operators"
        placeholder="e.g. SW"
        value={operators}
        onChange={setOperators}
        onSearchChange={setOperatorsQuery}
        data={operatorSuggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
        renderPill={({ option, onRemove }) => (
          <Pill withRemoveButton onRemove={onRemove} title={nameByCode[String(option.value)]}>
            {option.value}
          </Pill>
        )}
      />
      <Group align="end">
        <Autocomplete
          label="Add station (CRS code)"
          placeholder="e.g. Woking or WOK"
          value={stationInput}
          onChange={setStationInput}
          // `data`'s `label` — not `value` — is what Mantine's Autocomplete
          // writes into the field on selection (confirmed by reading its
          // source: `handleValueChange(optionsLockup[val].label)`), the
          // opposite of TagsInput below. So `label` is set to the code
          // itself here, and the friendlier "code — name" text is rendered
          // dropdown-only via `renderOption`, which doesn't affect what
          // gets written into the field.
          data={stationSuggestions.map((s) => ({ value: s.code, label: s.code }))}
          // `stationSuggestions` is already server-side filtered (the API
          // matches the search term against both CRS code and station
          // name), so Mantine's default client-side re-filtering -- which
          // only checks `label` (the code) -- would hide correct matches
          // when the user searched by station name instead of code.
          // Disable it: show whatever `stationSuggestions` already
          // contains, unfiltered further. Same fix as `StationSearchForm`.
          filter={({ options }) => options}
          renderOption={({ option }) => {
            const match = stationSuggestions.find((s) => s.code === option.value);
            return match ? `${match.code} — ${match.name}` : option.value;
          }}
        />
        {/* Not gated on `.length === 3` any more -- a typed station name
         * (e.g. "Woking") is longer than 3 characters but still resolves
         * to a valid code inside `addStation`, which remains the actual
         * validation gate. */}
        <Button variant="outline" onClick={addStation} disabled={stationInput.trim().length === 0}>
          Add
        </Button>
      </Group>
      {/* Task 3.4.4: this list had neither an empty state nor any
          indication that order matters (it's travel order, per
          DESIGN.md §5.1's ordered-list domain model) -- an empty
          `Group` gave no feedback at all before the first station was
          added, and once a couple were added there was nothing to show
          they were sequential rather than an unordered set. Numbering the
          chips inline (rather than a full drag-reorderable vertical list)
          is the smaller of the two fixes the review names; reordering
          stays out of scope here. */}
      {stations.length === 0 ? (
        <Text size="sm" c="dimmed">
          No stations yet — add at least two, in travel order.
        </Text>
      ) : (
        <Group gap="xs">
          {stations.map((crs, index) => (
            <Badge
              key={crs}
              title={nameByCode[crs]}
              rightSection={
              /* `aria-label` is not optional here: Mantine's `CloseButton`
                 renders a bare `<button>` around an SVG with no text and no
                 name of its own, so axe's `button-name` fires (critical) --
                 once per station chip, which on a prefilled
                 `/lines/[id]/edit` is every chip on the page. Naming the
                 station it removes (rather than a generic "Remove") is what
                 makes a list of these distinguishable when tabbed through
                 or listed by a screen reader; `nameByCode` is preferred
                 over the bare CRS for the same reason the `title` above
                 uses it, and falls back to the code before the station
                 lookup resolves. */
              <CloseButton
                size="xs"
                c="white"
                aria-label={`Remove ${nameByCode[crs] ?? crs}`}
                onClick={() => removeStation(crs)}
              />
              }
            >
              {index + 1} {crs}
            </Badge>
          ))}
        </Group>
      )}
      <Button
        // `variant="transparent"`, not `"subtle"`: `"subtle"` paints a
        // `--mantine-color-grape-light-hover` background (grape 1,
        // `#eebefa`) under the pointer/on focus, which Mantine designed to
        // pair with `"subtle"`'s OWN default text colour
        // (`--mantine-color-grape-light-color`) -- not with the
        // `--mantine-color-anchor` override below, which this button needs
        // for its resting, transparent-background state (see that override's
        // own comment). Grape 7 anchor text on that grape-1 hover background
        // is only 3.08:1, short of AA's 4.5:1 -- reproduced live: click the
        // button (which leaves a real mouse pointer hovering over it,
        // exactly like clicking with an actual mouse), and axe flags the
        // hovered/focused state, not the resting one, which is why this
        // slipped past a scan of the page's initial render. `"transparent"`
        // keeps the background transparent in every state, so the anchor
        // colour is always measured against the page background it was
        // actually chosen for.
        variant="transparent"
        // Task 3.4.12: Mantine `Button variant="subtle"`'s default text
        // colour measured near-white on this app's near-white dark
        // background -- effectively invisible as a link. Pinned to the
        // same `--mantine-color-anchor` token every ordinary text link in
        // this app already uses (grape 7 / 4.85:1 in light, grape 4 /
        // 5.70:1 in dark -- see app/globals.css's own anchor-contrast
        // comment), so both the "Show" and "Hide" states of this one
        // `Button` -- there is only ever one, never two differently
        // coloured elements -- read as the same, correctly-contrasted
        // affordance in both colour schemes.
        c="var(--mantine-color-anchor)"
        onClick={() => setAdvancedOpen((open) => !open)}
      >
        {advancedOpen ? 'Hide' : 'Show'} advanced options
      </Button>
      <Collapse expanded={advancedOpen}>
        <Stack gap="sm">
          <TagsInput label="Headcode prefixes" placeholder="e.g. 1P" value={headcodePrefixes} onChange={setHeadcodePrefixes} />
          <TagsInput
            label="Destination CRS filter"
            placeholder="e.g. AON"
            value={destinationCrsFilter}
            onChange={setDestinationCrsFilter}
            onSearchChange={setDestinationQuery}
            data={destinationSuggestions.map((s) => ({ value: s.code, label: `${s.code} — ${s.name}` }))}
            renderPill={({ option, onRemove }) => (
              <Pill withRemoveButton onRemove={onRemove} title={nameByCode[String(option.value)]}>
                {option.value}
              </Pill>
            )}
          />
        </Stack>
      </Collapse>
      {error && <Text c="var(--ds-color-error-text)">{error}</Text>}
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to {existingLine ? 'edit' : 'create'} a custom line.
      </LoginPromptModal>
      {cancelHref ? (
        // Paired actions sit on one right-aligned row so the secondary
        // reads as a peer of the primary rather than an afterthought
        // beneath a 480px-wide button. Plain `<Link>` wrapping `Button`,
        // not `component={Link}` on a Mantine polymorphic prop — that
        // pattern previously broke `next build`'s Server/Client boundary
        // check (see the comment in `app/layout.tsx`). `type="button"`
        // keeps Cancel inert should this ever be wrapped in a real
        // `<form>`.
        <Group justify="flex-end">
          <Link href={cancelHref} style={{ textDecoration: 'none' }}>
            <Button type="button" variant="default">
              Cancel
            </Button>
          </Link>
          <Button onClick={handleSubmit} loading={submitting}>
            {existingLine ? 'Save changes' : 'Create line'}
          </Button>
        </Group>
      ) : (
        <Button onClick={handleSubmit} loading={submitting}>
          {existingLine ? 'Save changes' : 'Create line'}
        </Button>
      )}
      {/* Task 3.4.11: the edit page had no way to delete a line at all --
          only the detail page's own heading-row `Button` did. Reuses that
          same component/modal/`handleDelete` (rather than a second,
          separately-implemented delete flow), rendered as a quieter text
          link appropriate to a form's footer. `existingLine` gates it
          precisely because there is nothing to delete yet while creating,
          and `CustomLineForm` is otherwise the one file both `/lines/new`
          and `/lines/[id]/edit` share. */}
      {existingLine && (
        <Group justify="center">
          <DeleteLineButton id={existingLine.id} trigger="link" />
        </Group>
      )}
    </Stack>
  );
}
