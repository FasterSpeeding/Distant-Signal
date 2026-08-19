import { Stack, Title, Table, TableThead, TableTbody, TableTr, TableTh, TableTd } from '@mantine/core';
import { getAllLines, getPreferences } from '@/lib/api';
import { PinToggle } from '@/components/PinToggle';
import { TextLink } from '@/components/TextLink';
import { CustomLineForm } from './CustomLineForm';

export const revalidate = 0;

export default async function AllLinesPage() {
  const [lines, preferences] = await Promise.all([getAllLines(), getPreferences()]);
  const pinnedSet = new Set(preferences.pinnedLines);

  return (
    <Stack p="lg" gap="xl">
      <Stack gap="md">
        <Title order={1}>All Lines</Title>
        <Table>
          {/* Flat `TableThead`/`TableTr`/... named exports, not the
              `Table.Thead` dot-notation compound API — this page is a
              Server Component, and under this Next.js 16 + Turbopack RSC
              setup the compound sub-components resolve to `undefined`
              (client-reference proxies don't forward property access),
              throwing "Element type is invalid" at render time. The flat
              exports are plain client references and work correctly. */}
          <TableThead>
            <TableTr>
              <TableTh>Name</TableTh>
              <TableTh>Category</TableTh>
              <TableTh>Operators</TableTh>
              <TableTh>Pin</TableTh>
            </TableTr>
          </TableThead>
          <TableTbody>
            {lines.map((line) => (
              <TableTr key={line.id}>
                <TableTd>
                  <TextLink href={`/lines/${line.id}`}>{line.name}</TextLink>
                </TableTd>
                <TableTd>{line.category}</TableTd>
                <TableTd>{line.operators.join(', ')}</TableTd>
                <TableTd>
                  <PinToggle kind="line" id={line.id} initiallyPinned={pinnedSet.has(line.id)} />
                </TableTd>
              </TableTr>
            ))}
          </TableTbody>
        </Table>
      </Stack>

      <Stack gap="md">
        <Title order={2}>New Custom Line</Title>
        <CustomLineForm />
      </Stack>
    </Stack>
  );
}
