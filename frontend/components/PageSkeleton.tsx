import type { ReactNode } from 'react';
import { Card, Group, SimpleGrid, Skeleton, Stack, VisuallyHidden, type MantineSpacing } from '@mantine/core';
import { tokens } from '@/lib/theme';

/** Heights of the text a skeleton stands in for, in px: the line box of
 * each Mantine size (font size times line height, docs/style-guide.md,
 * Typography). A bar this tall keeps the page from jumping when the real
 * text replaces it. */
const LINE_BOX = {
  h1: 44,
  section: 31,
  md: 25,
  sm: 20,
  xs: 17,
  badge: 20,
  button: 36,
} as const;

/** A whole-page skeleton for a route's `loading.tsx`: the page's real
 * padding, width and vertical rhythm, with grey bars where its headings,
 * cards and rows will be.
 *
 * One `role="status"` / `aria-busy` region, named by a visually hidden
 * word label ("Loading line"). Everything drawn inside it is
 * `aria-hidden`, so a screen reader hears the label once and nothing
 * else. The label is hidden rather than shown (unlike
 * `LoadingPlaceholder`) because the skeleton is the page: a visible
 * "Loading" line would sit where the h1 is about to land and push it down.
 * Mantine's skeleton shimmer says "still loading" to a sighted reader, and
 * it stops under reduced motion like every other animation.
 *
 * No hooks, so it renders on the server as the route's Suspense fallback. */
export function PageSkeleton({
  label,
  gap = 'md',
  maw,
  children,
}: {
  /** Announced, e.g. "Loading line". A plain word, no "…". */
  label: string;
  /** The page's own `Stack` gap, so its blocks start where the real ones will. */
  gap?: MantineSpacing;
  /** The page's own max width, for the 640px and 480px pages. */
  maw?: number;
  children: ReactNode;
}) {
  return (
    <Stack p="lg" gap={gap} maw={maw} role="status" aria-busy="true" data-page-skeleton>
      <VisuallyHidden>{label}</VisuallyHidden>
      <Stack gap={gap} aria-hidden="true">
        {children}
      </Stack>
    </Stack>
  );
}

/** A heading's bar: the h1 by default, or a section heading
 * (`SectionTitle`, drawn at h3 size). `width` is a share of the row, so a
 * short page title doesn't draw as a full-width slab. */
export function SkeletonHeading({
  level = 'h1',
  width = '45%',
}: {
  level?: 'h1' | 'section';
  width?: string | undefined;
}) {
  return <Skeleton height={LINE_BOX[level]} width={width} maw="100%" radius="sm" data-skeleton-heading={level} />;
}

/** One or more lines of text. The last of several is shorter, the way a
 * paragraph's last line is. */
export function SkeletonText({
  lines = 1,
  size = 'md',
  width = '100%',
}: {
  lines?: number;
  size?: 'md' | 'sm' | 'xs';
  width?: string;
}) {
  return (
    <Stack gap={4}>
      {Array.from({ length: lines }, (_, index) => (
        <Skeleton
          key={index}
          height={LINE_BOX[size] - 6}
          my={3}
          width={lines > 1 && index === lines - 1 ? '60%' : width}
          radius="sm"
        />
      ))}
    </Stack>
  );
}

/** A small pill, for a status badge or a button in a page's header. */
export function SkeletonPill({ width = 96, kind = 'badge' }: { width?: number; kind?: 'badge' | 'button' }) {
  return <Skeleton height={LINE_BOX[kind]} width={width} radius={kind === 'badge' ? 'xl' : 'md'} />;
}

/** The page header: the h1 bar on the left, with optional controls (pills)
 * on the right, as the real `Group justify="space-between"` header lays
 * them out. `subline` adds the dimmed line under the h1 that train pages
 * carry ("Headcode 1S00 · UID W12345"). */
export function SkeletonPageHeader({
  actions = 0,
  subline = false,
  width,
}: {
  actions?: number;
  subline?: boolean;
  width?: string | undefined;
}) {
  return (
    <Group justify="space-between" wrap="wrap" gap="sm">
      <Stack gap={2} flex={1} miw={0}>
        <SkeletonHeading width={width} />
        {subline && <SkeletonText size="sm" width="35%" />}
      </Stack>
      {actions > 0 && (
        <Group gap="sm">
          {Array.from({ length: actions }, (_, index) => (
            <SkeletonPill key={index} kind="button" width={index === actions - 1 ? 44 : 96} />
          ))}
        </Group>
      )}
    </Group>
  );
}

/** A status card, shaped like `LineStatusCard`: title and badge on one
 * row, a reason of up to three lines, then a small detail line. Drawn with
 * the static card token: a skeleton isn't a link yet, and Mantine drops
 * the link card's shadow in dark mode anyway. */
export function SkeletonStatusCard() {
  return (
    <Card {...tokens.card.static} padding={tokens.statusCard.padding}>
      <Stack gap="xs">
        <Group justify="space-between" wrap="nowrap" gap="xs">
          <Skeleton height={LINE_BOX.md - 6} my={3} width="55%" radius="sm" />
          <SkeletonPill width={88} />
        </Group>
        <SkeletonText lines={2} size="sm" />
        <SkeletonText size="xs" width="40%" />
      </Stack>
    </Card>
  );
}

/** A grid of status cards in the home page's own columns (1, 2, then 3). */
export function SkeletonCardGrid({ count = 3 }: { count?: number }) {
  return (
    <SimpleGrid cols={{ base: 1, sm: 2, lg: 3 }} spacing="md">
      {Array.from({ length: count }, (_, index) => (
        <SkeletonStatusCard key={index} />
      ))}
    </SimpleGrid>
  );
}

/** A list of row cards (`ClickableCard` rows on the home page and /journeys):
 * a title line with a badge, and a dimmed detail line under it. */
export function SkeletonRows({ count = 3 }: { count?: number }) {
  return (
    <Stack gap="xs">
      {Array.from({ length: count }, (_, index) => (
        <Card key={index} {...tokens.card.static}>
          <Stack gap={4}>
            <Group justify="space-between" wrap="nowrap" gap="xs">
              <Skeleton height={LINE_BOX.md - 6} my={3} width="50%" radius="sm" />
              <SkeletonPill width={80} />
            </Group>
            <SkeletonText size="sm" width="35%" />
          </Stack>
        </Card>
      ))}
    </Stack>
  );
}

/** A train's calling points: one row per stop, time on the left and the
 * station name beside it, as `TrainJourney` lists them. */
export function SkeletonStops({ count = 8 }: { count?: number }) {
  return (
    <Stack gap="sm">
      {Array.from({ length: count }, (_, index) => (
        <Group key={index} gap="md" wrap="nowrap">
          <Skeleton height={LINE_BOX.md - 6} my={3} width={48} radius="sm" />
          <Skeleton height={LINE_BOX.md - 6} my={3} width={index % 2 === 0 ? '45%' : '35%'} radius="sm" />
        </Group>
      ))}
    </Stack>
  );
}

/** A section: its heading bar, then whatever the section holds. */
export function SkeletonSection({ children, gap = 'md' }: { children: ReactNode; gap?: MantineSpacing }) {
  return (
    <Stack gap={gap}>
      <SkeletonHeading level="section" width="30%" />
      {children}
    </Stack>
  );
}
