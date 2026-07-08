import { SimpleGrid, Title, Stack } from '@mantine/core';
import { getLineStatusForMode } from '@/lib/api';
import { LineStatusCard } from '@/components/LineStatusCard';

// `/` has no dynamic route segment, so without this Next.js treats the
// `next: { revalidate: 30 }` fetch in getLineStatusForMode as eligible for
// static generation and tries to prerender it during `next build` — which
// fails in the Docker build, since the `api` service only exists on the
// compose network at runtime, not inside the image build. `revalidate = 0`
// (rather than `dynamic = 'force-dynamic'`) is used deliberately: it also
// forces per-request dynamic rendering, but unlike `force-dynamic` it does
// NOT override a fetch's own `next: { revalidate: N }` option when that
// fetch sets a positive value, so getLineStatusForMode's `revalidate: 30`
// still governs the Data Cache — preserving the ~30s cache window the
// design spec calls for instead of fetching fresh on every request.
export const revalidate = 0;

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
