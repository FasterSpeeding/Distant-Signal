'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { TextInput, TagsInput, Button, Stack, Group, Badge, CloseButton, Text, Collapse } from '@mantine/core';

/** Posts to the same-origin `/api/*` proxy (see `app/api/[...path]/route.ts`)
 * — this is a Client Component and cannot reach the `api` service directly. */
export function CustomLineForm() {
  const router = useRouter();
  const [name, setName] = useState('');
  const [operators, setOperators] = useState<string[]>([]);
  const [stationInput, setStationInput] = useState('');
  const [stations, setStations] = useState<string[]>([]);
  const [advancedOpen, setAdvancedOpen] = useState(false);
  const [headcodePrefixes, setHeadcodePrefixes] = useState<string[]>([]);
  const [destinationCrsFilter, setDestinationCrsFilter] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [submitting, setSubmitting] = useState(false);

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
      const response = await fetch('/api/lines', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ name, operators, stations, headcodePrefixes, destinationCrsFilter }),
      });
      if (!response.ok) {
        const message = await response.text();
        setError(message || `Request failed: ${response.status}`);
        setSubmitting(false);
        return;
      }
      router.push('/lines');
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <Stack gap="sm" maw={480}>
      <TextInput label="Name" value={name} onChange={(event) => setName(event.currentTarget.value)} />
      <TagsInput label="Operators" placeholder="e.g. SW" value={operators} onChange={setOperators} />
      <Group align="end">
        <TextInput
          label="Add station (CRS code)"
          placeholder="e.g. WOK"
          value={stationInput}
          onChange={(event) => setStationInput(event.currentTarget.value)}
          maxLength={3}
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
          <TagsInput label="Destination CRS filter" placeholder="e.g. AON" value={destinationCrsFilter} onChange={setDestinationCrsFilter} />
        </Stack>
      </Collapse>
      {error && <Text c="red">{error}</Text>}
      <Button onClick={handleSubmit} loading={submitting}>
        Create line
      </Button>
    </Stack>
  );
}
