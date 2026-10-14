import { Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';
import type { TransitNetwork } from '@/lib/types';

/** A network's data credit at the foot of its pages: the licence text the
 * network's data comes with, and where the full credits are. */
export function TransitAttributionNote({ network }: { network: Pick<TransitNetwork, 'attribution'> }) {
  return (
    <Text size="xs" c="dimmed">
      {network.attribution.text}{' '}
      <TextLink href="/attribution" tone="inherit" underline="always" inline>
        Data credits
      </TextLink>
    </Text>
  );
}
