import { commands, events, type DiagLive, type LiveRun, type Overview, type Settings, type Theme } from "./ipc/bindings";
import { call, type Answer } from "./api";
import { notices } from "./notices.svelte";
import { S } from "./strings";

export type Tab = "overview" | "strategies" | "tests" | "zapret" | "routes" | "diag" | "settings";

class App {
  tab = $state<Tab>("overview");
  settings = $state<Settings | null>(null);
  overview = $state<Overview | null>(null);
  /** Bumped by every `StateChanged`: tabs read what they show again when it moves. */
  revision = $state(0);
  live = $state<LiveRun | null>(null);
  /** The run that just finished, for the Tests tab to open. */
  finished = $state<string | null>(null);
  /** Running diagnostics, as `diagLive` last told. */
  diagLive = $state<DiagLive | null>(null);
  /** The report of the diagnostics that just finished, for the Diag tab to open. */
  diagFinished = $state<string | null>(null);
  pending = $state(0);

  /** A command the user waits on is running: the buttons that change zapret stay off. */
  get busy(): boolean {
    return this.pending > 0;
  }
}

export const app = new App();

const darkScheme = matchMedia("(prefers-color-scheme: dark)");

export function applyTheme(theme: Theme): void {
  const dark = theme === "dark" || (theme === "system" && darkScheme.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

darkScheme.addEventListener("change", () => applyTheme(app.settings?.theme ?? "system"));

/** `call` for a command the user waits on. */
export async function change<T>(title: string, run: () => Promise<Answer<T>>): Promise<T | undefined> {
  app.pending++;
  try {
    return await call(title, run());
  } finally {
    app.pending--;
  }
}

export async function loadSettings(): Promise<void> {
  app.settings = await commands.getSettings();
  // `#[serde(default)]` makes every Settings field optional in the generated types.
  applyTheme(app.settings.theme ?? "system");
}

export async function refreshOverview(): Promise<void> {
  const overview = await call(S.failed.overview, commands.getOverview());
  if (overview) app.overview = overview;
}

let finishes = 0;

/** A `testLive` answer that crossed `TestsFinished` on the way is stale: it is dropped. */
export async function refreshLive(): Promise<void> {
  const seen = finishes;
  const live = await commands.testLive();
  if (seen === finishes) app.live = live;
}

let diagFinishes = 0;

/** As `refreshLive`: an answer that crossed `DiagFinished` is stale. */
export async function refreshDiag(): Promise<void> {
  const seen = diagFinishes;
  const live = await commands.diagLive();
  if (seen === diagFinishes) app.diagLive = live;
}

async function pullNotices(): Promise<void> {
  for (const notice of await commands.takeNotices()) notices.push(notice);
}

/** Once, from `App.svelte`: listeners first, so nothing that happens meanwhile is missed. */
export async function start(): Promise<void> {
  try {
    await events.stateChanged.listen(() => {
      app.revision++;
      // zapret-hub-cli saves settings too: the window reads them again (CLI spec 8).
      void loadSettings();
      void refreshOverview();
    });
    await events.noticesChanged.listen(() => void pullNotices());
    await events.testProgress.listen(() => void refreshLive());
    await events.testsFinished.listen(({ payload }) => {
      finishes++;
      app.live = null;
      app.finished = payload.run_id;
    });
    await events.diagProgress.listen(() => void refreshDiag());
    await events.diagFinished.listen(({ payload }) => {
      diagFinishes++;
      app.diagLive = null;
      app.diagFinished = payload.id;
    });
    await loadSettings();
    await Promise.all([pullNotices(), refreshOverview(), refreshLive(), refreshDiag()]);
  } catch (err) {
    // `listen`, `getSettings`, `takeNotices`, `testLive` and `diagLive` return no `Answer`: they reject only when the IPC is broken.
    notices.push({ level: "Error", title: S.failed.start, text: String(err) });
  }
}
