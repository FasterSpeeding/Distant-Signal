import { Stack, Title, Text } from '@mantine/core';
import { StationSearchForm } from './StationSearchForm';

export default function StationSearchPage() {
  return (
    <Stack p="lg" gap="md">
      <Title order={1}>Station Disruption Lookup</Title>
      <Text c="dimmed">
        Enter a 3-letter CRS station code to see disruptions affecting lines through it.
      </Text>
      <StationSearchForm />
    </Stack>
  );
}
