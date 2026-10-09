import { describe, it, expect } from 'vitest';
import { screen } from '@testing-library/react';
import { renderWithMantine } from '@/test/render';
import { AiGeneratedBadge, CHAT_AI_NOTE, ENRICHED_INCIDENT_NOTE, isEnricherInfluenced } from './AiGeneratedBadge';

describe('AiGeneratedBadge', () => {
  it('shows a short visible label', () => {
    renderWithMantine(<AiGeneratedBadge note={ENRICHED_INCIDENT_NOTE} />);
    expect(screen.getByText('AI summary')).toBeInTheDocument();
  });

  it('describes the badge with its "may be inaccurate" note for assistive technology', () => {
    renderWithMantine(<AiGeneratedBadge note={CHAT_AI_NOTE} />);
    const badge = screen.getByText('AI summary').closest('[data-ai-badge]')!;
    expect(badge).toHaveAccessibleDescription(CHAT_AI_NOTE);
    expect(CHAT_AI_NOTE).toMatch(/may be inaccurate/);
  });

  it('keeps the note in the DOM as text, not only in a hover tooltip', () => {
    renderWithMantine(<AiGeneratedBadge note={ENRICHED_INCIDENT_NOTE} />);
    expect(screen.getByText(ENRICHED_INCIDENT_NOTE)).toBeInTheDocument();
  });

  it('is not focusable, so it can sit inside buttons and links', () => {
    renderWithMantine(<AiGeneratedBadge note={ENRICHED_INCIDENT_NOTE} />);
    const badge = screen.getByText('AI summary').closest('[data-ai-badge]')!;
    expect(badge).not.toHaveAttribute('tabindex');
    expect(badge.tagName).not.toBe('BUTTON');
  });
});

describe('isEnricherInfluenced', () => {
  it.each([
    ['knowledgebase-incident-ABC123', true],
    ['ldbws-sampling', false],
    ['tfl-line-status-central', false],
    [null, false],
    [undefined, false],
  ])('%s -> %s', (source, expected) => {
    expect(isEnricherInfluenced(source)).toBe(expected);
  });
});
