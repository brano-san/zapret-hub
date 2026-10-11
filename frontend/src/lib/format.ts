import type { CheckLevel, CheckStatus, DpiStatus, FolderCheck, Info, QuickCheck, Suite } from "./ipc/bindings";
import { S } from "./strings";

export function ms(value: number | null | undefined): string {
  return value == null ? "—" : `${value} мс`;
}

/** `2026-10-10_05-03-34` → `10.10.2026 05:03`. */
export function runDate(id: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})/.exec(id);
  return match ? `${match[3]}.${match[2]}.${match[1]} ${match[4]}:${match[5]}` : id;
}

/** The file stem of a custom strategy, as `CustomStrategy::file_stem` makes it. */
export function hubStem(name: string): string {
  return `general (HUB ${name})`;
}

/** A zapret2 strategy's name is `zapret2:<name>` (zapret2 spec 3.1). */
export const ZAPRET2 = "zapret2:";

/** `mine` for `zapret2:mine`; `null` for a Flowseal strategy. */
export function zapret2Name(name: string): string | null {
  return name.startsWith(ZAPRET2) ? name.slice(ZAPRET2.length) : null;
}

/** A block's card header (spec 6): protocol and ports, l7, the list or domains it is limited to. */
export function blockSummary(block: string): string {
  const pick = (flag: string): string | undefined => {
    const match = new RegExp(`--${flag}=(?:"([^"]*)"|(\\S+))`).exec(block);
    return match?.[1] ?? match?.[2];
  };
  const file = (path: string) => path.replace(/^%\w+%/, "").split(/[\\/]/).pop() ?? path;
  const parts: string[] = [];
  const tcp = pick("filter-tcp");
  const udp = pick("filter-udp");
  const l7 = pick("filter-l7");
  const hostlist = pick("hostlist");
  const ipset = pick("ipset");
  const domains = pick("hostlist-domains");
  if (tcp) parts.push(`TCP ${tcp}`);
  if (udp) parts.push(`UDP ${udp}`);
  if (l7) parts.push(`l7 ${l7}`);
  if (hostlist) parts.push(file(hostlist));
  if (ipset) parts.push(file(ipset));
  if (domains) parts.push(domains);
  return parts.length > 0 ? parts.join(" · ") : S.editor.allTraffic;
}

export function quickSummary(check: QuickCheck): string {
  if (check.service !== "Running") return S.overview.notRunning;
  if (!check.winws_running) return S.overview.winwsMissing;
  return check.checks
    .map((item) => `${item.name} — ${item.status === "Ok" ? ms(item.time_ms) : S.status[item.status]}`)
    .join("\n");
}

/** About 15 s a strategy for the standard suite (spec 14), a little more for DPI. */
export function estimateMinutes(count: number, suite: Suite): number {
  return Math.max(1, Math.round((count * (suite === "Standard" ? 15 : 20)) / 60));
}

export const statusClass: Record<CheckStatus, string> = { Ok: "ok", Ssl: "bad", Unsup: "warn", Error: "bad" };
export const dpiClass: Record<DpiStatus, string> = { Ok: "ok", Fail: "warn", Unsupported: "muted", LikelyBlocked: "bad" };
export const levelClass: Record<CheckLevel, string> = { Ok: "ok", Warn: "warn", Problem: "bad" };

/** The newest run of each suite, Standard first. `runs` come newest first, as `listRuns` returns them. */
export function latestPerSuite<T extends { suite: Suite }>(runs: T[]): T[] {
  return (["Standard", "Dpi"] as const).flatMap((suite) => {
    const run = runs.find((item) => item.suite === suite);
    return run ? [run] : [];
  });
}

/** The text a new zapret2 strategy starts with: one profile, to be edited. */
export const ZAPRET2_TEMPLATE = [
  "# New zapret2 strategy. A line starting with # is a comment.",
  "# Global part: what to intercept and which Lua to load.",
  "--wf-tcp-out=443",
  '--lua-init=@"%LUA%zapret-lib.lua"',
  '--lua-init=@"%LUA%zapret-antidpi.lua"',
  "",
  "# A profile; add more of them, each after --new.",
  "--filter-tcp=443 --filter-l7=tls",
  '--hostlist="%LISTS%list-general.txt"',
  "--out-range=-d10 --payload=tls_client_hello",
  "--lua-desync=multidisorder:pos=midsld",
  "",
].join("\n");

/** The wizard's first step: at least one engine's folder works, and none chosen fails its check. */
export function foldersReady(flowseal: FolderCheck | null, zapret2: Info | null): boolean {
  const flowsealOk = flowseal !== null && flowseal.missing.length === 0;
  const zapret2Ok = zapret2?.dir != null && zapret2.error === null;
  const flowsealBad = flowseal !== null && !flowsealOk;
  const zapret2Bad = zapret2?.dir != null && !zapret2Ok;
  return (flowsealOk || zapret2Ok) && !flowsealBad && !zapret2Bad;
}
