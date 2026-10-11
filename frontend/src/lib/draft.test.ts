import { expect, test } from "vitest";
import { follow } from "./draft";

const stored = { socks5: null as string | null, cli_enabled: false };
const agent = { socks5: "127.0.0.1:2080", cli_enabled: true };

test("a first load copies the settings", () => {
  const draft = follow(null, null, stored);
  expect(draft).toEqual(stored);
  expect(draft).not.toBe(stored);
});

test("a draft nobody edited is replaced by the settings read since", () => {
  const draft = follow({ ...stored }, stored, agent);
  expect(draft).toEqual(agent);
  expect(draft).not.toBe(agent);
});

test("an edited draft is kept: its Save wins on purpose", () => {
  const draft = { ...stored, socks5: "127.0.0.1:1080" };
  expect(follow(draft, stored, agent)).toBe(draft);
});
