import { SimpleGrid, Title, Stack } from '@mantine/core';
import { getLineStatusForMode } from '@/lib/api';
import { LineStatusCard } from '@/components/LineStatusCard';

export default async function DashboardPage() {
  const reports = await getLineStatusForMode('national-rail');

  return (
    <Stack p="lg" gap="md">
      <Title order={1}>National Rail Line Status</Title>
      <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
        {reports.map((report) => (
          <LineStatusCard key={report.id} report={report} />
        ))}
      </SimpleGrid>
    </Stack>
  );
}
