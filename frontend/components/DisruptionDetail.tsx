'use client';

import { Stack, Text, Badge, Group } from '@mantine/core';
import type { Disruption } from '@/lib/types';
import { sanitizeDescription } from '@/lib/sanitizeHtml';

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
