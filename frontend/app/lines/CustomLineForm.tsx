'use client';

import { useEffect, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Autocomplete, TextInput, TagsInput, Button, Stack, Group, Badge, CloseButton, Text, Collapse, Pill } from '@mantine/core';
import { searchStations, searchTocs } from '@/lib/suggestions';
import { useSuggestions } from '@/lib/useSuggestions';
import type { CustomLineDetail } from '@/lib/types';

/** Posts to the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly.
 * With `existingLine` set, edits that line via PUT instead of creating a
 * new one via POST. */
export function CustomLineForm({ existingLine }: { existingLine?: CustomLineDetail }) {
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
    const crs = stationInput.trim().toUpperCase();
    if (crs.length !== 3 || stations.includes(crs)) return;
    setStations([...stations, crs]);
    setStationInput('');
  }

  function removeStation(crs: string) {
    setStations(stations.filter((s) => s !== crs));
  }

  async function handleSubmit() {
    setError(null);
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
        const message = await response.text();
        setError(message || `Request failed: ${response.status}`);
        setSubmitting(false);
        return;
      }
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
          placeholder="e.g. WOK"
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
          renderOption={({ option }) => {
            const match = stationSuggestions.find((s) => s.code === option.value);
            return match ? `${match.code} — ${match.name}` : option.value;
          }}
        />
        <Button variant="outline" onClick={addStation} disabled={stationInput.trim().length !== 3}>
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
      <Button onClick={handleSubmit} loading={submitting}>
        {existingLine ? 'Save changes' : 'Create line'}
      </Button>
    </Stack>
  );
}
