import type { ServiceMode } from '@/lib/types';

/** One icon per non-train service mode, in the same inline-SVG,
 * `currentColor`, 24-unit stroke style as `InfoIcon`/`WarningIcon` (the
 * project has no icon dependency). Each is distinct on purpose:
 *
 * - `bus`: a plain bus.
 * - `replacementBus`: the bus with a two-way arrow above it -- standing in
 *   for a train.
 * - `ferry`: a boat on water.
 *
 * Decorative: `aria-hidden`, and every use sits next to the mode's text
 * label (`ServiceModeBadge`), so nothing is said by the icon alone. A
 * train renders nothing. */
export function ServiceModeIcon({ mode, size = 16 }: { mode: ServiceMode; size?: number }) {
  if (mode === 'train') {
    return null;
  }
  return (
    <svg
      xmlns="http://www.w3.org/2000/svg"
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
      data-icon={mode}
    >
      {mode === 'ferry' ? <FerryPaths /> : <BusPaths replacement={mode === 'replacementBus'} />}
    </svg>
  );
}

function BusPaths({ replacement }: { replacement: boolean }) {
  return (
    <>
      {replacement ? (
        <>
          {/* Two-way arrow across the top: "instead of the train". */}
          <path d="M7 3h10" />
          <path d="M15 1l2 2-2 2" />
          <path d="M9 1L7 3l2 2" />
          <rect x="4" y="7" width="16" height="12" rx="2" />
          <path d="M4 13h16" />
          <circle cx="8" cy="21" r="1" />
          <circle cx="16" cy="21" r="1" />
        </>
      ) : (
        <>
          <rect x="4" y="3" width="16" height="16" rx="2" />
          <path d="M4 11h16" />
          <path d="M12 3v8" />
          <circle cx="8" cy="21" r="1" />
          <circle cx="16" cy="21" r="1" />
        </>
      )}
    </>
  );
}

function FerryPaths() {
  return (
    <>
      <path d="M3 14h18l-2 5H5z" />
      <path d="M6 14V9h12v5" />
      <path d="M12 9V4" />
      <path d="M2 22c2 0 2-1 4-1s2 1 4 1 2-1 4-1 2 1 4 1 2-1 4-1" />
    </>
  );
}
