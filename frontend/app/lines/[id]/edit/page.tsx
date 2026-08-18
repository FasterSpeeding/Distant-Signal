import { notFound } from 'next/navigation';
import { Center, Stack, Title, Group, Button } from '@mantine/core';
import Link from 'next/link';
import { ApiNotFoundError, getCustomLine } from '@/lib/api';
import { CustomLineForm } from '../../CustomLineForm';

export default async function EditCustomLinePage({
  params,
}: {
  params: Promise<{ id: string }>;
}) {
  const { id } = await params;

  let line;
  try {
    line = await getCustomLine(id);
  } catch (err) {
    if (err instanceof ApiNotFoundError) {
      notFound();
    }
    throw err;
  }

  return (
    // `Center` plus a `maw` matching CustomLineForm's own `maw={480}` keeps
    // this chrome's width in lockstep with the form's, so the heading and
    // Cancel button line up with the form's edges instead of just picking
    // an independent width that happens to look similar.
    <Center>
      <Stack p="lg" gap="md" maw={480} w="100%">
        <Title order={1}>Edit: {line.name}</Title>
        <CustomLineForm existingLine={line} />
        <Group justify="flex-end">
          {/* Plain `<Link>` wrapping `Button`, not `component={Link}` on a
              Mantine polymorphic prop — this page is a Server Component,
              and that pattern previously broke `next build`'s
              Server/Client boundary check (see LineDetailPage's Edit
              link, which hit the same issue). Cancel lives outside
              CustomLineForm so it can't accidentally submit the form. */}
          <Link href={`/lines/${id}`} style={{ textDecoration: 'none' }}>
            <Button variant="default">Cancel</Button>
          </Link>
        </Group>
      </Stack>
    </Center>
  );
}
