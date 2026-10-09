import { Stack, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { OpenDataAttributionDetails } from '@/components/OpenDataAttribution';
import { READING_PAGE_WIDTH } from '@/components/LegalPage';
import { pageMetadata } from '@/lib/pageMetadata';

// Render per request so the footer's legal links (lib/legal.ts, read at
// request time) are right on this page too. Without it Next prerenders this
// page at build time, with the flags as they were in the image build.
export const dynamic = 'force-dynamic';

const METADATA_TITLE = 'Data sources and licences';
const METADATA_DESCRIPTION = 'The open data Distant Signal uses, and the credit each licence requires.';

export const metadata: Metadata = pageMetadata(METADATA_TITLE, METADATA_DESCRIPTION);

/** `/attribution`: always public (unlike the draft legal pages), because the
 * licences below require the attribution now. Content lives in
 * `components/OpenDataAttribution.tsx` beside the footer's short version. */
export default function AttributionPage() {
  return (
    <Stack p="lg" gap="lg" maw={READING_PAGE_WIDTH}>
      <Title order={1}>Data sources and licences</Title>
      <OpenDataAttributionDetails />
    </Stack>
  );
}
