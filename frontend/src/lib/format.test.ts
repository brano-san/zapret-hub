import { describe, expect, it } from "vitest";
import type { StrategyResult } from "./ipc/bindings";
import { blockSummary, estimateMinutes, foldersReady, latencySpread, latestPerSuite, localizeError, nameError, ownName, runDate, uniqueName, zapret2Name } from "./format";
import { S } from "./strings";

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

describe("nameError", () => {
  it("accepts what validate_name accepts", () => {
    for (const name of ["mine", "general ALT11 copy", " a.b-c_d ", "x".repeat(40), "CONSOLE", "COM10", "con2"]) {
      expect(nameError(name), name).toBeNull();
    }
  });

  it("refuses the rest, in Russian", () => {
    expect(nameError("  ")).toBe(S.names.empty);
    for (const name of ["general (ALT11)", "-mine", "x".repeat(41), "a/b", "..\\evil", "имя"]) {
      expect(nameError(name), name).toBe(S.names.invalid);
    }
    for (const name of ["con", "NUL.txt", "com1", "Lpt9 .x"]) expect(nameError(name), name).toBe(S.names.device);
  });
});

describe("uniqueName", () => {
  it("makes a valid name of a Flowseal one", () => {
    expect(uniqueName("general (ALT11) copy", [])).toBe("general ALT11 copy");
    expect(uniqueName("(x) copy", [])).toBe("x copy");
    expect(uniqueName("!!!", [])).toBe("strategy");
  });

  it("stays within 40 and away from taken names", () => {
    const long = uniqueName(`${"a".repeat(50)} copy`, []);
    expect(long).toBe("a".repeat(40));
    expect(uniqueName("general copy", ["General Copy", "general copy 2"])).toBe("general copy 3");
    const taken = uniqueName(`${"a".repeat(50)}`, ["a".repeat(40)]);
    expect(taken).toBe(`${"a".repeat(38)} 2`);
    expect(nameError(taken)).toBeNull();
  });

  it("knows the own name inside a strategy's", () => {
    expect(ownName("zapret2:mine")).toBe("mine");
    expect(ownName("general (HUB my one)")).toBe("my one");
    expect(ownName("general (ALT11)")).toBe("general (ALT11)");
  });
});

describe("localizeError", () => {
  it("puts a Russian line over a known name error", () => {
    const raw = "a name is 1-40 Latin letters, digits, spaces, '-', '_' or '.', starting with a letter or digit: general (ALT11)";
    expect(localizeError(raw)).toBe(`${S.names.invalid}\n\n${raw}`);
    expect(localizeError("something else")).toBe("something else");
  });

  it("explains an antivirus block of a download", () => {
    const raw = "add the folder D:\\zapret2-v0.9.4 to the antivirus exclusions and download again: x.zip: blocked or removed by Windows Defender or another antivirus";
    expect(localizeError(raw)).toBe(`${S.download.defender}\n\n${raw}`);
  });
});

describe("repeats", () => {
  const score = (median_ms: number | null) => ({ median_ms }) as StrategyResult["score"];
  const result = (medians: (number | null)[]) =>
    ({ score: score(null), passes: medians.map((m) => ({ score: score(m) })) }) as StrategyResult;

  it("spreads the passes' medians, ignoring passes without one", () => {
    expect(latencySpread(result([180, 120, null, 240]))).toEqual({ min: 120, max: 240 });
    expect(latencySpread(result([180, null]))).toBeNull();
    expect(latencySpread({ score: score(100) } as StrategyResult)).toBeNull();
  });

  it("multiplies the estimate by the passes", () => {
    expect(estimateMinutes(8, "Standard")).toBe(2);
    expect(estimateMinutes(8, "Standard", 3)).toBe(6);
  });
});
