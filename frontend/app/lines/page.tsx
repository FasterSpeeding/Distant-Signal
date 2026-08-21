import { Stack, Title } from '@mantine/core';
import { getAllLines, getLineStatusForMode, getPreferences } from '@/lib/api';
import { CustomLineForm } from './CustomLineForm';
import { AllLinesTable } from './AllLinesTable';

export const revalidate = 0;

export default async function AllLinesPage() {
  const [lines, preferences, reports] = await Promise.all([
    getAllLines(),
    getPreferences(),
    getLineStatusForMode('national-rail'),
  ]);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Title order={1}>All Lines</Title>
        <AllLinesTable lines={lines} reports={reports} pinnedLineIds={preferences.pinnedLines} />
      </Stack>

      <Stack gap="md">
        <Title order={2}>New Custom Line</Title>
        <CustomLineForm />
      </Stack>
    </Stack>
  );
}
