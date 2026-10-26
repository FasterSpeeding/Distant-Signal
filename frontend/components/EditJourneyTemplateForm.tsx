'use client';

import { useEffect, useEffectEvent, useRef, useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Chip, Group, SegmentedControl, Stack, Switch, Text, TextInput } from '@mantine/core';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginControl } from './LoginControl';
import { TimeFilterInput } from './TimeFilterInput';
import type { JourneyTemplateDetail, PutJourneyTemplateRequest, TemplateLegRequest } from '@/lib/types';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import { resolveStationCode } from '@/lib/suggestionAutocomplete';
import { StationField } from './StationField';

interface EditableLeg {
  // A stable, position-independent identity for this leg -- used for React
  // `key`s and for tracking per-leg incomplete-time state (see
  // `incompleteByLeg` below). Deliberately NOT the array index: this form
  // lets a leg be removed from the middle of the list (`removeLeg`), and an
  // index-keyed identity would silently re-point a later leg's still-open
  // `TimeFilterInput` incomplete-state onto whatever leg slides into its old
  // slot. A pre-existing leg keeps its real backend id (`server-{id}`,
  // stable across the whole edit session); a leg added client-side via
  // `addLeg` gets a monotonic counter value instead, since it has no
  // backend id yet.
  key: string;
  // What each station field shows: a code, or a station name typed into
  // the search.
  originCrs: string;
  destinationCrs: string;
  // The CRS each station field's text stands for (`resolveStationCode`),
  // or `null` while it stands for none. Reported by that leg's
  // `LegStationField` as its text changes; this, not the text, is
  // what Save sends.
  originResolved: string | null;
  destinationResolved: string | null;
  departFrom: string;
  departTo: string;
  arriveFrom: string;
  arriveTo: string;
}

function toEditableLeg(leg: JourneyTemplateDetail['legs'][number]): EditableLeg {
  return {
    key: `server-${leg.id}`,
    originCrs: leg.originCrs ?? '',
    destinationCrs: leg.destinationCrs ?? '',
    // A saved leg's fields hold its codes, which resolve without a search.
    originResolved: resolveStationCode(leg.originCrs ?? '', []),
    destinationResolved: resolveStationCode(leg.destinationCrs ?? '', []),
    departFrom: leg.departAfter ?? '',
    departTo: leg.departBefore ?? '',
    arriveFrom: leg.arriveAfter ?? '',
    arriveTo: leg.arriveBefore ?? '',
  };
}

/** Whether each of a single leg's four `TimeFilterInput`s is currently
 * mid-entry (e.g. an hour typed with no minutes yet) -- see that
 * component's own doc comment on why a half-typed time reports `value` as
 * `''`, indistinguishable from untouched, unless the owner also listens for
 * this. Mirrors `AddJourneyLegButton.tsx`'s own `incompleteTimes` shape,
 * just one instance per leg instead of one for the whole form. */
interface LegIncompleteFlags {
  departFrom: boolean;
  departTo: boolean;
  arriveFrom: boolean;
  arriveTo: boolean;
}

const NO_INCOMPLETE_TIMES: LegIncompleteFlags = {
  departFrom: false,
  departTo: false,
  arriveFrom: false,
  arriveTo: false,
};

/** One leg's From or To field: the site's standard `StationField`, so a
 * station can be found by its code or its name and shows its name once
 * picked. Reports the code it holds (`resolveStationCode`) through
 * `onResolvedChange`, since the template API takes codes: a picked
 * suggestion, a typed code, or a typed full name the field matched on
 * blur. */
