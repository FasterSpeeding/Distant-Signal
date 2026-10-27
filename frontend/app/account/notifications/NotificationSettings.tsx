'use client';

import { useCallback, useEffect, useState } from 'react';
import { Badge, Button, Group, Select, Stack, Switch, Text } from '@mantine/core';
import { TimeInput } from '@mantine/dates';
import { NotificationsToggle } from '@/components/NotificationsToggle';
import { CLOCK_HINT } from '@/components/TimeFilterInput';
import { describeFailure, failureFromResponse } from '@/lib/failure';
import { formatDateTime } from '@/lib/dateFormat';
import type { NotificationPreferences, NotificationPreferencesUpdate, PushDevice } from '@/lib/types';
import { PreferencesSection } from '../preferences/PreferencesSection';

/** A pinned line and whether its status changes are pushed. */
export interface PinnedLineAlert {
  id: string;
  name: string;
  alerts: boolean;
}

type Switchable = Exclude<keyof NotificationPreferences, 'delayThresholdMinutes' | 'quietHours' | 'snoozedUntil'>;

/** The switches, in the order the page lists them, with what each sends.
 * `journeySuggestions` says how it changes the train pushes. */
const SWITCHES: { key: Switchable; label: string; description: string }[] = [
  {
    key: 'trainDelay',
    label: 'Train delays',
    description: 'When a train you track is running late by at least the delay below.',
  },
  {
    key: 'trainCancel',
    label: 'Cancellations',
    description: 'When a train you track is cancelled. These come through even in quiet hours.',
  },
  {
    key: 'predictedStart',
    label: 'Expected late starts',
    description: "Before a train sets off, when the train it's formed from is running late.",
  },
  {
    key: 'stationSkip',
    label: 'Skipped stops',
    description: 'When a train on one of your journeys stops calling where you get on or off.',
  },
  {
    key: 'journeySuggestions',
    label: 'Suggest other trains when a journey breaks',
    description:
      "One notification with other ways to get there, in place of that train's delay or cancellation notification. Turn this off to get those instead.",
  },
  {
    key: 'templateAttention',
    label: 'Recurring journeys',
    description: 'When no train is found for one of your recurring journeys today.',
  },
  {
    key: 'lineStatus',
    label: 'Line status',
    description: 'When the status of a pinned line changes, for the lines you choose below.',
  },
];

const THRESHOLD_OPTIONS = [
  { value: '5', label: '5 min' },
  { value: '10', label: '10 min' },
  { value: '15', label: '15 min' },
  { value: '30', label: '30 min' },
];
/** What the api uses when the threshold is `null`. */
const DEFAULT_THRESHOLD = 15;

/** The browser's own time zone, for new quiet hours. */
function browserTimeZone(): string {
  try {
    return Intl.DateTimeFormat().resolvedOptions().timeZone || 'Europe/London';
  } catch {
    return 'Europe/London';
  }
}

/** The hex SHA-256 of this browser's push endpoint, to find it in the
 * device list (`endpointSha256`), or null with push off here. */
async function currentEndpointHash(): Promise<string | null> {
  try {
    if (!('serviceWorker' in navigator)) return null;
    const serviceWorker: Partial<Pick<ServiceWorkerContainer, 'getRegistration'>> = navigator.serviceWorker;
    const registration = await serviceWorker.getRegistration?.();
    const subscription = await registration?.pushManager.getSubscription();
    if (!subscription) return null;
    const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(subscription.endpoint));
    return Array.from(new Uint8Array(digest), (byte) => byte.toString(16).padStart(2, '0')).join('');
  } catch {
    return null;
  }
}

/** `/account/notifications`: every notification setting in one place
 * (DS-153). This device's push, a switch per kind of notification, the
 * delay threshold, quiet hours, which pinned lines alert, and the devices
 * notifications go to. Each change saves on its own; a failed save puts
 * the control back and says so. */
