'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { Autocomplete, TextInput, TagsInput, Button, Stack, Group, Badge, CloseButton, Text, Collapse, Pill } from '@mantine/core';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import { useNeedsLogin } from '@/components/useNeedsLogin';
import { LoginPromptModal } from '@/components/LoginPromptModal';
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
      <TextInput label="Name" value={name} onChange={(event) => setName(event.currentTarget.value)} />
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
      <Group gap="xs">
        {stations.map((crs) => (
          <Badge key={crs} rightSection={<CloseButton size="xs" onClick={() => removeStation(crs)} />}>
            {crs}
          </Badge>
        ))}
      </Group>
      <Button variant="subtle" onClick={() => setAdvancedOpen((open) => !open)}>
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
      {error && <Text c="red">{error}</Text>}
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
    </Stack>
  );
}
