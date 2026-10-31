import { Accordion, AccordionControl, AccordionItem, AccordionPanel, Stack, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';
import { transitStopHref } from '@/lib/transitStops';
import type { TransitLineBranch } from '@/lib/types';

/** One direction's branches on a line page (the TfL transit design §8.3):
 * each a closed panel named by its ends ("Edgware to Morden via Bank"),
 * holding its stations in order and a link to its own timetable. */
export function TransitBranches({
  network,
  line,
  branches,
}: {
  network: string;
  line: string;
  branches: readonly TransitLineBranch[];
}) {
  return (
    <Accordion multiple variant="separated" chevronPosition="left">
      {branches.map((branch) => (
        <AccordionItem key={branch.id} value={branch.id}>
          <AccordionControl>
            <Text fw={500}>{branch.name}</Text>
            <Text size="xs" c="dimmed">
              {branch.stations.length} stops
            </Text>
          </AccordionControl>
          <AccordionPanel>
            <Stack gap="xs">
              <Stack component="ol" gap={4} className="ds-plain-list">
                {branch.stations.map((station) => (
                  <li key={station.id}>
                    <TextLink href={transitStopHref(network, station.id)}>{station.name}</TextLink>
                  </li>
                ))}
              </Stack>
              <TextLink
                href={`/transit/${network}/lines/${line}/timetable?${new URLSearchParams({
                  direction: branch.direction,
                  branch: branch.id,
                }).toString()}`}
              >
                Timetable for this branch
              </TextLink>
            </Stack>
          </AccordionPanel>
        </AccordionItem>
      ))}
    </Accordion>
  );
}
