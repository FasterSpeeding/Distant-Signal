import { notFound } from 'next/navigation';
import { Stack, Title, Text, Group, Button } from '@mantine/core';
import Link from 'next/link';
import { ApiNotFoundError, getLineStatus, getCustomLine } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { DeleteLineButton } from '@/components/DeleteLineButton';
import { worstStatus } from '@/lib/severity';

export default async function LineDetailPage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;

  let reports;
  try {
    reports = await getLineStatus([id], true);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  const report = reports[0];
  const worst = worstStatus(report);

  // `getCustomLine` 404s for a catalogue-line id (the endpoint only ever
  // reads the `custom_lines` table) — that expected 404 is how this page
  // tells a custom line apart from a catalogue one, without needing a
  // second "is this custom" field on the status endpoint itself.
  let isCustom = true;
  try {
    await getCustomLine(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      isCustom = false;
    } else {
      throw err;
    }
  }

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Title order={1}>{report.name}</Title>
        <Group gap="sm">
          {isCustom && (
            <>
              {/* Plain `<Link>` wrapping `Button`, not `component={Link}`
                  on a Mantine polymorphic prop — this page is a Server
                  Component, and that pattern previously broke
                  `next build`'s Server/Client boundary check (see
                  LineStatusCard's fix). */}
              <Link href={`/lines/${id}/edit`} style={{ textDecoration: 'none' }}>
                <Button variant="outline" size="xs">Edit</Button>
              </Link>
              <DeleteLineButton id={id} />
            </>
          )}
          <StatusBadge severity={worst.statusSeverity} />
        </Group>
      </Group>
      <Text c="dimmed">Operators: {report.operators.join(', ')}</Text>
      <Link href={`/lines/${id}/history`} style={{ textDecoration: 'none' }}>
        <Text c="blue">View history</Text>
      </Link>
      <RepresentativeInfo statuses={report.lineStatuses} />
      <IssueList statuses={report.lineStatuses} />
    </Stack>
  );
}
