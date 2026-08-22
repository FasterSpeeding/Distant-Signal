import { notFound } from 'next/navigation';
import { Stack, Title, Text, Group, Button } from '@mantine/core';
import Link from 'next/link';
import { ApiNotFoundError, getLineStatus, getCustomLine, getLineDefinition, getAllLines } from '@/lib/api';
import { StatusBadge } from '@/components/StatusBadge';
import { RepresentativeInfo } from '@/components/RepresentativeInfo';
import { IssueList } from '@/components/IssueList';
import { DeleteLineButton } from '@/components/DeleteLineButton';
import { LineDefinitionTooltip } from '@/components/LineDefinitionTooltip';
import { TextLink } from '@/components/TextLink';
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

  // Category only exists on `LineSummary` (from `getAllLines`), not on the
  // `LineStatusReport` this page otherwise relies on -- fetched here, after
  // the notFound() check above, so an unknown line id still 404s cleanly.
  const lines = await getAllLines();
  const category = lines.find((line) => line.id === id)?.category;

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

  // A tooltip showing stations/operators is a nice-to-have, not core page
  // functionality — if this fails for any reason, just don't show it
  // rather than breaking the whole page over it. Works for both catalogue
  // and custom lines (unlike `getCustomLine` above, `getLineDefinition`
  // doesn't 404 for a catalogue id — see its backend doc comment for why
  // that endpoint stays separate from this one).
  let definition = null;
  try {
    definition = await getLineDefinition(id);
  } catch {
    // swallowed — see comment above
  }

  // Stamped server-side so IssueList's buckets don't depend on a
  // `Date.now()` that differs between the SSR pass and hydration. Fresh on
  // every request (this route is dynamic) and re-stamped by AutoRefresh.
  const now = Date.now();

  return (
    <Stack p="lg" gap="md">
      <Group justify="space-between">
        <Group gap="xs">
          <Title order={1}>{report.name}</Title>
          {definition && <LineDefinitionTooltip stations={definition.stations} operators={definition.operators} />}
        </Group>
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
      {category && <Text c="dimmed">Category: {category}</Text>}
      <Text c="dimmed">Operators: {report.operators.join(', ')}</Text>
      <TextLink href={`/lines/${id}/history`} underline="always">
        View history
      </TextLink>
      <RepresentativeInfo statuses={report.lineStatuses} />
      {/* Every issue here belongs to the line already named in the heading,
          so no per-issue line attribution is needed — that's what the
          optional `lines` on IssueItem is for on the station page. */}
      <IssueList items={report.lineStatuses.map((status) => ({ status }))} now={now} />
    </Stack>
  );
}
