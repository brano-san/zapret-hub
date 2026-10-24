import { describe, expect, it } from "vitest";
import { moveActive } from "./select";

describe("moveActive", () => {
  const labels = ["как в системе", "светлая", "тёмная", "своя"];

  it("steps with the arrows and stops at the ends", () => {
    expect(moveActive("ArrowDown", 0, labels)).toBe(1);
    expect(moveActive("ArrowDown", 3, labels)).toBe(3);
    expect(moveActive("ArrowUp", 1, labels)).toBe(0);
    expect(moveActive("ArrowUp", 0, labels)).toBe(0);
  });

  it("jumps with Home and End", () => {
    expect(moveActive("Home", 2, labels)).toBe(0);
    expect(moveActive("End", 0, labels)).toBe(3);
  });

  it("goes to the next label starting with a letter, wrapping", () => {
    expect(moveActive("с", 0, labels)).toBe(1);
    expect(moveActive("с", 1, labels)).toBe(3);
    expect(moveActive("С", 3, labels)).toBe(1);
    expect(moveActive("я", 0, labels)).toBeNull();
  });

  it("leaves other keys and an empty list alone", () => {
    expect(moveActive(" ", 0, labels)).toBeNull();
    expect(moveActive("Enter", 0, labels)).toBeNull();
    expect(moveActive("ArrowDown", 0, [])).toBeNull();
  });
});
