import { beforeEach, expect, test, vi } from "vitest";
import { notices } from "./notices.svelte";
import { S } from "./strings";
import { app, start } from "./app.svelte";
import { confirmation } from "./confirm.svelte";

// `app.svelte` asks for `matchMedia` when it is imported and `applyTheme` writes to `document`: Node has neither.
const ipc = vi.hoisted(() => {
  Object.assign(globalThis, {
    matchMedia: () => ({ matches: false, addEventListener: () => {} }),
    document: { documentElement: { dataset: {} } },
  });
  return { log: [] as string[], failing: "", settings: {} as object, stateChanged: () => {} };
});

vi.mock("./ipc/bindings", () => {
  const listen = async () => {
    ipc.log.push("listen");
    return () => {};
  };
  const read = (name: string, value: () => unknown) => async () => {
    ipc.log.push(name);
    if (name === ipc.failing) throw new Error(`${name} is down`);
    return value();
  };
  return {
    events: {
      stateChanged: {
        listen: async (handler: () => void) => {
          ipc.stateChanged = handler;
          return listen();
        },
      },
      noticesChanged: { listen },
      testProgress: { listen },
      testsFinished: { listen },
      diagProgress: { listen },
      diagFinished: { listen },
    },
    commands: {
      getSettings: read("getSettings", () => ipc.settings),
      takeNotices: read("takeNotices", () => []),
      testLive: read("testLive", () => null),
      diagLive: read("diagLive", () => null),
      getOverview: read("getOverview", () => ({ status: "ok", data: {} })),
    },
  };
});

beforeEach(() => {
  ipc.log.length = 0;
  ipc.failing = "";
  ipc.settings = {};
  notices.items = [];
});

test("start listens before it reads and stays quiet when nothing fails", async () => {
  await start();
  expect(ipc.log.slice(0, 7)).toEqual(["listen", "listen", "listen", "listen", "listen", "listen", "getSettings"]);
  expect(notices.items).toEqual([]);
});

test.each(["getSettings", "takeNotices", "testLive", "diagLive"])("a rejected %s becomes an Error notice with the error as is", async (name) => {
  ipc.failing = name;
  await start();
  expect(notices.items).toEqual([{ level: "Error", title: S.failed.start, text: `Error: ${name} is down` }]);
});

test("a StateChanged reads the settings again: zapret-hub-cli saves them too", async () => {
  await start();
  ipc.settings = { theme: "dark", cli_enabled: false };
  ipc.log.length = 0;
  ipc.stateChanged();
  expect(ipc.log).toEqual(["getSettings", "getOverview"]);
  await vi.waitFor(() => expect(app.settings).toEqual({ theme: "dark", cli_enabled: false }));
});

test("a tab switch with unsaved edits asks first and stays on «no»", async () => {
  app.tab = "settings";
  app.unsaved = () => true;
  const no = app.setTab("overview");
  expect(confirmation.question?.title).toBe(S.settings.unsavedTitle);
  confirmation.answer(false);
  await no;
  expect(app.tab).toBe("settings");
  const yes = app.setTab("overview");
  confirmation.answer(true);
  await yes;
  expect(app.tab).toBe("overview");
  app.unsaved = () => false;
  await app.setTab("tests");
  expect(confirmation.question).toBeNull();
  expect(app.tab).toBe("tests");
  app.unsaved = null;
});
