'use client';

import { Stack, Text } from '@mantine/core';
import type { Disruption } from '@/lib/types';
import { sanitizeDescription } from '@/lib/sanitizeHtml';
import { incidentIdFromSource } from '@/lib/incidents';
import { impactTypeLabel } from '@/lib/impactType';
import { incidentSourceLabel } from '@/lib/incidentSource';
import { TextLink } from './TextLink';
import { AiGeneratedBadge, ENRICHED_INCIDENT_NOTE, isEnricherInfluenced } from './AiGeneratedBadge';
import { BadgeRow, StateBadge } from './StateBadge';

export function DisruptionDetail({ disruption }: { disruption: Disruption }) {
  const incidentId = incidentIdFromSource(disruption.source);
  const impactLabel = impactTypeLabel(disruption.impactType);
  const enriched = isEnricherInfluenced(disruption.source);
  return (
    <Stack gap="xs">
      {impactLabel && (
        // `impactType` only ever comes from the enricher's LLM output.
        <BadgeRow
          state={<StateBadge tone="late">{impactLabel}</StateBadge>}
          ai={enriched ? <AiGeneratedBadge note={ENRICHED_INCIDENT_NOTE} /> : undefined}
        />
      )}
      {/* `data-rich-text`: the CSS hook for `app/globals.css`'s
          `[data-rich-text] a` rule. Anchors inside knowledgebase incident
          copy arrive as external HTML, so they carry no Mantine class and
          (per `lib/sanitizeHtml.ts`'s `ALLOWED_ATTR = ['href']`) no class or
          data attribute of their own -- they were rendering browser-default
          blue next to blue "PLANNED WORK" badges, the exact collision the
          grape theme was created to eliminate
          (docs/superpowers/specs/2026-09-02-frontend-ui-ux-review.md §F7).
          A descendant selector from this container is the only way to reach
          them, and this is the same data-attribute pattern `data-text-link`
          and `data-status-badge` already use. */}
      <div data-rich-text dangerouslySetInnerHTML={{ __html: sanitizeDescription(disruption.description) }} />
      {disruption.affectedStops.length > 0 && (
        // Dimmed text, not pills (round 2): stations an item affects are
        // never badges.
        <Text size="sm" c="dimmed">
          Stops: {disruption.affectedStops.join(', ')}
        </Text>
      )}
      {disruption.affectedRoutes.map((route, i) => (
        <Text key={i} size="sm" c="dimmed">
          {route.from} to {route.to}
        </Text>
      ))}
      {incidentSourceLabel(disruption.source) && (
        // `title` keeps the raw provenance string one hover away for debugging
        // without putting a 32-hex ID in body copy -- the same tactic
        // CustomLineForm.tsx uses for its code/name pills. Not an
        // InfoIcon+Tooltip (components/InfoIcon.tsx): that's a heavier control
        // than a value no user needs to read deserves. Note the incident id
        // this string carries is ALREADY surfaced usefully below, as the
        // "View full incident details" link (via lib/incidents.ts) -- so
        // nothing is lost by not printing it.
        <Text size="xs" c="dimmed" title={disruption.source ?? undefined}>
          Source: {incidentSourceLabel(disruption.source)}
        </Text>
      )}
      {enriched && (
        // Visible, not tooltip-only: this is the one place a sighted
        // keyboard user can read the AI note (the badges are not focusable).
        <Text size="xs" c="dimmed" data-ai-note>
          {ENRICHED_INCIDENT_NOTE}
        </Text>
      )}
      {incidentId && (
        <TextLink href={`/incidents/${encodeURIComponent(incidentId)}`} underline="always">
          View full incident details
        </TextLink>
      )}
    </Stack>
  );
}
