import { beforeEach, expect, test, vi } from "vitest";
import { notices } from "./notices.svelte";
import { S } from "./strings";
import { start } from "./app.svelte";

// `app.svelte` asks for `matchMedia` when it is imported and `applyTheme` writes to `document`: Node has neither.
const ipc = vi.hoisted(() => {
  Object.assign(globalThis, {
    matchMedia: () => ({ matches: false, addEventListener: () => {} }),
    document: { documentElement: { dataset: {} } },
  });
  return { log: [] as string[], failing: "" };
});

vi.mock("./ipc/bindings", () => {
  const listen = async () => {
    ipc.log.push("listen");
    return () => {};
  };
  const read = (name: string, value: unknown) => async () => {
    ipc.log.push(name);
    if (name === ipc.failing) throw new Error(`${name} is down`);
    return value;
  };
  return {
    events: { stateChanged: { listen }, noticesChanged: { listen }, testProgress: { listen }, testsFinished: { listen } },
    commands: {
      getSettings: read("getSettings", {}),
      takeNotices: read("takeNotices", []),
      testLive: read("testLive", null),
      getOverview: read("getOverview", { status: "ok", data: {} }),
    },
  };
});

beforeEach(() => {
  ipc.log.length = 0;
  ipc.failing = "";
  notices.items = [];
});

test("start listens before it reads and stays quiet when nothing fails", async () => {
  await start();
  expect(ipc.log.slice(0, 5)).toEqual(["listen", "listen", "listen", "listen", "getSettings"]);
  expect(notices.items).toEqual([]);
});

test.each(["getSettings", "takeNotices", "testLive"])("a rejected %s becomes an Error notice with the error as is", async (name) => {
  ipc.failing = name;
  await start();
  expect(notices.items).toEqual([{ level: "Error", title: S.failed.start, text: `Error: ${name} is down` }]);
});
