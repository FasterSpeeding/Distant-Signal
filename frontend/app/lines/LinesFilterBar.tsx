'use client';

import { useEffect, useId, useRef, useState, type ReactNode } from 'react';
import {
  Badge,
  Box,
  Button,
  CloseButton,
  Drawer,
  Group,
  Popover,
  PopoverDropdown,
  PopoverTarget,
  Stack,
  Text,
  TextInput,
  Title,
  UnstyledButton,
  VisuallyHidden,
  useMantineTheme,
} from '@mantine/core';
import { useDisclosure, useMediaQuery, useReducedMotion } from '@mantine/hooks';

/** Feather `search`, 16px, decorative. */
function SearchIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <circle cx="11" cy="11" r="8" />
      <line x1="21" y1="21" x2="16.65" y2="16.65" />
    </svg>
  );
}

/** Feather `sliders`, 16px, decorative. */
function FiltersIcon() {
  return (
    <svg
      width="16"
      height="16"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      focusable="false"
    >
      <line x1="4" y1="21" x2="4" y2="14" />
      <line x1="4" y1="10" x2="4" y2="3" />
      <line x1="12" y1="21" x2="12" y2="12" />
      <line x1="12" y1="8" x2="12" y2="3" />
      <line x1="20" y1="21" x2="20" y2="16" />
      <line x1="20" y1="12" x2="20" y2="3" />
      <line x1="1" y1="14" x2="7" y2="14" />
      <line x1="9" y1="8" x2="15" y2="8" />
      <line x1="17" y1="16" x2="23" y2="16" />
    </svg>
  );
}

function linesLabel(count: number): string {
  return `${count} line${count === 1 ? '' : 's'}`;
}

/** The /lines search and filter bar: one row holding "Find a line" and a
 * "Filters" button, sticky at every width so search is always one tap
 * away. The filters themselves (`children`) live behind the button, in a
 * popover from `sm` up and a bottom sheet below it, so the bar costs one
 * input's height instead of the ~300px the old always-open block took on a
 * phone.
 *
 * Deliberately not hide-on-scroll: the bar is already small, and a bar that
 * slides away while the visitor scrolls (or while the phone keyboard
 * resizes the viewport) is the jumpy behaviour this replaces.
 *
 * Transparent until it is actually stuck, so it never paints a solid box
 * over the page's top-of-page colour wash (`body` in app/globals.css);
 * once stuck it gets the body colour and a bottom rule, so rows scrolling
 * under it don't show through. */
