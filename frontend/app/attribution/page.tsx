import { Stack, Title } from '@mantine/core';
import type { Metadata } from 'next';
import { OpenDataAttributionDetails } from '@/components/OpenDataAttribution';

// Render per request so the footer's legal links (lib/legal.ts, read at
// request time) are right on this page too. Without it Next prerenders this
// page at build time, with the flags as they were in the image build.
export const dynamic = 'force-dynamic';

const METADATA_TITLE = 'Data sources and licences — Distant Signal';
const METADATA_DESCRIPTION =
  'The open data Distant Signal uses, with the attribution each licence requires: National Rail, Network Rail, RSP, TfL, the National Transport Authority, Iarnród Éireann and Translink.';

export const metadata: Metadata = {
  title: METADATA_TITLE,
  description: METADATA_DESCRIPTION,
  openGraph: { title: METADATA_TITLE, description: METADATA_DESCRIPTION, type: 'website' },
  twitter: { card: 'summary', title: METADATA_TITLE, description: METADATA_DESCRIPTION },
};

/** `/attribution`: always public (unlike the draft legal pages), because the
 * licences below require the attribution now. Content lives in
 * `components/OpenDataAttribution.tsx` beside the footer's short version. */
export default function AttributionPage() {
  return (
    <Stack p="lg" gap="lg" maw={760}>
      <Title order={1}>Data sources and licences</Title>
      <OpenDataAttributionDetails />
    </Stack>
  );
}