function LegStationField({
  label,
  placeholder,
  value,
  onChange,
  onResolvedChange,
}: {
  label: string;
  placeholder: string;
  value: string;
  onChange: (value: string) => void;
  onResolvedChange: (code: string | null) => void;
}) {
  // The "not a station" error waits for the field to lose focus: while
  // someone is still typing a name, it isn't a code yet, and that's fine.
  const [touched, setTouched] = useState(false);
  const resolved = resolveStationCode(value, []);
  const reportResolved = useEffectEvent((code: string | null) => onResolvedChange(code));
  useEffect(() => {
    reportResolved(resolved);
  }, [resolved]);

  const error = touched && value.trim().length > 0 && resolved === null ? 'Must be a 3-letter station code' : null;

  return (
    <StationField
      label={label}
      placeholder={placeholder}
      value={value}
      onChange={onChange}
      onBlur={() => setTouched(true)}
      error={error}
    />
  );
}

/** Mon=bit 0 (value 1) .. Sun=bit 6 (value 64), mirroring the backend's
 * own `weekday_bit`/`due_templates_for` convention
 * (`chrono::Weekday::num_days_from_monday()`-based). */
const DAYS: { key: string; label: string; bit: number }[] = [
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
  // Per-leg incomplete-time tracking, keyed by `EditableLeg.key` (not array
  // index -- see that field's own doc comment). Absent entries mean "no
  // incomplete field reported yet", equivalent to `NO_INCOMPLETE_TIMES`.
  const [incompleteByLeg, setIncompleteByLeg] = useState<Record<string, LegIncompleteFlags>>({});
  const [selectedDays, setSelectedDays] = useState<string[]>(daysOfWeekToKeys(template.daysOfWeek));
  const [matchMode, setMatchMode] = useState<'manual' | 'auto'>(template.defaultMatchMode);
  const [paused, setPaused] = useState(!template.active);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState(false);
  const needsLoginState = useNeedsLogin();
  // Monotonic source of client-side-only leg keys (`addLeg` below) -- see
  // `EditableLeg.key`'s own doc comment for why these can't just be the
  // leg's array index.
  const nextClientLegId = useRef(0);

  function updateLeg(key: string, patch: Partial<EditableLeg>) {
    setSaved(false);
    setLegs((current) => current.map((leg) => (leg.key === key ? { ...leg, ...patch } : leg)));
  }

  /** Records what a station field's text resolves to. Not an edit, so it
   * leaves "Saved." alone, and it keeps the same `legs` array when nothing
   * changed so a field reporting its unchanged code doesn't re-render. */
  function setLegResolved(key: string, field: 'originResolved' | 'destinationResolved', code: string | null) {
    setLegs((current) =>
      current.some((leg) => leg.key === key && leg[field] !== code)
        ? current.map((leg) => (leg.key === key ? { ...leg, [field]: code } : leg))
        : current,
    );
  }

  function setLegIncomplete(key: string, field: keyof LegIncompleteFlags, value: boolean) {
    setIncompleteByLeg((current) => ({
      ...current,
      [key]: { ...(current[key] ?? NO_INCOMPLETE_TIMES), [field]: value },
    }));
  }

  function addLeg() {
    setSaved(false);
    const key = `client-${nextClientLegId.current++}`;
    setLegs((current) => [
      ...current,
      {
        key,
        originCrs: '',
        destinationCrs: '',
        originResolved: null,
        destinationResolved: null,
        departFrom: '',
        departTo: '',
        arriveFrom: '',
        arriveTo: '',
      },
    ]);
  }

  function removeLeg(key: string) {
    setSaved(false);
    setLegs((current) => current.filter((leg) => leg.key !== key));
    // Drop the removed leg's own incomplete-time tracking along with it,
    // rather than leaving it to be reassigned to whichever leg now sits in
    // its old slot -- the whole point of keying this map by a stable `key`
    // instead of position.
    setIncompleteByLeg((current) => {
      const { [key]: _removed, ...next } = current;
      return next;
    });
  }

  // A half-typed time in any leg (e.g. an hour with no minutes) blocks Save
  // exactly like `AddJourneyLegButton.tsx`'s own `windowTimesComplete` --
  // see that component and `TimeFilterInput`'s own doc comment for why this
  // signal exists at all. Without it, a half-entered bound would silently
  // report `value` as `''` and PUT as `null`, with the page showing a green
  // "Saved." and no indication the bound was ever dropped.
  const allTimesComplete = legs.every((leg) => {
    const flags = incompleteByLeg[leg.key];
    return !flags || !Object.values(flags).some(Boolean);
  });

  const isValid =
    legs.length > 0 &&
    legs.every((leg) => leg.originResolved !== null && leg.destinationResolved !== null) &&
    allTimesComplete;

  async function handleSave() {
    if (!isValid) return;
    setSubmitting(true);
    setError(null);
    setSaved(false);
    needsLoginState.reset();
    try {
      const body: PutJourneyTemplateRequest = {
        ...(customName.trim() ? { customName: customName.trim() } : {}),
        legs: legs.map((leg): TemplateLegRequest => ({
          // `isValid` has already checked both resolve.
          originCrs: leg.originResolved ?? '',
          destinationCrs: leg.destinationResolved ?? '',
          departWindow: { after: leg.departFrom || null, before: leg.departTo || null },
          arriveWindow: { after: leg.arriveFrom || null, before: leg.arriveTo || null },
        })),
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
          setError(await failureFromResponse('save', 'this template', response));
        }
        setSubmitting(false);
        return;
      }
      setSubmitting(false);
      setSaved(true);
      router.refresh();
    } catch {
      setError(describeFailure('save', 'this template'));
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
            setMatchMode(value);
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
        <Stack key={leg.key} gap="xs" p="sm" className="templateLeg">
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
                onClick={() => removeLeg(leg.key)}
              >
                Remove
              </Button>
            )}
          </Group>
          <Group grow align="flex-start">
            <LegStationField
              label="From station"
              placeholder="e.g. Woking or WOK"
              value={leg.originCrs}
              onChange={(value) => updateLeg(leg.key, { originCrs: value })}
              onResolvedChange={(code) => setLegResolved(leg.key, 'originResolved', code)}
            />
            <LegStationField
              label="To station"
              placeholder="e.g. London Waterloo or WAT"
              value={leg.destinationCrs}
              onChange={(value) => updateLeg(leg.key, { destinationCrs: value })}
              onResolvedChange={(code) => setLegResolved(leg.key, 'destinationResolved', code)}
            />
          </Group>
          <Group grow align="flex-start">
            <TimeFilterInput
              clockHint
              label="Earliest departure (optional)"
              name={`leg-${index}-depart-from`}
              description="Only trains leaving at or after this time."
              value={leg.departFrom}
              onChange={(v) => updateLeg(leg.key, { departFrom: v })}
              onIncompleteChange={(v) => setLegIncomplete(leg.key, 'departFrom', v)}
              error={null}
            />
            <TimeFilterInput
              label="Latest departure (optional)"
              name={`leg-${index}-depart-to`}
              description="Only trains leaving at or before this time."
              value={leg.departTo}
              onChange={(v) => updateLeg(leg.key, { departTo: v })}
              onIncompleteChange={(v) => setLegIncomplete(leg.key, 'departTo', v)}
              error={null}
            />
          </Group>
          <Group grow align="flex-start">
            <TimeFilterInput
              label="Earliest arrival (optional)"
              name={`leg-${index}-arrive-from`}
              description="Only trains reaching the destination at or after this time."
              value={leg.arriveFrom}
              onChange={(v) => updateLeg(leg.key, { arriveFrom: v })}
              onIncompleteChange={(v) => setLegIncomplete(leg.key, 'arriveFrom', v)}
              error={null}
            />
            <TimeFilterInput
              label="Latest arrival (optional)"
              name={`leg-${index}-arrive-to`}
              description="Only trains reaching the destination at or before this time."
              value={leg.arriveTo}
              onChange={(v) => updateLeg(leg.key, { arriveTo: v })}
              onIncompleteChange={(v) => setLegIncomplete(leg.key, 'arriveTo', v)}
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
      {needsLoginState.needsLogin && <LoginControl underline="always">Log in to save changes</LoginControl>}
      <Button onClick={handleSave} disabled={!isValid} loading={submitting}>
        Save changes
      </Button>
    </Stack>
  );
}
