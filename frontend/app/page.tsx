import { SimpleGrid, Title, Stack } from '@mantine/core';
import { getLineStatusForMode } from '@/lib/api';
import { LineStatusCard } from '@/components/LineStatusCard';

// `/` has no dynamic route segment, so without this Next.js treats the
// `next: { revalidate: 30 }` fetch in getLineStatusForMode as eligible for
// static generation and tries to prerender it during `next build` — which
// fails in the Docker build, since the `api` service only exists on the
// compose network at runtime, not inside the image build. Forcing dynamic
// rendering makes this page (like every other page here) resolve
// API_BASE_URL at request time only, matching the Dockerfile's assumption.
export const dynamic = 'force-dynamic';

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
