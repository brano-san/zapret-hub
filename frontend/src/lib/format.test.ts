import { describe, expect, it } from "vitest";
import { blockSummary, foldersReady, latestPerSuite, runDate, zapret2Name } from "./format";

describe("blockSummary", () => {
  it("names the protocol, the ports and what the block is limited to", () => {
    expect(
      blockSummary('--filter-udp=443 --hostlist="%LISTS%list-general.txt" --hostlist-exclude="%LISTS%list-exclude.txt" --dpi-desync=fake'),
    ).toBe("UDP 443 · list-general.txt");
    expect(blockSummary("--filter-udp=19294-19344,50000-50100 --filter-l7=discord,stun --dpi-desync=fake")).toBe(
      "UDP 19294-19344,50000-50100 · l7 discord,stun",
    );
    expect(blockSummary("--filter-tcp=443 --hostlist-domains=discord.media --dpi-desync=multisplit")).toBe(
      "TCP 443 · discord.media",
    );
    expect(blockSummary("--dpi-desync=fake")).toBe("все пакеты");
  });
});

describe("zapret2Name", () => {
  it("tells zapret2 strategies by their prefix", () => {
    expect(zapret2Name("zapret2:general-sni")).toBe("general-sni");
    expect(zapret2Name("general (ALT11)")).toBeNull();
  });
});

describe("runDate", () => {
  it("turns a run id into a date and a time", () => {
    expect(runDate("2026-10-10_05-03-34")).toBe("10.10.2026 05:03");
    expect(runDate("odd")).toBe("odd");
  });
});

describe("latestPerSuite", () => {
  it("keeps the newest run of each suite, whichever ran last", () => {
    const runs = [
      { id: "c", suite: "Dpi" as const },
      { id: "b", suite: "Standard" as const },
      { id: "a", suite: "Dpi" as const },
    ];
    expect(latestPerSuite(runs).map((run) => run.id)).toEqual(["b", "c"]);
    expect(latestPerSuite(runs.slice(0, 1)).map((run) => run.id)).toEqual(["c"]);
    expect(latestPerSuite([])).toEqual([]);
  });
});

describe("foldersReady", () => {
  const flowseal = (missing: string[]) => ({ path: "E:\Zapret", missing, version: null });
  const zapret2 = (error: string | null) => ({ dir: "D:\zapret2", version: null, compat: null, error });

  it("needs at least one working folder", () => {
    expect(foldersReady(null, null)).toBe(false);
    expect(foldersReady(flowseal([]), null)).toBe(true);
    expect(foldersReady(null, zapret2(null))).toBe(true);
    expect(foldersReady(flowseal([]), zapret2(null))).toBe(true);
  });

  it("refuses a chosen folder that fails its check, even next to a good one", () => {
    expect(foldersReady(flowseal(["bin"]), zapret2(null))).toBe(false);
    expect(foldersReady(flowseal([]), zapret2("missing winws2.exe"))).toBe(false);
    expect(foldersReady(null, { dir: null, version: null, compat: null, error: null })).toBe(false);
  });
});
