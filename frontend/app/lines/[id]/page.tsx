import { notFound } from 'next/navigation';
import { Stack, Title, Text, Divider } from '@mantine/core';
import Link from 'next/link';
import { ApiNotFoundError, getLineStatus } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { DisruptionDetail } from '@/components/DisruptionDetail';

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

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>{report.name}</Title>
      <Text c="dimmed">Operators: {report.operators.join(', ')}</Text>
      {/* Plain `<Link>` wrapping `Text` rather than `component={Link}` on a
          Mantine polymorphic prop: this page is a Server Component, and
          that pattern previously broke `next build`'s Server/Client
          boundary check (see LineStatusCard fix). */}
      <Link href={`/lines/${id}/history`} style={{ textDecoration: 'none' }}>
        <Text c="blue">View history</Text>
      </Link>
      {report.lineStatuses.map((status, i) => (
        <div key={i}>
          <Divider my="sm" />
          <Stack gap="xs">
            <StatusBadge severity={status.statusSeverity} />
            <Text>{status.reason}</Text>
            <Text size="sm" c="dimmed">
              Data quality: {status.dataQuality}
            </Text>
            {status.disruption && <DisruptionDetail disruption={status.disruption} />}
          </Stack>
        </div>
      ))}
    </Stack>
  );
}
