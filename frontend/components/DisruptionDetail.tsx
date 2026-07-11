'use client';

import DOMPurify from 'isomorphic-dompurify';
import { Stack, Text, Badge, Group } from '@mantine/core';
import type { Disruption } from '@/lib/types';

// Registered once at module load. `disruption.description` comes from the
// Darwin/Knowledgebase feed already fully HTML-entity-decoded by the time
// it reaches the frontend (see poller-incidents' quick_xml parsing) — it's
// real markup, not escaped/serialized XML needing re-parsing. DOMPurify's
// ALLOWED_ATTR strips `target`/`rel` by default since they're not in the
// allowlist below; this hook adds them back on every surviving `<a>` so
// external links don't inherit this page's window/referrer.
DOMPurify.addHook('afterSanitizeAttributes', (node) => {
  if (node.tagName === 'A') {
    node.setAttribute('target', '_blank');
    node.setAttribute('rel', 'noopener');
  }
});

const ALLOWED_TAGS = ['p', 'br', 'strong', 'b', 'em', 'i', 'ul', 'ol', 'li', 'a'];
const ALLOWED_ATTR = ['href'];

function sanitizeDescription(html: string): string {
  return DOMPurify.sanitize(html, { ALLOWED_TAGS, ALLOWED_ATTR });
}

export function DisruptionDetail({ disruption }: { disruption: Disruption }) {
  return (
    <Stack gap="xs">
      <div dangerouslySetInnerHTML={{ __html: sanitizeDescription(disruption.description) }} />
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
    </Stack>
  );
}
