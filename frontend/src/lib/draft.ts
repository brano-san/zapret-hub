/**
 * A form's copy once the stored value is read again (`next`); `from` is what the copy was last taken from.
 * A first load and a copy nobody edited take `next`, so the form's Save cannot put back what zapret-hub-cli
 * changed meanwhile (CLI spec 8). An edited copy stays: its Save wins on purpose.
 */
export function follow<T extends object>(draft: T | null, from: T | null, next: T): T {
  return draft === null || JSON.stringify(draft) === JSON.stringify(from) ? { ...next } : draft;
}
