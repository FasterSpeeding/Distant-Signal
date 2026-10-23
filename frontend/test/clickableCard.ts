/** The `ClickableCard` a card's title link (or button) sits in. A card's
 * link is named by its title alone and holds only the title, so a test
 * that checks the rest of the card's text looks in the card. */
export function cardOf(target: HTMLElement): HTMLElement {
  const card = target.closest<HTMLElement>('[data-clickable-card]');
  if (card === null) throw new Error('Not inside a ClickableCard');
  return card;
}
