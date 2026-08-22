import { Stack, Title, Text } from '@mantine/core';
import { TextLink } from '@/components/TextLink';

export default function StationNotFound() {
  return (
    <Stack p="lg" gap="md">
      <Title order={2}>Station not found</Title>
      <Text c="dimmed">
        No National Rail station matches that code. Station codes are three letters, like WOK or EUS.
      </Text>
      <TextLink href="/stations" underline="always">
        Look up a station
      </TextLink>
    </Stack>
  );
}
