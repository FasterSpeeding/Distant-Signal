'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Stack, Text, TextInput } from '@mantine/core';
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

/** The template detail page's leg editor -- §6 item 3: "edit
 * origin/destination/windows per leg." Submits the WHOLE leg list on
 * every Save (`PUT /JourneyTemplates/{id}`, a full-resource replace, not
 * a per-leg patch — see the Phase B plan's Judgment Calls 1/4), so
 * add/remove/edit are all plain client-side array operations until Save
 * is pressed; nothing is persisted mid-edit. Deliberately does NOT render
 * a day-of-week picker, match-mode chooser, or Pause toggle — see this
 * page's own scope note in the Phase B plan (Task 8).
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
