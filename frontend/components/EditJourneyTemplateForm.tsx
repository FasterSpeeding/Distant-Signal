'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Chip, Group, SegmentedControl, Stack, Switch, Text, TextInput } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginLink } from './LoginLink';
import { TimeFilterInput } from './TimeFilterInput';
import type { JourneyTemplateDetail, PutJourneyTemplateRequest, TemplateLegRequest } from '@/lib/types';

interface EditableLeg {
  originCrs: string;
  destinationCrs: string;
  departFrom: string;
  departTo: string;
  arriveFrom: string;
  arriveTo: string;
}

function toEditableLeg(leg: JourneyTemplateDetail['legs'][number]): EditableLeg {
  return {
    originCrs: leg.originCrs ?? '',
    destinationCrs: leg.destinationCrs ?? '',
    departFrom: leg.departAfter ?? '',
    departTo: leg.departBefore ?? '',
    arriveFrom: leg.arriveAfter ?? '',
    arriveTo: leg.arriveBefore ?? '',
  };
}

/** Mon=bit 0 (value 1) .. Sun=bit 6 (value 64), mirroring the backend's
 * own `weekday_bit`/`due_templates_for` convention
 * (`chrono::Weekday::num_days_from_monday()`-based). */
const DAYS: Array<{ key: string; label: string; bit: number }> = [
  { key: 'mon', label: 'Mon', bit: 1 },
  { key: 'tue', label: 'Tue', bit: 2 },
  { key: 'wed', label: 'Wed', bit: 4 },
  { key: 'thu', label: 'Thu', bit: 8 },
  { key: 'fri', label: 'Fri', bit: 16 },
  { key: 'sat', label: 'Sat', bit: 32 },
  { key: 'sun', label: 'Sun', bit: 64 },
];

function daysOfWeekToKeys(mask: number | null): string[] {
  if (mask === null) return [];
  return DAYS.filter((day) => (mask & day.bit) !== 0).map((day) => day.key);
}

/** An empty selection means "not recurring" — sent as `null`, matching
 * the DB's own "NULL = one-shot template" semantics, not `0` or `[]`. */
function keysToDaysOfWeek(keys: string[]): number | null {
  if (keys.length === 0) return null;
  return keys.reduce((mask, key) => {
    const day = DAYS.find((d) => d.key === key);
    return day ? mask | day.bit : mask;
  }, 0);
}

/** The template detail page's leg editor -- §6 item 3: "edit
 * origin/destination/windows per leg." Submits the WHOLE leg list on
 * every Save (`PUT /JourneyTemplates/{id}`, a full-resource replace, not
 * a per-leg patch — see the Phase B plan's Judgment Calls 1/4), so
 * add/remove/edit are all plain client-side array operations until Save
 * is pressed; nothing is persisted mid-edit. Also renders the
 * recurrence controls (day-of-week picker, match-mode chooser, Pause
 * toggle) added in Phase C.
 *
 * The remove-leg control is a plain, text-labeled `Button`, not an
 * `ActionIcon`+icon -- `@tabler/icons-react` is not a dependency of this
 * frontend (see `InfoIcon.tsx`'s own doc comment, and
 * `RemoveJourneyLegButton.tsx`, the sibling control this one is styled
 * after, which uses the exact same plain-`Button` convention). */
