import { notFound } from 'next/navigation';
import { Center, Stack, Title } from '@mantine/core';
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
    // this chrome's width in lockstep with the form's, so the heading lines
    // up with the form's edges instead of just picking an independent width
    // that happens to look similar.
    <Center>
      <Stack p="lg" gap="md" maw={480} w="100%">
        <Title order={1}>Edit: {line.name}</Title>
        <CustomLineForm existingLine={line} cancelHref={`/lines/${id}`} />
      </Stack>
    </Center>
  );
}
