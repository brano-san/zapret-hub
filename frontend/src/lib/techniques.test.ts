import { describe, expect, it } from "vitest";
import { detectTechniques, summarizeServices, techniqueColor } from "./techniques";

describe("detectTechniques", () => {
  it("detects Fake TLS and Multisplit", () => {
    const text =
      "--filter-tcp=443 --filter-l7=tls --lua-desync=fake:blob=tls_google:repeats=8 --lua-desync=multisplit:pos=1";
    const techs = detectTechniques(text);
    expect(techs).toContain("Fake TLS");
    expect(techs).toContain("Multisplit");
  });

  it("detects Fake QUIC", () => {
    const text = "--filter-udp=443 --filter-l7=quic --lua-desync=fake:blob=quic_google:repeats=11";
    const techs = detectTechniques(text);
    expect(techs).toContain("Fake QUIC");
  });

  it("detects Disorder and Syndata", () => {
    const text = "--lua-desync=multidisorder:pos=1,midsld --lua-desync=syndata";
    const techs = detectTechniques(text);
    expect(techs).toContain("Disorder");
    expect(techs).toContain("Syndata");
  });

  it("returns colors for every technique", () => {
    expect(techniqueColor("Fake TLS").border).toBe("#3b82f6");
    expect(techniqueColor("Multisplit").border).toBe("#8b5cf6");
  });
});

describe("summarizeServices", () => {
  it("summarizes typical zapret2 profiles", () => {
    const blocks = [
      '--filter-tcp=443 --hostlist="list-google.txt" --lua-desync=fake:blob=tls_google --lua-desync=multisplit:pos=1',
      "--filter-udp=50000-50100 --filter-l7=discord,stun --lua-desync=fake:blob=discord_udp",
      '--filter-tcp=80,443 --ipset="ipset-all.txt" --lua-desync=fake:blob=tls_max',
    ];
    const summaries = summarizeServices(blocks);
    expect(summaries.length).toBe(3);
    expect(summaries[0]?.title).toBe("YouTube & Google");
    expect(summaries[0]?.techniques).toContain("Fake TLS");
    expect(summaries[0]?.techniques).toContain("Multisplit");
    expect(summaries[1]?.title).toBe("Discord");
    expect(summaries[2]?.title).toBe("Прочие ресурсы (ipset/general)");
  });
});