export function EditJourneyTemplateForm({ template }: { template: JourneyTemplateDetail }) {
  const router = useRouter();
  const [customName, setCustomName] = useState(template.customName ?? '');
  const [legs, setLegs] = useState<EditableLeg[]>(template.legs.map(toEditableLeg));
  const [selectedDays, setSelectedDays] = useState<string[]>(daysOfWeekToKeys(template.daysOfWeek));
  const [matchMode, setMatchMode] = useState<'manual' | 'auto'>(template.defaultMatchMode);
  const [paused, setPaused] = useState(!template.active);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const needsLoginState = useNeedsLogin();

  function updateLeg(index: number, patch: Partial<EditableLeg>) {
    setSaved(false);
    setLegs((current) => current.map((leg, i) => (i === index ? { ...leg, ...patch } : leg)));
  }

  function addLeg() {
    setSaved(false);
    setLegs((current) => [
      ...current,
      { originCrs: '', destinationCrs: '', departFrom: '', departTo: '', arriveFrom: '', arriveTo: '' },
    ]);
  }

  function removeLeg(index: number) {
    setSaved(false);
    setLegs((current) => current.filter((_, i) => i !== index));
  }

  const isValid =
    legs.length > 0 && legs.every((leg) => leg.originCrs.trim() !== '' && leg.destinationCrs.trim() !== '');

  async function handleSave() {
    if (!isValid) return;
    setSubmitting(true);
    setError(null);
    setSaved(false);
    needsLoginState.reset();
    try {
      const body: PutJourneyTemplateRequest = {
        ...(customName.trim() ? { customName: customName.trim() } : {}),
        legs: legs.map(
          (leg): TemplateLegRequest => ({
            originCrs: leg.originCrs.trim(),
            destinationCrs: leg.destinationCrs.trim(),
            departWindow: { after: leg.departFrom || null, before: leg.departTo || null },
            arriveWindow: { after: leg.arriveFrom || null, before: leg.arriveTo || null },
          }),
        ),
        daysOfWeek: keysToDaysOfWeek(selectedDays),
        active: !paused,
        startsOn: template.startsOn,
        endsOn: template.endsOn,
        defaultMatchMode: matchMode,
        autoCommitRule: matchMode === 'auto' ? 'nearest_to_now' : null,
      };
      const response = await fetch(`/api/JourneyTemplates/${template.id}`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });
      if (!response.ok) {
        if (response.status === 401) {
          needsLoginState.markNeedsLogin();
        } else {
          const message = await response.text();
          setError(message || `Request failed: ${response.status}`);
        }
        setSubmitting(false);
        return;
      }
      setSubmitting(false);
      setSaved(true);
      router.refresh();
    } catch {
      setError('Request failed.');
      setSubmitting(false);
    }
  }

  return (
    <Stack>
      <TextInput
        label="Template name"
        placeholder="e.g. Weekday commute"
        value={customName}
        onChange={(event) => setCustomName(event.currentTarget.value)}
      />
      <Stack gap={4}>
        <Text size="sm" fw={500}>
          Repeats on
        </Text>
        <Chip.Group
          multiple
          value={selectedDays}
          onChange={(value) => {
            setSaved(false);
            setSelectedDays(value);
          }}
        >
          <Group gap="xs">
            {DAYS.map((day) => (
              <Chip key={day.key} value={day.key}>
                {day.label}
              </Chip>
            ))}
          </Group>
        </Chip.Group>
        <Text size="xs" c="dimmed">
          {selectedDays.length === 0 ? 'Not recurring' : 'Recurs on the selected days'}
        </Text>
      </Stack>
      <Stack gap={4}>
        <Text size="sm" fw={500}>
          When a matching train appears
        </Text>
        <SegmentedControl
          value={matchMode}
          onChange={(value) => {
            setSaved(false);
            setMatchMode(value as 'manual' | 'auto');
          }}
          data={[
            { label: "Remind me, don't guess", value: 'manual' },
            { label: 'Auto-commit for me', value: 'auto' },
          ]}
        />
      </Stack>
      <Switch
        label="Paused"
        description="A paused template won't generate new journeys until resumed."
        checked={paused}
        onChange={(event) => {
          setSaved(false);
          setPaused(event.currentTarget.checked);
        }}
      />
      {legs.map((leg, index) => (
        <Stack key={index} gap="xs" p="sm" style={{ border: '1px solid var(--mantine-color-gray-3)' }}>
          <Group justify="space-between">
            <Text size="sm" fw={600}>
              Leg {index + 1}
            </Text>
            {legs.length > 1 && (
              <Button
                variant="subtle"
                color="red"
                size="xs"
                aria-label={`Remove leg ${index + 1}`}
                onClick={() => removeLeg(index)}
              >
                Remove
              </Button>
            )}
          </Group>
          <Group grow>
            <TextInput
              label="Origin CRS"
              value={leg.originCrs}
              onChange={(event) => updateLeg(index, { originCrs: event.currentTarget.value })}
            />
            <TextInput
              label="Destination CRS"
              value={leg.destinationCrs}
              onChange={(event) => updateLeg(index, { destinationCrs: event.currentTarget.value })}
            />
          </Group>
          <Group grow align="flex-start">
            <TimeFilterInput
              label="Earliest departure (optional)"
              name={`leg-${index}-depart-from`}
              description="Only trains leaving at or after this time."
              value={leg.departFrom}
              onChange={(v) => updateLeg(index, { departFrom: v })}
              onIncompleteChange={() => {}}
              error={null}
            />
            <TimeFilterInput
              label="Latest departure (optional)"
              name={`leg-${index}-depart-to`}
              description="Only trains leaving at or before this time."
              value={leg.departTo}
              onChange={(v) => updateLeg(index, { departTo: v })}
              onIncompleteChange={() => {}}
              error={null}
            />
          </Group>
          <Group grow align="flex-start">
            <TimeFilterInput
              label="Earliest arrival (optional)"
              name={`leg-${index}-arrive-from`}
              description="Only trains reaching the destination at or after this time."
              value={leg.arriveFrom}
              onChange={(v) => updateLeg(index, { arriveFrom: v })}
              onIncompleteChange={() => {}}
              error={null}
            />
            <TimeFilterInput
              label="Latest arrival (optional)"
              name={`leg-${index}-arrive-to`}
              description="Only trains reaching the destination at or before this time."
              value={leg.arriveTo}
              onChange={(v) => updateLeg(index, { arriveTo: v })}
              onIncompleteChange={() => {}}
              error={null}
            />
          </Group>
        </Stack>
      ))}
      <Button variant="default" onClick={addLeg}>
        Add another leg
      </Button>
      {error && <Alert color="red">{error}</Alert>}
      {saved && <Alert color="green">Saved.</Alert>}
      {needsLoginState.needsLogin && <LoginLink underline="always">Log in to save changes</LoginLink>}
      <Button onClick={handleSave} disabled={!isValid} loading={submitting}>
        Save changes
      </Button>
    </Stack>
  );
}
