/**
 * Where the reader is in a tour of stacked cards: the card on top (the last one whose top has
 * reached `line`), and the way through the tour from 0 to 1 (the cards passed, plus how far
 * the next one has come up from the bottom of the window towards `line`).
 */
export function tourPosition(
  tops: readonly number[],
  line: number,
  height: number,
): { active: number; progress: number } {
  let active = 0;
  for (const [index, top] of tops.entries()) if (top <= line) active = index;
  const next = tops[active + 1];
  const coming =
    next === undefined ? 0 : Math.min(1, Math.max(0, (height - next) / Math.max(1, height - line)));
  const progress = tops.length > 1 ? Math.min(1, (active + coming) / (tops.length - 1)) : 0;
  return { active, progress };
}
