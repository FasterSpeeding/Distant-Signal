import { Card, Stack, Text } from '@mantine/core';
import type { ReactNode } from 'react';
import { SectionTitle } from '@/components/SectionTitle';

/** One card on /account/preferences: a section heading, an optional
 * one-line description and the controls. Every section on the page is
 * built from this, so a new one (connected apps, say) matches the rest. */
export function PreferencesSection({
  id,
  title,
  description,
  children,
}: {
  /** The heading's id, which the card's `aria-labelledby` points at. */
  id: string;
  title: string;
  description?: ReactNode;
  children: ReactNode;
}) {
  return (
    <Card withBorder component="section" aria-labelledby={id}>
      <Stack gap="md" align="stretch">
        <Stack gap={4}>
          <SectionTitle id={id}>{title}</SectionTitle>
          {description !== undefined && (
            <Text size="sm" c="dimmed">
              {description}
            </Text>
          )}
        </Stack>
        {children}
      </Stack>
    </Card>
  );
}
