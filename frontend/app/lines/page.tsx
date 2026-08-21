import { Stack, Title, Table, TableThead, TableTbody, TableTr, TableTh, TableTd, Text } from '@mantine/core';
import { getAllLines, getLineStatusForMode, getPreferences } from '@/lib/api';
import { PinToggle } from '@/components/PinToggle';
import { TextLink } from '@/components/TextLink';
import { StatusBadge } from '@/components/StatusBadge';
import { worstStatus } from '@/lib/severity';
import type { LineStatusReport } from '@/lib/types';
import { CustomLineForm } from './CustomLineForm';

export const revalidate = 0;

function sampleStatsFor(report: LineStatusReport | undefined) {
  return report?.lineStatuses.find((status) => status.sampleStats)?.sampleStats;
}

export default async function AllLinesPage() {
  const [lines, preferences, reports] = await Promise.all([
    getAllLines(),
    getPreferences(),
    getLineStatusForMode('national-rail'),
  ]);
  const pinnedSet = new Set(preferences.pinnedLines);
  const reportsById = new Map(reports.map((report) => [report.id, report]));

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
              <TableTh>Status</TableTh>
              <TableTh>Avg Delay</TableTh>
              <TableTh>Cancelled</TableTh>
              <TableTh>Pin</TableTh>
            </TableTr>
          </TableThead>
          <TableTbody>
            {lines.map((line) => {
              const report = reportsById.get(line.id);
              const worst = report ? worstStatus(report) : undefined;
              const stats = sampleStatsFor(report);
              const cancelledPct = stats && stats.total > 0 ? Math.round((stats.cancelled / stats.total) * 100) : null;
              return (
                <TableTr key={line.id}>
                  <TableTd>
                    <TextLink href={`/lines/${line.id}`}>{line.name}</TextLink>
                  </TableTd>
                  <TableTd>{line.category}</TableTd>
                  <TableTd>{line.operators.join(', ')}</TableTd>
                  <TableTd>{worst ? <StatusBadge severity={worst.statusSeverity} /> : null}</TableTd>
                  <TableTd>
                    {stats ? (
                      <Text size="sm">{stats.avgDelayMinutes.toFixed(1)} min</Text>
                    ) : (
                      <Text size="sm" c="dimmed">
                        —
                      </Text>
                    )}
                  </TableTd>
                  <TableTd>
                    {cancelledPct !== null ? (
                      <Text size="sm">{cancelledPct}%</Text>
                    ) : (
                      <Text size="sm" c="dimmed">
                        —
                      </Text>
                    )}
                  </TableTd>
                  <TableTd>
                    <PinToggle kind="line" id={line.id} initiallyPinned={pinnedSet.has(line.id)} />
                  </TableTd>
                </TableTr>
              );
            })}
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
