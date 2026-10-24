/**
 * The option a key moves `Select`'s highlight to, as a native `<select>` does: arrows step and
 * stop at the ends, Home/End jump, a letter goes to the next label starting with it (wrapping).
 * `null`: the key does not move the highlight.
 */
export function moveActive(key: string, active: number, labels: string[]): number | null {
  const last = labels.length - 1;
  if (last < 0) return null;
  switch (key) {
    case "ArrowDown":
      return Math.min(active + 1, last);
    case "ArrowUp":
      return Math.max(active - 1, 0);
    case "Home":
    case "PageUp":
      return 0;
    case "End":
    case "PageDown":
      return last;
  }
  if (key.length !== 1 || key === " ") return null;
  const letter = key.toLowerCase();
  for (let step = 1; step <= labels.length; step++) {
    const i = (active + step) % labels.length;
    if (labels[i]?.toLowerCase().startsWith(letter)) return i;
  }
  return null;
}
