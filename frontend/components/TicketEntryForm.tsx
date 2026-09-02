'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import { Alert, Button, Group, Stack, Tabs, TextInput, Text } from '@mantine/core';
import { Dropzone, PDF_MIME_TYPE } from '@mantine/dropzone';
import { useNeedsLogin } from './useNeedsLogin';
import { LoginPromptModal } from './LoginPromptModal';
import { TextLink } from './TextLink';
import type { PartialTicket, TicketCreatedResponse, TicketEntryRequest, TicketSource } from '@/lib/types';

const CRS_PATTERN = /^[A-Za-z]{3}$/;
type Tab = 'manual' | 'pkpass' | 'pdf';

/** The upload/manual-entry flow for one journey, per
 * docs/superpowers/specs/2026-08-29-journey-ticket-tracking-frontend-design.md
 * Decision 2, extended by Part A of the upload-first plan to also work with
 * NO tracked train yet. Collapsed by default -- `label` (e.g. "Add a ticket
 * for this journey", "Add another ticket", or "Add a ticket" when no
 * `trackingId` is given) is the entry point that expands this into the real
 * form, matching Decision 1's "entry point that opens TicketEntryForm" --
 * the exact collapse mechanism is this plan's own choice, since the spec
 * doesn't detail it further, kept self-contained here so `TicketPanel`
 * stays a plain, server-renderable async function with no interactive
 * state of its own.
 *
 * Three ways to arrive at the same underlying field set and the same final
 * submit: manual entry (default, always available, every field optional),
 * `.pkpass` upload, and PDF upload. Both uploads are read-only PREVIEWS --
 * a `200` from either upload route pre-fills the manual-entry fields and
 * switches back to the manual view; it never bypasses that view or offers
 * a one-click accept. `source` is whatever tier produced the current
 * starting point and is NOT reset to 'manual' by a later manual edit --
 * only a user who never touched an upload keeps `source: 'manual'`.
 *
 * `trackingId` is now OPTIONAL. When given, every request targets the
 * `/Train/{trackingId}/tickets...` family, unchanged from before, and a
 * successful save just closes the form and refreshes the page (there's
 * already a tracked train to see the new ticket under). When omitted, every
 * request targets the flat `/Train/tickets...` family instead (a
 * STANDALONE ticket, per `20260901140000_standalone_tickets.sql` -- no
 * tracked train exists for it yet), and a successful save does NOT just
 * close the form: since extraction can never recover a date/time (see
 * `crates/api/src/data/ticket_extraction.rs`'s module doc), this app has no
 * way to guess which tracked train the ticket is for, so the form instead
 * shows a concrete next step -- find or create that tracked train, with
 * this ticket's own extracted origin pre-filled as a starting point and its
 * id carried forward so `/track`'s own form can attach it automatically
 * once a pin is created (see `TrackTrainForm`'s `attachTicketId` prop).
 *
 * Every request here goes through the same-origin `/api/Train/...` proxy
 * (Client Components can't read the server-only `API_BASE_URL` env var
 * `lib/api.ts` relies on -- same reasoning as `PinToggle`/`TrackTrainForm`),
 * fixed for binary uploads by this plan's own Task 1. */
