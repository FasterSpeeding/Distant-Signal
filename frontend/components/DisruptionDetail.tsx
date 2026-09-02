'use client';

import { Stack, Text, Badge, Group } from '@mantine/core';
import type { Disruption } from '@/lib/types';
import { sanitizeDescription } from '@/lib/sanitizeHtml';
import { incidentIdFromSource } from '@/lib/incidents';
import { impactTypeLabel } from '@/lib/impactType';
import { TextLink } from './TextLink';

export function DisruptionDetail({ disruption }: { disruption: Disruption }) {
  const incidentId = incidentIdFromSource(disruption.source);
  const impactLabel = impactTypeLabel(disruption.impactType);
  return (
    <Stack gap="xs">
      {impactLabel && (
        <Badge variant="light" color="orange" w="fit-content">
          {impactLabel}
        </Badge>
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
        <Group gap="xs">
          {disruption.affectedStops.map((crs) => (
            <Badge key={crs} variant="outline" color="gray">
              {crs}
            </Badge>
          ))}
        </Group>
      )}
      {disruption.affectedRoutes.map((route, i) => (
        <Text key={i} size="sm" c="dimmed">
          {route.from} → {route.to}
        </Text>
      ))}
      {disruption.source && (
        <Text size="xs" c="dimmed">
          Source: {disruption.source}
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
