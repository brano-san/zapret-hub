import type { CheckLevel, CheckStatus, DpiStatus, FolderCheck, Info, MergeNote, Protocol, ProxyState, QuickCheck, Rule, StrategyResult, Suite, Traffic, Via } from "./ipc/bindings";
import { plural, S } from "./strings";

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

/** The stem of a copy's name: `ALT5` for `zapret2:general (ALT5)` (a name with parentheses is refused), else the name without the prefix. */
export function copyStem(name: string): string {
  const own = zapret2Name(name) ?? name;
  return /\((.+)\)$/.exec(own)?.[1] ?? own;
}

/** `"%LISTS%list-general.txt"` → `list-general.txt`. */
function file(path: string): string {
  return path.replace(/"/g, "").replace(/^%\w+%/, "").split(/[\\/]/).pop() ?? path;
}

/** A block's card header (spec 6): protocol and ports, l7, the list or domains it is limited to. */
export function blockSummary(block: string): string {
  const pick = (flag: string): string | undefined => {
    const match = new RegExp(`--${flag}=(?:"([^"]*)"|(\\S+))`).exec(block);
    return match?.[1] ?? match?.[2];
  };
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

/** About 15 s a strategy and pass for the standard suite (spec 14), a little more for DPI. */
export function estimateMinutes(count: number, suite: Suite, repeats = 1): number {
  return Math.max(1, Math.round((count * repeats * (suite === "Standard" ? 15 : 20)) / 60));
}

/** The choices of «Повторов» (`tester::run::MAX_REPEATS`). */
export const REPEATS = [1, 2, 3, 4, 5] as const;

/** The lowest and highest of the passes' median latencies; `null` unless two passes had one. */
export function latencySpread(result: StrategyResult): { min: number; max: number } | null {
  const values = (result.passes ?? []).map((pass) => pass.score.median_ms).filter((value) => value !== null);
  return values.length < 2 ? null : { min: Math.min(...values), max: Math.max(...values) };
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

const DEVICE = /^(CON|PRN|AUX|NUL|COM\d|LPT\d)$/i;

/** `zapret::custom::validate_name` for the page, in Russian: `null` for a name it accepts. */
export function nameError(name: string): string | null {
  const trimmed = name.trim();
  if (trimmed.length === 0) return S.names.empty;
  if (trimmed.length > 40 || !/^[A-Za-z0-9][A-Za-z0-9 ._-]*$/.test(trimmed)) return S.names.invalid;
  if (DEVICE.test((trimmed.split(".")[0] ?? "").trimEnd())) return S.names.device;
  return null;
}

/** A name `nameError` accepts, made of `text` (`general (ALT11) copy` → `general ALT11 copy`) and not in `taken` (any case). */
export function uniqueName(text: string, taken: string[]): string {
  const base =
    text
      .replace(/[^A-Za-z0-9 ._-]+/g, " ")
      .replace(/\s+/g, " ")
      .trim()
      .replace(/^[^A-Za-z0-9]+/, "") || "strategy";
  const used = new Set(taken.map((name) => name.toLowerCase()));
  for (let n = 1; ; n++) {
    const suffix = n === 1 ? "" : ` ${n}`;
    const name = base.slice(0, 40 - suffix.length).trimEnd() + suffix;
    if (!used.has(name.toLowerCase()) && nameError(name) === null) return name;
  }
}

/** The name inside a strategy's own name, for `uniqueName`: `mine` for `zapret2:mine` and `general (HUB mine)`. */
export function ownName(name: string): string {
  return zapret2Name(name) ?? /\(HUB (.+)\)$/.exec(name)?.[1] ?? name;
}

/** A known error of the app, said in Russian above the original; the rest as written. */
export function localizeError(text: string): string {
  if (text.includes("a name is 1-40")) return `${S.names.invalid}\n\n${text}`;
  if (text.includes("is a Windows device name")) return `${S.names.device}\n\n${text}`;
  // `zapret::release::DEFENDER`.
  if (text.includes("blocked or removed by Windows Defender")) return `${S.download.defender}\n\n${text}`;
  return text;
}

/** `fake:blob=tls_google:tcp_ts=-600000:repeats=8` → `fake tls_google ×8`: the function, its blob, the cut and how many times. */
export function desyncLabel(call: string): string {
  const [name = "", ...args] = call.split(":");
  const value = (key: string) => args.find((arg) => arg.startsWith(`${key}=`))?.slice(key.length + 1);
  const parts = [name];
  const blob = value("blob");
  if (blob) parts.push(blob);
  const pos = value("pos");
  if (pos && /split|disorder/.test(name)) parts.push(pos);
  if (value("seqovl")) parts.push("seqovl");
  const repeats = value("repeats");
  if (repeats) parts.push(`×${repeats}`);
  return parts.join(" ");
}

/** A circular variant's text (`--payload=` and `--lua-desync=` lines) as `fake tls_google ×8 + multisplit 1 seqovl`. */
export function variantLabel(text: string): string {
  const calls = text
    .split("\n")
    .filter((line) => !line.trim().startsWith("#"))
    .flatMap((line) => line.trim().split(/\s+/))
    .map((token) => token.replace(/"/g, ""))
    .filter((token) => token.startsWith("--lua-desync="))
    .map((token) => desyncLabel(token.slice("--lua-desync=".length)));
  return calls.length > 0 ? calls.join(" + ") : S.circular.emptyVariant;
}

/** The builder's ready instances. The blobs tls_google and stun2 are added to a text that lacks them. */
export const CIRCULAR_PRESETS = [
  { label: "fake TLS google", line: "--lua-desync=fake:blob=tls_google:tcp_ts=-600000:repeats=8" },
  { label: "fake stun2", line: "--lua-desync=fake:blob=stun2:tcp_ts=-600000:repeats=8" },
  { label: "multisplit seqovl", line: "--lua-desync=multisplit:pos=1:seqovl=681:seqovl_pattern=tls_google" },
  { label: "multidisorder midsld", line: "--lua-desync=multidisorder:pos=midsld" },
  { label: "fakedsplit", line: "--lua-desync=fakedsplit:pos=midsld:tcp_md5" },
] as const;

/** What a new circular profile starts with: ALT11's TLS desync, then multidisorder. */
export const CIRCULAR_START = [`${CIRCULAR_PRESETS[0].line}\n${CIRCULAR_PRESETS[2].line}`, CIRCULAR_PRESETS[3].line];

/** The lists a new profile can be limited to: Flowseal's usual ones and every `--hostlist=`/`--ipset=` the profiles use, as `[token, label]`. */
export function listChoices(filters: string[]): [string, string][] {
  const tokens = [
    '--hostlist="%LISTS%list-general.txt"',
    '--hostlist="%LISTS%list-google.txt"',
    '--ipset="%LISTS%ipset-all.txt"',
    ...filters.flatMap((filter) => filter.split(/\s+/).filter((token) => /^--(hostlist|ipset)=/.test(token))),
  ];
  return [...new Set(tokens)].map((token) => [token, file(token.slice(token.indexOf("=") + 1))]);
}

/** The filter of a new circular profile: TCP `ports`, `l7` (empty for any) and a list token. */
export function circularFilter(ports: string, l7: string, list: string): string {
  return [`--filter-tcp=${ports.replace(/\s+/g, "")}`, l7 ? `--filter-l7=${l7}` : "", list].filter(Boolean).join(" ");
}

/** A merge note in words; `first` is the strategy the rest comes from. */
export function mergeNote(note: MergeNote, first: string): string {
  const where = S.circular.profile(note.number, blockSummary(note.filter));
  const text = S.circular.notes;
  switch (note.kind) {
    case "Rotates":
      return text.Rotates(where, note.names, note.variants);
    case "Same":
      return text.Same(where);
    case "BlobConflict":
      return text.BlobConflict(note.filter, note.names, first);
    default:
      return text[note.kind](where, note.names, first);
  }
}

/** A rule's line in the list: `UDP · 2 подсети · 1 домен`. */
export function routeSummary(rule: Rule): string {
  const parts = [rule.protocol === "both" ? "TCP/UDP" : rule.protocol.toUpperCase()];
  const subnets = rule.ips.length;
  const domains = rule.domains.length;
  if (subnets > 0) parts.push(`${subnets} ${plural(subnets, ["подсеть", "подсети", "подсетей"])}`);
  if (domains > 0) parts.push(`${domains} ${plural(domains, ["домен", "домена", "доменов"])}`);
  const apps = rule.apps.length;
  if (apps > 0) parts.push(`${apps} ${plural(apps, ["программа", "программы", "программ"])}`);
  return parts.join(" · ");
}

/** The non-empty trimmed lines of a text field. */
export const lines = (text: string): string[] =>
  text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);

/** The rule the editor's fields make, as it is checked and saved. */
export function routeDraft(form: { name: string; enabled: boolean; protocol: Protocol; ports: string; ips: string; domains: string; apps: string }): Rule {
  return { name: form.name.trim(), enabled: form.enabled, protocol: form.protocol, ports: lines(form.ports), ips: lines(form.ips), domains: lines(form.domains), apps: lines(form.apps) };
}

/** The file name of a program path; `—` when unknown. */
export const programName = (path: string | null): string => (path ? (path.split(/[\\/]/).pop() ?? path) : "—");

/** `1536` → `1,5 КБ`. */
export function bytes(n: number): string {
  const units = ["Б", "КБ", "МБ", "ГБ", "ТБ"];
  let unit = 0;
  while (n >= 1024 && unit < units.length - 1) {
    n /= 1024;
    unit++;
  }
  return unit === 0 ? `${n} ${units[0]}` : `${n.toFixed(1).replace(".", ",")} ${units[unit]}`;
}

/** `3 потока · ↑ 1,5 КБ ↓ 2,0 МБ`. */
export function trafficLabel(traffic: Traffic): string {
  return `${S.routes.flowCount(traffic.flows)} · ↑ ${bytes(traffic.up ?? 0)} ↓ ${bytes(traffic.down ?? 0)}`;
}

export function viaLabel(via: Via): string {
  if (via.via === "Proxy") return S.routes.viaProxy;
  if (via.via === "Opening") return S.routes.viaOpening;
  const reason = via.reason;
  return S.routes.viaDirect(reason.kind === "Failed" ? reason.error : S.routes.reasons[reason.kind]);
}

export function proxyLabel(proxy: ProxyState): string {
  return proxy.state === "Ok" ? S.routes.proxy.Ok(proxy.latency_ms) : S.routes.proxy[proxy.state];
}