export function TicketEntryForm({
  trackingId,
  label,
  defaultOpen,
}: {
  trackingId?: number;
  label: string;
  defaultOpen?: boolean;
}) {
  const router = useRouter();
  // Every existing call site omits this and keeps today's exact
  // collapsed-by-default behavior; only /track/mine/add-ticket passes
  // `defaultOpen` (a dedicated page whose entire reason for existing is
  // "add a ticket" has no competing content to protect the way
  // /track/mine's own trains list and unattached-tickets section do, so
  // forcing a click through a button reading the identical words the
  // page's own heading already committed to would be friction with
  // nothing behind it).
  const [open, setOpen] = useState(defaultOpen ?? false);
  const [tab, setTab] = useState<Tab>('manual');
  const [savedStandaloneTicket, setSavedStandaloneTicket] = useState<{ id: number; originCrs: string } | null>(null);

  const [operator, setOperator] = useState('');
  const [ticketType, setTicketType] = useState('');
  const [originCrs, setOriginCrs] = useState('');
  const [destinationCrs, setDestinationCrs] = useState('');
  const [source, setSource] = useState<TicketSource>('manual');
  const [autoFilled, setAutoFilled] = useState<Set<string>>(new Set());

  const [uploading, setUploading] = useState(false);
  const [uploadError, setUploadError] = useState<string | null>(null);

  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);
  const needsLoginState = useNeedsLogin();

  const originValid = originCrs.trim() === '' || CRS_PATTERN.test(originCrs.trim());
  const destinationValid = destinationCrs.trim() === '' || CRS_PATTERN.test(destinationCrs.trim());

  // The flat `Train/tickets...` family when there's no tracked train yet
  // (a STANDALONE ticket), the existing `Train/{trackingId}/tickets...`
  // family otherwise -- every upload/submit request below is built from
  // this one base path (no leading slash -- always used as
  // `/api/${ticketsBasePath}/...`), so the two modes never drift apart on
  // URL shape.
  const ticketsBasePath = trackingId !== undefined ? `Train/${trackingId}/tickets` : 'Train/tickets';

  function resetFields() {
    setOperator('');
    setTicketType('');
    setOriginCrs('');
    setDestinationCrs('');
    setSource('manual');
    setAutoFilled(new Set());
    setTab('manual');
    setUploadError(null);
    setSubmitError(null);
  }

  function clearAutoFilled(field: string) {
    setAutoFilled((current) => {
      const next = new Set(current);
      next.delete(field);
      return next;
    });
  }

  function applyPreview(preview: PartialTicket) {
    const filled = new Set<string>();
    if (preview.operator) {
      setOperator(preview.operator);
      filled.add('operator');
    }
    if (preview.ticketType) {
      setTicketType(preview.ticketType);
      filled.add('ticketType');
    }
    if (preview.originCrs) {
      setOriginCrs(preview.originCrs);
      filled.add('originCrs');
    }
    if (preview.destinationCrs) {
      setDestinationCrs(preview.destinationCrs);
      filled.add('destinationCrs');
    }
    setSource(preview.source);
    setAutoFilled(filled);
    setTab('manual');
  }

  async function handleUpload(file: File | null, kind: 'pkpass' | 'pdf') {
    if (!file) return;
    setUploading(true);
    setUploadError(null);
    needsLoginState.reset();
    try {
      const formData = new FormData();
      formData.append('file', file);
      // No explicit Content-Type header -- the browser sets the correct
      // 'multipart/form-data; boundary=...' value itself for a FormData
      // body, and Task 1's proxy fix is what lets that boundary survive to
      // the backend.
      const response = await fetch(`/api/${ticketsBasePath}/${kind}`, {
        method: 'POST',
        body: formData,
      });

      if (response.ok) {
        applyPreview((await response.json()) as PartialTicket);
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (response.status === 400) {
        setUploadError("That doesn't look like a valid upload — try again or fill in the form manually");
        return;
      }
      if (response.status === 422) {
        // Backend's own message is already human-readable, e.g. "could
        // not read this as a train .pkpass: ..." -- safe to surface
        // directly per Decision 2's table.
        setUploadError(await response.text());
        return;
      }
      if (response.status === 504) {
        setUploadError('That file took too long to read — try a smaller or simpler PDF, or fill in the details manually');
        return;
      }
      if (response.status === 413) {
        setUploadError('That file is too large (8 MB limit). Try filling in the details manually');
        return;
      }
      setUploadError("Couldn't read this file. Try filling in the details manually");
    } catch {
      setUploadError("Couldn't read this file. Try filling in the details manually");
    } finally {
      setUploading(false);
    }
  }

  async function handleSubmit() {
    setSubmitting(true);
    setSubmitError(null);
    needsLoginState.reset();
    try {
      const body: TicketEntryRequest = {
        source,
        ...(operator.trim() ? { operator: operator.trim() } : {}),
        ...(ticketType.trim() ? { ticket_type: ticketType.trim() } : {}),
        ...(originCrs.trim() ? { origin_crs: originCrs.trim().toUpperCase() } : {}),
        ...(destinationCrs.trim() ? { destination_crs: destinationCrs.trim().toUpperCase() } : {}),
      };
      const response = await fetch(`/api/${ticketsBasePath}`, {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify(body),
      });

      if (response.ok) {
        const created: TicketCreatedResponse = await response.json();
        if (trackingId === undefined) {
          // No tracked train exists for this ticket yet -- extraction can
          // never recover a date/time (see `ticket_extraction.rs`'s module
          // doc), so this app has no way to guess which one it's for. Show
          // the concrete next step instead of just closing: find or create
          // that tracked train, with this ticket's own origin (if any)
          // carried forward as a starting point and its id carried forward
          // so it can be attached automatically once a pin exists.
          setSavedStandaloneTicket({ id: created.ticketId, originCrs: originCrs.trim().toUpperCase() });
          resetFields();
          router.refresh();
          return;
        }
        setOpen(false);
        resetFields();
        router.refresh();
        return;
      }
      if (response.status === 401) {
        needsLoginState.markNeedsLogin();
        return;
      }
      if (response.status === 400) {
        setSubmitError(await response.text());
        return;
      }
      setSubmitError("Couldn't save this ticket. Try again.");
    } catch {
      setSubmitError("Couldn't save this ticket. Try again.");
    } finally {
      setSubmitting(false);
    }
  }

  if (savedStandaloneTicket) {
    // The concrete next step for a standalone ticket, per this
    // component's own doc comment: extraction never recovers a date/time,
    // so this app can't guess which tracked train the ticket is for --
    // hand the user straight to `/track`'s own form instead, with the
    // ticket's origin (if any) pre-filled as a starting point (the same
    // `initialOrigin` mechanism `/stations/[crs]`'s "Track a train from
    // here" shortcut already uses) and this ticket's id carried forward so
    // `TrackTrainForm` can attach it automatically once a pin is created.
    const trackParams = new URLSearchParams();
    if (savedStandaloneTicket.originCrs) {
      trackParams.set('origin', savedStandaloneTicket.originCrs);
    }
    trackParams.set('ticketId', String(savedStandaloneTicket.id));
    return (
      <Alert color="blue" title="Ticket saved">
        <Stack gap="sm">
          <Text size="sm">
            This ticket isn&apos;t attached to a tracked train yet — extraction can&apos;t tell us exactly which
            service you mean. Find or create the tracked train it&apos;s for, and it&apos;ll be attached
            automatically.
          </Text>
          <Group>
            <TextLink href={`/track?${trackParams.toString()}`} underline="always">
              Find or track the train this ticket is for
            </TextLink>
            <Button
              variant="subtle"
              size="xs"
              onClick={() => {
                setSavedStandaloneTicket(null);
                setOpen(false);
              }}
            >
              Done for now
            </Button>
          </Group>
        </Stack>
      </Alert>
    );
  }

  if (!open) {
    return (
      <Button variant="light" onClick={() => setOpen(true)}>
        {label}
      </Button>
    );
  }

  return (
    <Stack gap="md">
      <Tabs
        value={tab}
        onChange={(value) => {
          setTab((value as Tab) ?? 'manual');
          // A stale error from a previous failed upload on another tab
          // shouldn't linger on screen once the user has switched away --
          // no new upload attempt has happened yet on whichever tab they
          // land on.
          setUploadError(null);
        }}
      >
        <Tabs.List>
          <Tabs.Tab value="manual">Manual entry</Tabs.Tab>
          <Tabs.Tab value="pkpass">Upload .pkpass</Tabs.Tab>
          <Tabs.Tab value="pdf">Upload PDF e-ticket</Tabs.Tab>
        </Tabs.List>

        <Tabs.Panel value="manual" pt="md">
          <Stack gap="sm">
            <TextInput
              label="Operator"
              value={operator}
              onChange={(event) => {
                setOperator(event.currentTarget.value);
                clearAutoFilled('operator');
              }}
              description={autoFilled.has('operator') ? 'Auto-filled — please check this value' : undefined}
            />
            <TextInput
              label="Ticket type"
              value={ticketType}
              onChange={(event) => {
                setTicketType(event.currentTarget.value);
                clearAutoFilled('ticketType');
              }}
              description={autoFilled.has('ticketType') ? 'Auto-filled — please check this value' : undefined}
            />
            <TextInput
              label="Origin CRS code"
              value={originCrs}
              onChange={(event) => {
                setOriginCrs(event.currentTarget.value);
                clearAutoFilled('originCrs');
              }}
              error={!originValid ? 'Must be a 3-letter CRS code' : null}
              description={
                autoFilled.has('originCrs') ? 'Auto-filled — please check this is a real 3-letter CRS code' : undefined
              }
            />
            <TextInput
              label="Destination CRS code"
              value={destinationCrs}
              onChange={(event) => {
                setDestinationCrs(event.currentTarget.value);
                clearAutoFilled('destinationCrs');
              }}
              error={!destinationValid ? 'Must be a 3-letter CRS code' : null}
              description={
                autoFilled.has('destinationCrs')
                  ? 'Auto-filled — please check this is a real 3-letter CRS code'
                  : undefined
              }
            />
          </Stack>
        </Tabs.Panel>

        <Tabs.Panel value="pkpass" pt="md">
          <UploadPanel kind="pkpass" accept={['.pkpass']} uploading={uploading} error={uploadError} onFile={handleUpload}
            onFallback={() => setTab('manual')} />
        </Tabs.Panel>

        <Tabs.Panel value="pdf" pt="md">
          <UploadPanel kind="pdf" accept={PDF_MIME_TYPE} uploading={uploading} error={uploadError} onFile={handleUpload}
            onFallback={() => setTab('manual')} />
        </Tabs.Panel>
      </Tabs>

      {submitError && (
        <Alert color="red" title="Couldn't save this ticket">
          {submitError}
        </Alert>
      )}

      <Group>
        <Button onClick={handleSubmit} disabled={submitting || !originValid || !destinationValid}>
          {submitting ? 'Saving…' : 'Save ticket'}
        </Button>
        <Button variant="subtle" onClick={() => setOpen(false)}>
          Cancel
        </Button>
      </Group>
      <LoginPromptModal opened={needsLoginState.needsLogin} onClose={needsLoginState.reset}>
        Log in to save this ticket.
      </LoginPromptModal>
    </Stack>
  );
}

