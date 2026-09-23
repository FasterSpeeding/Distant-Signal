import { Anchor, Card, Group, Stack, Text, Title } from '@mantine/core';
import Link from 'next/link';
import { getMyJourneyTemplates } from '@/lib/api';
import { LoginLink } from '@/components/LoginLink';
import { TextLink } from '@/components/TextLink';
import { formatDate } from '@/lib/dateFormat';
import { routeLabel } from '@/lib/stationLabel';
import type { JourneyTemplateListItem } from '@/lib/types';

export const revalidate = 0;

/** `/journeys/templates` -- the templates list, per
 * docs/superpowers/specs/2026-09-22-reusable-repeating-journeys-design.md
 * §6 item 3. `getMyJourneyTemplates()` returns `null` on a 401 (not "no
 * templates"), same "not logged in at all" signal `getMyJourneys`/
 * `getMyTrackedTrains` already use -- this page shows a login prompt for
 * that case, an empty state for a real logged-in-but-templateless caller. */
export default async function JourneyTemplatesPage() {
  const templates = await getMyJourneyTemplates();

  if (templates === null) {
    return (
      <Stack p="lg" gap="md">
        <Title order={1}>Your journey templates</Title>
        <LoginLink underline="always">Log in to see your journey templates</LoginLink>
      </Stack>
    );
  }

  return (
    <Stack p="lg" gap="md">
      <TextLink href="/track/mine" underline="always">
        Back to my trains &amp; journeys
      </TextLink>
      <Title order={1}>Your journey templates</Title>
      {templates.length === 0 && (
        <Text c="dimmed">
          No templates yet. Open a journey and choose &quot;Make this a template&quot; to save
          its shape for reuse.
        </Text>
      )}
      {templates.map((template) => (
        <TemplateCard key={template.id} template={template} />
      ))}
    </Stack>
  );
}

function TemplateCard({ template }: { template: JourneyTemplateListItem }) {
  const route = routeLabel(
    template.firstOriginCrs,
    template.firstOriginName,
    template.lastDestinationCrs,
    template.lastDestinationName,
  );
  const title = template.customName ?? route;
  return (
    <Card withBorder>
      <Group justify="space-between">
        <Stack gap={2}>
          <Anchor component={Link} href={`/journeys/templates/${template.id}`} fw={600}>
            {title}
          </Anchor>
          {template.customName && (
            <Text size="sm" c="dimmed">
              {route}
            </Text>
          )}
          <Text size="xs" c="dimmed">
            {template.legCount} {template.legCount === 1 ? 'leg' : 'legs'} · saved{' '}
            {formatDate(template.createdAt)}
          </Text>
        </Stack>
      </Group>
    </Card>
  );
}
