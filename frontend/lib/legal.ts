/** Operator-specific values for the legal pages (`/privacy`, `/terms`,
 * `/cookies`, `/contact`), and the flag that publishes them.
 *
 * DRAFT -- NOT YET REVIEWED. The page text under `app/privacy`, `app/terms`,
 * `app/cookies` and `app/contact` was drafted from the 2026-09-27 UK legal
 * compliance gap analysis (LEG-1, LEG-2, LEG-8, LEG-11, LEG-12, LEG-13,
 * LEG-25). It is not legal advice. The operator, and ideally a lawyer, must
 * review every page before `LEGAL_PAGES_PUBLISHED` is switched on.
 *
 * Every value the operator has to supply lives in `LEGAL_CONFIG` below, as a
 * `[[LIKE_THIS]]` placeholder. The pages stay unpublished (404, and hidden
 * from the footer) until BOTH:
 *   1. the server env `LEGAL_PAGES_PUBLISHED` is exactly `true`
 *      (chart value `frontend.legalPagesPublished`, off by default), and
 *   2. no placeholder is left in `LEGAL_CONFIG`.
 * The second check means flipping the flag early can't put `[[CONTACT_EMAIL]]`
 * in front of users. `LEGAL_PAGES_PREVIEW=true` renders the drafts with a
 * "draft" banner and `noindex` for review and for CI's accessibility sweep;
 * see `legalPagesMode`. */

import type { Metadata } from 'next';

/** Matches a value the operator hasn't filled in yet. */
const PLACEHOLDER = /\[\[[A-Z0-9_]+\]\]/;

export interface LegalConfig {
  /** The data controller: an individual's name or a trading name
   * (audit open question 1). */
  OPERATOR_NAME: string;
  /** A monitored inbox for privacy requests, content reports (Online Safety
   * Act 2023 ss. 20-21), complaints and accessibility requests. */
  CONTACT_EMAIL: string;
  /** The ICO data protection fee registration number, or the literal
   * `'not required'` once the ICO self-assessment says no fee is due
   * (LEG-9). Record the self-assessment outcome either way. */
  ICO_REGISTRATION: string;
  /** Minimum age to create an account: `'18'` or `'13'` (audit open
   * question 3). 18 is simplest for the OSA children's duties and the
   * Children's Code. */
  MINIMUM_AGE: string;
  /** How long the sign-in service (Authentik) keeps its event logs,
   * including login IP addresses, e.g. `'1 year'` (audit open question 8;
   * Authentik's default is 1 year). */
  SSO_LOG_RETENTION: string;
  /** Date the operator last reviewed the text, e.g. `'1 November 2026'`. */
  LAST_UPDATED: string;
}

// DRAFT: operator must replace every placeholder before publishing.
export const LEGAL_CONFIG: LegalConfig = {
  OPERATOR_NAME: '[[OPERATOR_NAME]]',
  CONTACT_EMAIL: '[[CONTACT_EMAIL]]',
  ICO_REGISTRATION: '[[ICO_REGISTRATION]]',
  MINIMUM_AGE: '[[MINIMUM_AGE]]',
  SSO_LOG_RETENTION: '[[SSO_LOG_RETENTION]]',
  LAST_UPDATED: '[[LAST_UPDATED]]',
};

/** Names of the `LEGAL_CONFIG` fields still holding a placeholder. */
export function unfilledLegalPlaceholders(config: LegalConfig = LEGAL_CONFIG): string[] {
  return (Object.keys(config) as (keyof LegalConfig)[]).filter((key) => PLACEHOLDER.test(config[key]));
}

let warnedPlaceholders = false;

/** Test-only reset for the one-time warning below. */
export function __resetLegalWarningForTests(): void {
  warnedPlaceholders = false;
}

export type LegalPagesMode = 'off' | 'preview' | 'published';

/** How `/privacy`, `/terms`, `/cookies` and `/contact` are served:
 *   - `'published'`: `LEGAL_PAGES_PUBLISHED=true` and no placeholder left.
 *   - `'preview'`: `LEGAL_PAGES_PREVIEW=true` (and not published). The pages
 *     render with a "draft" banner and `noindex`, placeholders and all, so
 *     the operator can review them on a staging deployment and CI's axe
 *     sweep can reach them. Never set this in production.
 *   - `'off'` (the default): the pages 404 and the footer hides their links.
 *
 * Read from `process.env` at call time, so every caller must render per
 * request (`dynamic = 'force-dynamic'`): the image build never sets these,
 * and a build-time prerender would freeze them off. */
export function legalPagesMode(
  env: Record<string, string | undefined> = process.env,
  config: LegalConfig = LEGAL_CONFIG,
): LegalPagesMode {
  if (env.LEGAL_PAGES_PUBLISHED === 'true') {
    const unfilled = unfilledLegalPlaceholders(config);
    if (unfilled.length === 0) return 'published';
    if (!warnedPlaceholders) {
      warnedPlaceholders = true;
      console.warn(
        `LEGAL_PAGES_PUBLISHED is true but frontend/lib/legal.ts still has placeholders (${unfilled.join(', ')}); ` +
          'the legal pages stay unpublished until they are filled in.',
      );
    }
  }
  return env.LEGAL_PAGES_PREVIEW === 'true' ? 'preview' : 'off';
}

/** Whether the legal pages are live for real (not just previewed). */
export function legalPagesPublished(
  env: Record<string, string | undefined> = process.env,
  config: LegalConfig = LEGAL_CONFIG,
): boolean {
  return legalPagesMode(env, config) === 'published';
}

/** Whether the legal pages (and their footer links) render at all. */
export function legalPagesVisible(
  env: Record<string, string | undefined> = process.env,
  config: LegalConfig = LEGAL_CONFIG,
): boolean {
  return legalPagesMode(env, config) !== 'off';
}

/** Route names of the account self-service features the privacy notice
 * points to. The account export and deletion work is a separate change;
 * keep these in sync with it. */
export const ACCOUNT_ROUTE = '/account';
export const ACCOUNT_EXPORT_LABEL = 'Download my data';
export const ACCOUNT_DELETE_LABEL = 'Delete my account';

/** Footer links for the legal pages, in display order. */
export const LEGAL_LINKS: readonly { href: string; label: string }[] = [
  { href: '/privacy', label: 'Privacy' },
  { href: '/terms', label: 'Terms' },
  { href: '/cookies', label: 'Cookies' },
  { href: '/contact', label: 'Contact' },
];

/** Metadata for a legal page. Unless the pages are really published
 * (e.g. in preview), it adds `noindex` so a draft never lands in a search
 * index. Call from `generateMetadata` so it is evaluated per request. */
export function legalPageMetadata(
  title: string,
  description: string,
  published: boolean = legalPagesPublished(),
): Metadata {
  const fullTitle = `${title} — Distant Signal`;
  return {
    title: fullTitle,
    description,
    openGraph: { title: fullTitle, description, type: 'website' },
    twitter: { card: 'summary', title: fullTitle, description },
    ...(published ? {} : { robots: { index: false, follow: false } }),
  };
}