export function LinesFilterBar({
  query,
  onQueryChange,
  activeCount,
  matchCount,
  onClearFilters,
  children,
}: {
  query: string;
  onQueryChange: (next: string) => void;
  /** How many filter values are set inside the panel (not the search). */
  activeCount: number;
  /** Rows left after every filter and the search. */
  matchCount: number;
  /** Resets the panel's filters, leaving the search alone. */
  onClearFilters: () => void;
  children: ReactNode;
}) {
  const theme = useMantineTheme();
  // `undefined` before mount, so the first render (server and client
  // alike) is the popover branch; by the time anyone can press the button
  // the real value is in. Only the container differs between the two, not
  // the button, so nothing in the bar shifts when this resolves.
  const isPhone = useMediaQuery(`(max-width: calc(${theme.breakpoints.sm} - 0.0625em))`);
  const reduceMotion = useReducedMotion();
  const [opened, { toggle, close }] = useDisclosure(false);
  const panelId = useId();
  const titleId = useId();
  const countId = useId();

  const sentinelRef = useRef<HTMLDivElement>(null);
  const [stuck, setStuck] = useState(false);
  useEffect(() => {
    const sentinel = sentinelRef.current;
    if (!sentinel || typeof IntersectionObserver === 'undefined') return;
    const observer = new IntersectionObserver(([entry]) => {
      if (entry) setStuck(!entry.isIntersecting && entry.boundingClientRect.top < 0);
    });
    observer.observe(sentinel);
    return () => observer.disconnect();
  }, []);

  const transitionProps = reduceMotion ? { duration: 0 } : {};

  const panel = (
    <Stack gap="md">
      {children}
      <Group justify="space-between" align="center" wrap="nowrap">
        {activeCount > 0 ? (
          <UnstyledButton onClick={onClearFilters} className="textButton">
            Clear filters
          </UnstyledButton>
        ) : (
          <span />
        )}
        <Button onClick={close} size="sm">
          {matchCount === 0 ? 'Close' : `Show ${linesLabel(matchCount)}`}
        </Button>
      </Group>
      {matchCount === 0 && (
        <Text size="sm" c="dimmed">
          No lines match these filters.
        </Text>
      )}
    </Stack>
  );

  const button = (
    <Button
      variant="default"
      leftSection={<FiltersIcon />}
      rightSection={
        activeCount > 0 ? (
          <Badge size="sm" circle aria-hidden="true" data-testid="lines-filter-count">
            {activeCount}
          </Badge>
        ) : undefined
      }
      onClick={toggle}
      aria-expanded={opened}
      aria-haspopup="dialog"
      aria-controls={opened && !isPhone ? panelId : undefined}
      aria-describedby={countId}
      flex="0 0 auto"
    >
      Filters
    </Button>
  );

  return (
    <>
      <div ref={sentinelRef} className="linesFilterBarSentinel" aria-hidden="true" />
      <Box className="linesFilterBar" data-stuck={stuck || undefined}>
        <Group gap="xs" wrap="nowrap" align="center">
          <TextInput
            aria-label="Find a line"
            placeholder="Find a line"
            value={query}
            onChange={(event) => onQueryChange(event.currentTarget.value)}
            onKeyDown={(event) => {
              // Lets "Search" on a phone keyboard put the keyboard away so
              // the results are visible; there is nothing to submit.
              if (event.key === 'Enter') event.currentTarget.blur();
            }}
            enterKeyHint="search"
            autoComplete="off"
            leftSection={<SearchIcon />}
            rightSection={
              query ? <CloseButton size="sm" aria-label="Clear search" onClick={() => onQueryChange('')} /> : undefined
            }
            flex="1 1 auto"
            miw={0}
          />
          <Popover
            opened={opened && !isPhone}
            onDismiss={close}
            position="bottom-end"
            width={340}
            shadow="md"
            trapFocus
            returnFocus
            withRoles={false}
            // Escape is handled on the dropdown below instead: Mantine's own
            // handler runs in the capture phase, so the Escape that should
            // only close the operator list closed the whole panel.
            closeOnEscape={false}
            transitionProps={transitionProps}
          >
            <PopoverTarget>{button}</PopoverTarget>
            <PopoverDropdown
              id={panelId}
              role="dialog"
              aria-labelledby={titleId}
              onKeyDown={(event) => {
                if (event.key !== 'Escape') return;
                // A combobox whose list was open has just closed it.
                if (event.target instanceof HTMLElement && event.target.getAttribute('aria-expanded') === 'true')
                  return;
                close();
              }}
            >
              <Stack gap="sm">
                {/* The focus trap's first stop. Without it focus lands on
                    the operator input, and a searchable MultiSelect opens
                    its list on focus, covering the rest of the panel. */}
                <Title order={2} size="h5" id={titleId} tabIndex={-1} data-autofocus>
                  Filters
                </Title>
                {panel}
              </Stack>
            </PopoverDropdown>
          </Popover>
          <VisuallyHidden id={countId}>{activeCount === 0 ? 'None on' : `${activeCount} on`}</VisuallyHidden>
        </Group>
      </Box>
      <Drawer
        opened={opened && isPhone}
        onClose={close}
        position="bottom"
        title="Filters"
        transitionProps={transitionProps}
        classNames={{ content: 'linesFilterSheet', title: 'linesFilterSheetTitle' }}
      >
        {panel}
      </Drawer>
    </>
  );
}
