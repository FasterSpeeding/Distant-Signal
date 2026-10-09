'use client';

import { Text } from '@mantine/core';
import { useMounted } from '@mantine/hooks';
import { TextLink } from './TextLink';

/** An email address that only becomes a `mailto:` link in the browser.
 *
 * Cloudflare's Email Address Obfuscation rewrites any address it finds in
 * the HTML into "[email protected]" plus a decoding script, and our CSP
 * (rightly) blocks that script, so visitors saw "[email protected]". The
 * server render therefore carries no address pattern at all ("info at
 * example.co.uk"); the real link appears once the page has mounted, after
 * Cloudflare has seen the HTML. Turning the zone's Email Address
 * Obfuscation off (Scrape Shield) fixes the same thing at the edge. */
export function EmailLink({ address }: { address: string }) {
  const mounted = useMounted();
  if (!mounted) {
    return <Text span>{address.replace('@', ' at ')}</Text>;
  }
  return (
    <TextLink href={`mailto:${address}`} underline="always">
      {address}
    </TextLink>
  );
}
