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

  // A `200` from `getCustomLine` stopped meaning "you own this" when
  // custom-line group sharing landed: a member of a group the owner shared
  // the line into gets the same full detail (see
  // docs/superpowers/specs/2026-09-12-custom-line-group-sharing-design.md
  // §3.2/§3.5). Rendering the edit form for them would be a form whose
  // only possible outcome is a 404 from an owner-only `PUT`, on someone
  // else's private line. The backend is still the authority
  // (`update_custom_line` is gated purely on `user_id = caller.id` and is
  // completely grant-blind); this makes the page agree with it, and 404s
  // for the same reason a non-owner has always been 404'd here.
  if (!line.isOwner) {
    notFound();
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
