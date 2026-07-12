import { notFound } from 'next/navigation';
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

  return <CustomLineForm existingLine={line} />;
}
