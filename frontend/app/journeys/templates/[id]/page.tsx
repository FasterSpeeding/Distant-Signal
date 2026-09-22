import { Group, Stack, Title } from '@mantine/core';
import { notFound } from 'next/navigation';
import { getJourneyTemplate, ApiNotFoundError, ApiUnauthorizedError } from '@/lib/api';
import { DeleteJourneyTemplateButton } from '@/components/DeleteJourneyTemplateButton';
import { EditJourneyTemplateForm } from '@/components/EditJourneyTemplateForm';
import { LoginLink } from '@/components/LoginLink';
import { RunTemplateNowButton } from '@/components/RunTemplateNowButton';
import { TextLink } from '@/components/TextLink';

export const revalidate = 0;

/** `/journeys/templates/[id]` -- detail/edit view, §6 item 3. Templates
 * have no group-shared read path in Phase B (unlike `/journeys/[id]`,
 * which does) -- every field and control here is owner-only by
 * construction, since `GET /JourneyTemplates/{id}` itself 404s for
 * anyone but the owner. */
export default async function JourneyTemplateDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;
  if (!/^\d+$/.test(id)) {
    notFound();
  }

  let template;
  try {
    template = await getJourneyTemplate(Number(id));
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    if (err instanceof ApiUnauthorizedError) {
      return (
        <Stack p="lg" gap="md">
          <Title order={1}>Someone&apos;s journey template — log in to see it</Title>
          <LoginLink underline="always">Log in to view this template</LoginLink>
        </Stack>
      );
    }
    throw err;
  }

  return (
    <Stack p="lg" gap="md">
      <TextLink href="/journeys/templates" underline="always">
        Back to your templates
      </TextLink>
      <Group justify="space-between" align="baseline">
        <Title order={1}>{template.customName ?? 'Journey template'}</Title>
        <Group gap="xs">
          <RunTemplateNowButton templateId={template.id} />
          <DeleteJourneyTemplateButton templateId={template.id} />
        </Group>
      </Group>
      <EditJourneyTemplateForm template={template} />
    </Stack>
  );
}
