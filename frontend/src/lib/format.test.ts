import { describe, expect, it } from "vitest";
import { blockSummary, runDate, zapret2Name } from "./format";

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
