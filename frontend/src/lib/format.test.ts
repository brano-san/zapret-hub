import { describe, expect, it } from "vitest";
import type { Rule, StrategyResult } from "./ipc/bindings";
import { copyStem, blockSummary, bytes, programName, proxyLabel, routeDraft, routeSummary, viaLabel, circularFilter, desyncLabel, estimateMinutes, listChoices, mergeNote, variantLabel, foldersReady, latencySpread, latestPerSuite, localizeError, nameError, ownName, runDate, uniqueName, zapret2Name } from "./format";
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

describe("copyStem", () => {
  it("drops the parentheses a copy's name cannot have", () => {
    expect(copyStem("zapret2:general (ALT5)")).toBe("ALT5");
    expect(copyStem("zapret2:general (Flowseal)")).toBe("Flowseal");
    expect(copyStem("zapret2:mine")).toBe("mine");
    expect(copyStem("general (ALT11)")).toBe("ALT11");
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

describe("circular labels", () => {
  it("say what each instance does, not its arguments", () => {
    expect(desyncLabel("fake:blob=tls_google:tcp_ts=-600000:repeats=8")).toBe("fake tls_google ×8");
    expect(desyncLabel("multisplit:pos=1:seqovl=681:seqovl_pattern=tls_google")).toBe("multisplit 1 seqovl");
    expect(desyncLabel("multidisorder:pos=midsld")).toBe("multidisorder midsld");
    expect(variantLabel("--payload=tls_client_hello\n--lua-desync=fake:blob=tls_google:repeats=8\n# a note\n--lua-desync=multisplit")).toBe(
      "fake tls_google ×8 + multisplit",
    );
    expect(variantLabel("# nothing yet")).toBe(S.circular.emptyVariant);
  });

  it("offer the usual lists and the strategy's own, once each", () => {
    const choices = listChoices(['--filter-tcp=443 --hostlist="%LISTS%list-google.txt" --hostlist-exclude="%LISTS%x.txt"', "--ipset=my.txt"]);
    expect(choices.map(([, label]) => label)).toEqual(["list-general.txt", "list-google.txt", "ipset-all.txt", "my.txt"]);
    expect(circularFilter(" 80, 443", "tls,http", choices[0]![0])).toBe('--filter-tcp=80,443 --filter-l7=tls,http --hostlist="%LISTS%list-general.txt"');
    expect(circularFilter("443", "", "")).toBe("--filter-tcp=443");
  });

  it("explain each merged profile", () => {
    const note = { kind: "Missing", number: 3, filter: "--filter-tcp=443 --hostlist-domains=discord.media", names: ["mine"], variants: 0 } as const;
    expect(mergeNote({ ...note, names: [...note.names] }, "ALT11")).toBe("Профиль 3 · TCP 443 · discord.media: нет в mine — взят из ALT11");
    expect(mergeNote({ ...note, kind: "Rotates", names: ["ALT11", "mine"], variants: 2 }, "ALT11")).toBe(
      "Профиль 3 · TCP 443 · discord.media: circular из 2 вариантов (ALT11, mine)",
    );
  });
});

describe("routes", () => {
  const voice = { name: "Discord голос", enabled: true, protocol: "udp", ports: ["19294-19344", "50000-65535"], ips: ["104.29.0.0/16", "35.217.0.0/16"], domains: [], apps: [] } as const;
  it("sums a rule up by protocol and subnets", () => {
    expect(routeSummary({ ...voice, ports: [...voice.ports], ips: [...voice.ips], domains: [], apps: [] })).toBe("UDP · 2 подсети");
    expect(routeSummary({ ...voice, ports: [], ips: ["1.1.1.1"], domains: [], apps: [] })).toBe("UDP · 1 подсеть");
    expect(routeSummary({ ...voice, ports: [], ips: ["1", "2", "3", "4", "5"], domains: [], apps: [] })).toBe("UDP · 5 подсетей");
  });
  it("sums a rule up by protocol, subnets and domains", () => {
    const rule = (over: Partial<Rule>): Rule => ({ ...voice, ports: [...voice.ports], ips: [...voice.ips], domains: [], apps: [], ...over });
    expect(routeSummary(rule({ domains: ["discord.media"] }))).toBe("UDP · 2 подсети · 1 домен");
    expect(routeSummary(rule({ protocol: "both", ips: [], domains: ["a.com", "b.com", "c.com", "d.com", "e.com"] }))).toBe("TCP/UDP · 5 доменов");
    expect(routeSummary(rule({ protocol: "tcp", ips: [], domains: ["a.com", "b.com"] }))).toBe("TCP · 2 домена");
  });
  it("names the TCP direct reasons", () => {
    expect(viaLabel({ via: "Direct", reason: { kind: "Marked" } })).toBe("напрямую: прокси недавно не открыл это соединение");
    expect(viaLabel({ via: "Direct", reason: { kind: "NotAccepted" } })).toBe("напрямую: брандмауэр Windows не пропустил соединение к хабу");
  });
  it("saves what the editor shows, domains included", () => {
    const form = { name: " Discord целиком ", enabled: true, protocol: "both", ports: "", ips: "", domains: " discord.com\n\ndiscord.media \n", apps: "" } as const;
    expect(routeDraft(form)).toEqual({ name: "Discord целиком", enabled: true, protocol: "both", ports: [], ips: [], domains: ["discord.com", "discord.media"], apps: [] });
    expect(routeDraft({ ...form, ips: "1.1.1.0/24\r\n2.2.2.2", domains: "" })).toMatchObject({ ips: ["1.1.1.0/24", "2.2.2.2"], domains: [] });
  });
  it("counts programs and names a program by its file", () => {
    const dbd: Rule = { name: "Dead by Daylight", enabled: true, protocol: "both", ports: [], ips: [], domains: ["bhvrdbd.com"], apps: ["DeadByDaylight-Win64-Shipping.exe"] };
    expect(routeSummary(dbd)).toBe("TCP/UDP · 1 домен · 1 программа");
    expect(routeSummary({ ...dbd, domains: [], apps: ["a.exe", "b.exe"] })).toBe("TCP/UDP · 2 программы");
    expect(programName("C:\\Games\\DbD\\DeadByDaylight-Win64-Shipping.exe")).toBe("DeadByDaylight-Win64-Shipping.exe");
    expect(programName(null)).toBe("—");
    expect(viaLabel({ via: "Direct", reason: { kind: "NoRule" } })).toBe("напрямую: нет правила");
  });
  it("counts bytes the Russian way", () => {
    expect(bytes(512)).toBe("512 Б");
    expect(bytes(1536)).toBe("1,5 КБ");
    expect(bytes(3 * 1024 * 1024)).toBe("3,0 МБ");
    expect(bytes(2 * 1024 ** 3)).toBe("2,0 ГБ");
  });
  it("says how a flow goes", () => {
    expect(viaLabel({ via: "Proxy" })).toBe("прокси");
    expect(viaLabel({ via: "Opening" })).toBe("открывается");
    expect(viaLabel({ via: "Direct", reason: { kind: "ProxyDown" } })).toBe("напрямую: прокси недоступен");
    expect(viaLabel({ via: "Direct", reason: { kind: "Failed", error: "connection reset" } })).toBe("напрямую: connection reset");
  });
  it("says how the proxy is", () => {
    expect(proxyLabel({ state: "Ok", latency_ms: 4 })).toBe("прокси отвечает · 4 мс");
    expect(proxyLabel({ state: "Down", error: "refused" })).toBe("прокси недоступен, трафик идёт напрямую");
  });
});