export function NotificationSettings({ pinnedLines }: { pinnedLines: PinnedLineAlert[] }) {
  const [settings, setSettings] = useState<NotificationPreferences | null>(null);
  const [loadError, setLoadError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [saveError, setSaveError] = useState<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    const isCancelled = () => cancelled;
    void (async () => {
      try {
        const response = await fetch('/api/preferences/notifications');
        if (isCancelled()) return;
        if (!response.ok) {
          setLoadError(describeFailure('load', 'your notification settings', response.status));
          return;
        }
        const body = (await response.json()) as NotificationPreferences;
        if (!isCancelled()) setSettings(body);
      } catch {
        if (!isCancelled()) setLoadError(describeFailure('load', 'your notification settings'));
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  /** Saves `change`, showing it at once and putting it back on failure. */
  async function save(change: NotificationPreferencesUpdate): Promise<boolean> {
    if (!settings) return false;
    const previous = settings;
    setSettings({ ...settings, ...change });
    setSaving(true);
    setSaveError(null);
    try {
      const response = await fetch('/api/preferences/notifications', {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(change),
      });
      if (!response.ok) {
        setSettings(previous);
        setSaveError(await failureFromResponse('save', 'this setting', response));
        return false;
      }
      setSettings((await response.json()) as NotificationPreferences);
      return true;
    } catch {
      setSettings(previous);
      setSaveError(describeFailure('save', 'this setting'));
      return false;
    } finally {
      setSaving(false);
    }
  }

  const [devicesVersion, setDevicesVersion] = useState(0);

  return (
    <>
      <PreferencesSection
        id="this-device-heading"
        title="This device"
        description="Notifications come to each device you turn them on for."
      >
        <Group>
          <NotificationsToggle onChange={() => setDevicesVersion((version) => version + 1)} />
        </Group>
      </PreferencesSection>

      {loadError && (
        <Text c="var(--ds-color-error-text)" role="alert">
          {loadError}
        </Text>
      )}

      <PreferencesSection
        id="what-to-send-heading"
        title="What to send"
        description="These apply on every device. Each tracked train also has its own switch on its page; a train turned off there sends nothing, suggestions included."
      >
        <Stack gap="md">
          {SWITCHES.map(({ key, label, description }) => (
            <Switch
              key={key}
              label={label}
              description={description}
              checked={settings?.[key] ?? false}
              disabled={settings === null || saving}
              onChange={(event) => void save({ [key]: event.currentTarget.checked })}
            />
          ))}
          <Select
            label="Delay threshold"
            description="How late a train must be before you hear about it."
            data={THRESHOLD_OPTIONS}
            value={String(settings?.delayThresholdMinutes ?? DEFAULT_THRESHOLD)}
            disabled={settings === null || saving || settings.trainDelay === false}
            allowDeselect={false}
            onChange={(value) => {
              if (value) void save({ delayThresholdMinutes: Number(value) as 5 | 10 | 15 | 30 });
            }}
            maw={240}
          />
          {saveError && (
            <Text size="sm" c="var(--ds-color-error-text)" role="alert">
              {saveError}
            </Text>
          )}
        </Stack>
      </PreferencesSection>

      <PauseSection settings={settings} disabled={settings === null || saving} save={save} />

      <QuietHoursSection settings={settings} disabled={settings === null || saving} save={save} />

      <PreferencesSection
        id="line-alerts-heading"
        title="Lines"
        description="Pinning a line puts it on your home page. Choose which pinned lines also send notifications."
      >
        {pinnedLines.length === 0 ? (
          <Text size="sm">Pin a line from its page to get alerts about it.</Text>
        ) : (
          <Stack gap="sm">
            {pinnedLines.map((line) => (
              <LineAlertSwitch key={line.id} line={line} lineStatusOn={settings?.lineStatus !== false} />
            ))}
          </Stack>
        )}
      </PreferencesSection>

      <DevicesSection version={devicesVersion} />
    </>
  );
}

/** The pauses offered, each as the time it ends from `now`. */
const PAUSES: { label: string; until: (now: Date) => Date }[] = [
  { label: 'Pause for 1 hour', until: (now) => new Date(now.getTime() + 60 * 60 * 1000) },
  { label: 'Pause for 4 hours', until: (now) => new Date(now.getTime() + 4 * 60 * 60 * 1000) },
  {
    label: 'Pause until 07:00 tomorrow',
    until: (now) => {
      const until = new Date(now);
      until.setDate(until.getDate() + 1);
      until.setHours(7, 0, 0, 0);
      return until;
    },
  },
];

/** "Pause notifications" (snooze): held as in quiet hours until the time
 * chosen, then the latest news. */
function PauseSection({
  settings,
  disabled,
  save,
}: {
  settings: NotificationPreferences | null;
  disabled: boolean;
  save: (change: NotificationPreferencesUpdate) => Promise<boolean>;
}) {
  const until = settings?.snoozedUntil ?? null;
  return (
    <PreferencesSection
      id="pause-heading"
      title="Pause"
      description="Pause notifications for a while. They wait until the pause ends, then you get the latest news. Cancellations still come through."
    >
      {until ? (
        <Group gap="sm">
          <Text size="sm" role="status">
            Paused until {formatDateTime(until)}.
          </Text>
          <Button size="xs" variant="default" disabled={disabled} onClick={() => void save({ snoozedUntil: null })}>
            Resume now
          </Button>
        </Group>
      ) : (
        <Group gap="xs">
          {PAUSES.map((pause) => (
            <Button
              key={pause.label}
              size="xs"
              variant="default"
              disabled={disabled}
              onClick={() => void save({ snoozedUntil: pause.until(new Date()).toISOString() })}
            >
              {pause.label}
            </Button>
          ))}
        </Group>
      )}
    </PreferencesSection>
  );
}

/** Quiet hours: a daily window in which notifications wait until it ends,
 * in this browser's time zone (or the one they were saved in). */
function QuietHoursSection({
  settings,
  disabled,
  save,
}: {
  settings: NotificationPreferences | null;
  disabled: boolean;
  save: (change: NotificationPreferencesUpdate) => Promise<boolean>;
}) {
  const saved = settings?.quietHours ?? null;
  const [start, setStart] = useState('22:00');
  const [end, setEnd] = useState('07:00');
  const [error, setError] = useState<string | null>(null);
  const timezone = saved?.timezone ?? browserTimeZone();

  useEffect(() => {
    if (saved) {
      // eslint-disable-next-line react-hooks/set-state-in-effect -- the form starts from the saved window once it has loaded
      setStart(saved.start);
      setEnd(saved.end);
    }
  }, [saved]);

  async function apply(on: boolean) {
    setError(null);
    if (!on) {
      await save({ quietHours: null });
      return;
    }
    if (!start || !end) {
      setError('Choose a start and an end time.');
      return;
    }
    if (start === end) {
      setError("Quiet hours can't start and end at the same time.");
      return;
    }
    await save({ quietHours: { start, end, timezone } });
  }

  const changed = saved !== null && (saved.start !== start || saved.end !== end);

  return (
    <PreferencesSection
      id="quiet-hours-heading"
      title="Quiet hours"
      description="Notifications that arrive in quiet hours wait until they end. Then you get the latest news, not a backlog. Cancellations still come through."
    >
      <Stack gap="sm">
        <Switch
          label="Quiet hours"
          checked={saved !== null}
          disabled={disabled}
          onChange={(event) => void apply(event.currentTarget.checked)}
        />
        <Group gap="md" align="flex-end">
          <TimeInput
            label="From"
            description={CLOCK_HINT}
            value={start}
            disabled={disabled}
            onChange={(event) => setStart(event.currentTarget.value)}
          />
          <TimeInput
            label="Until"
            value={end}
            disabled={disabled}
            onChange={(event) => setEnd(event.currentTarget.value)}
          />
          {changed && (
            <Button variant="default" onClick={() => void apply(true)} disabled={disabled}>
              Save quiet hours
            </Button>
          )}
        </Group>
        <Text size="sm" c="dimmed">
          Times are in {timezone}.
        </Text>
        {error && (
          <Text size="sm" c="var(--ds-color-error-text)" role="alert">
            {error}
          </Text>
        )}
      </Stack>
    </PreferencesSection>
  );
}

/** One pinned line's alert switch (`PUT /preferences/pinned-lines/{id}/alerts`). */
function LineAlertSwitch({ line, lineStatusOn }: { line: PinnedLineAlert; lineStatusOn: boolean }) {
  const [alerts, setAlerts] = useState(line.alerts);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function change(next: boolean) {
    setAlerts(next);
    setSaving(true);
    setError(null);
    try {
      const response = await fetch(`/api/preferences/pinned-lines/${encodeURIComponent(line.id)}/alerts`, {
        method: 'PUT',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ notify: next }),
      });
      if (!response.ok) {
        setAlerts(!next);
        setError(await failureFromResponse('save', 'this setting', response));
      }
    } catch {
      setAlerts(!next);
      setError(describeFailure('save', 'this setting'));
    } finally {
      setSaving(false);
    }
  }

  return (
    <Stack gap={4}>
      <Switch
        label={line.name}
        checked={alerts}
        disabled={saving || !lineStatusOn}
        onChange={(event) => void change(event.currentTarget.checked)}
      />
      {error && (
        <Text size="sm" c="var(--ds-color-error-text)" role="alert">
          {error}
        </Text>
      )}
    </Stack>
  );
}

/** The devices notifications go to (`GET /notifications/devices`), this one
 * marked, each other one removable. Reloads when `version` changes. */
function DevicesSection({ version }: { version: number }) {
  const [devices, setDevices] = useState<PushDevice[] | null>(null);
  const [current, setCurrent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [removing, setRemoving] = useState<number | null>(null);

  const load = useCallback(async () => {
    try {
      const [response, hash] = await Promise.all([fetch('/api/notifications/devices'), currentEndpointHash()]);
      setCurrent(hash);
      if (!response.ok) {
        setError(describeFailure('load', 'your devices', response.status));
        return;
      }
      setDevices((await response.json()) as PushDevice[]);
      setError(null);
    } catch {
      setError(describeFailure('load', 'your devices'));
    }
  }, []);

  useEffect(() => {
    // eslint-disable-next-line react-hooks/set-state-in-effect -- loads the list on mount and after this device's push changes
    void load();
  }, [load, version]);

  async function remove(id: number) {
    setRemoving(id);
    setError(null);
    try {
      const response = await fetch(`/api/notifications/devices/${String(id)}`, { method: 'DELETE' });
      if (!response.ok && response.status !== 404) {
        setError(await failureFromResponse('remove', 'this device', response));
        return;
      }
      setDevices((list) => list?.filter((device) => device.id !== id) ?? null);
    } catch {
      setError(describeFailure('remove', 'this device'));
    } finally {
      setRemoving(null);
    }
  }

  return (
    <PreferencesSection
      id="devices-heading"
      title="Devices"
      description="The browsers and phones your notifications go to. Remove one you no longer use."
    >
      {devices === null && !error && <Text size="sm">Loading devices</Text>}
      {devices?.length === 0 && <Text size="sm">{"Notifications aren't on for any device yet."}</Text>}
      {devices && devices.length > 0 && (
        <Stack gap="sm" role="list">
          {devices.map((device) => {
            const isThis = current !== null && device.endpointSha256 === current;
            return (
              <Group key={device.id} role="listitem" justify="space-between" wrap="wrap" gap="xs">
                <Stack gap={0}>
                  <Group gap="xs">
                    <Text fw={600}>{device.label ?? 'Unknown browser'}</Text>
                    {isThis && (
                      <Badge color="gray" variant="light">
                        This device
                      </Badge>
                    )}
                  </Group>
                  <Text size="sm" c="dimmed">
                    Last used {formatDateTime(device.lastUsedAt)}
                  </Text>
                </Stack>
                {isThis ? (
                  <Text size="sm" c="dimmed">
                    Turn it off above.
                  </Text>
                ) : (
                  <Button
                    size="xs"
                    variant="default"
                    onClick={() => void remove(device.id)}
                    disabled={removing === device.id}
                    aria-label={`Remove ${device.label ?? 'unknown browser'}`}
                  >
                    Remove
                  </Button>
                )}
              </Group>
            );
          })}
        </Stack>
      )}
      {error && (
        <Text size="sm" c="var(--ds-color-error-text)" role="alert">
          {error}
        </Text>
      )}
    </PreferencesSection>
  );
}
