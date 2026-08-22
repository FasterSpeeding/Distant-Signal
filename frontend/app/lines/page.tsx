import { Stack, Title } from '@mantine/core';
import { getAllLines, getAllTocs, getLineStatusForMode, getPreferences } from '@/lib/api';
import { DISPLAYED_MODES_PARAM } from '@/lib/modes';
import { CustomLineForm } from './CustomLineForm';
import { AllLinesTable } from './AllLinesTable';

export const revalidate = 0;

export default async function AllLinesPage() {
  const [lines, preferences, reports, tocs] = await Promise.all([
    getAllLines(),
    getPreferences(),
    getLineStatusForMode(DISPLAYED_MODES_PARAM),
    getAllTocs(),
  ]);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Title order={1}>All Lines</Title>
        <AllLinesTable lines={lines} reports={reports} pinnedLineIds={preferences.pinnedLines} tocs={tocs} />
      </Stack>

      <Stack gap="md">
        <Title order={2}>New Custom Line</Title>
        <CustomLineForm />
      </Stack>
    </Stack>
  );
}