function UploadPanel({
  kind,
  accept,
  uploading,
  error,
  onFile,
  onFallback,
}: {
  kind: 'pkpass' | 'pdf';
  accept: string[];
  uploading: boolean;
  error: string | null;
  onFile: (file: File | null, kind: 'pkpass' | 'pdf') => void;
  onFallback: () => void;
}) {
  return (
    <Stack gap="sm">
      <Dropzone
        accept={accept}
        multiple={false}
        loading={uploading}
        onDrop={(files) => onFile(files[0] ?? null, kind)}
        onReject={() => {
          /* Decision 3: a mismatched file type is a client-side pre-filter,
           * not a request -- no request reaches handleUpload, so there is
           * nothing to report through the existing uploadError/UploadPanel
           * error path. Rendering polish (an inline "wrong file type"
           * message via Dropzone.Reject) is explicitly left to the
           * implementer per the spec's "Explicitly out of scope: Visual
           * design of the drop area itself." */
        }}
      >
        <Stack gap={4} align="center" style={{ pointerEvents: 'none' }}>
          <Text size="sm" fw={500}>
            {kind === 'pkpass' ? 'Apple Wallet .pkpass file' : 'PDF e-ticket'}
          </Text>
          <Text size="xs" c="dimmed">
            Drag and drop, or click to browse
          </Text>
        </Stack>
      </Dropzone>
      {error && (
        <Alert color="red">
          <Stack gap={4}>
            <span>{error}</span>
            {/* Always reachable, per Decision 2's table -- the manual form
                is right there and always usable regardless of why the
                upload failed. */}
            <Button variant="subtle" size="xs" onClick={onFallback} style={{ alignSelf: 'flex-start' }}>
              or fill in the details manually
            </Button>
          </Stack>
        </Alert>
      )}
    </Stack>
  );
}
