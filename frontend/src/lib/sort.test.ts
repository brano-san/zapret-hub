import { describe, expect, it } from "vitest";
import { sortBy } from "./sort";

describe("sortBy", () => {
  const rows = [
    { name: "a", value: 3 },
    { name: "b", value: null },
    { name: "c", value: 1 },
    { name: "d", value: 3 },
  ];

  it("puts empty values last either way and keeps ties in order", () => {
    expect(sortBy(rows, (row) => row.value, "asc").map((row) => row.name)).toEqual(["c", "a", "d", "b"]);
    expect(sortBy(rows, (row) => row.value, "desc").map((row) => row.name)).toEqual(["a", "d", "c", "b"]);
  });

  it("compares numbers inside names as numbers", () => {
    const names = ["general (ALT10)", "general (ALT2)", "general"].map((name) => ({ name }));
    expect(sortBy(names, (row) => row.name, "asc").map((row) => row.name)).toEqual([
      "general",
      "general (ALT2)",
      "general (ALT10)",
    ]);
  });
});
