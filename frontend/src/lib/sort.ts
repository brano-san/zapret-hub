export type Direction = "asc" | "desc";

/**
 * A sorted copy. Empty values go last in either direction; ties keep the input order, which for
 * a finished run is the ranking. Strings compare with numbers as numbers (`ALT2` before `ALT10`).
 */
export function sortBy<T>(
  rows: readonly T[],
  value: (row: T) => number | string | null | undefined,
  dir: Direction,
): T[] {
  const sign = dir === "asc" ? 1 : -1;
  return rows
    .map((row, index) => ({ row, index, key: value(row) }))
    .sort((a, b) => {
      if (a.key == null || b.key == null) {
        if (a.key == null && b.key == null) return a.index - b.index;
        return a.key == null ? 1 : -1;
      }
      const order =
        typeof a.key === "number" && typeof b.key === "number"
          ? a.key - b.key
          : String(a.key).localeCompare(String(b.key), "ru", { numeric: true });
      return order !== 0 ? sign * order : a.index - b.index;
    })
    .map((item) => item.row);
}
