/** All techniques, in the order the tab's tag filter lists them. */
export const TECHNIQUES: readonly Technique[] = [
  "Fake TLS",
  "Multisplit",
  "Fake QUIC",
  "Disorder",
  "Syndata",
  "Fake UDP",
];

/**
 * Parses DPI circumvention techniques from strategy configurations.
 */

export type Technique = "Fake TLS" | "Multisplit" | "Fake QUIC" | "Disorder" | "Syndata" | "Fake UDP";

export interface ServiceSummary {
  icon: string;
  title: string;
  details: string;
  techniques: string[];
}

/**
 * Detects circumvention techniques present in an argument string or full strategy text.
 */
export function detectTechniques(text: string): Technique[] {
  if (!text) return [];
  const lower = text.toLowerCase();
  const techniques: Technique[] = [];

  const hasFake = lower.includes("desync=fake") || lower.includes("fake:");
  const hasMultisplit =
    lower.includes("multisplit") ||
    lower.includes("fakedsplit") ||
    lower.includes("fakesplit") ||
    lower.includes("desync=split2");
  const hasDisorder = lower.includes("multidisorder") || lower.includes("desync=disorder");
  const hasSyndata = lower.includes("syndata");

  const hasTls =
    lower.includes("tls") ||
    lower.includes("tls_client_hello") ||
    lower.includes("tls_clienthello") ||
    lower.includes("tls_google") ||
    lower.includes("tls_max");
  const hasQuic =
    lower.includes("quic") ||
    lower.includes("quic_initial") ||
    lower.includes("quic_google");
  const hasUdp =
    lower.includes("discord_udp") ||
    lower.includes("game_udp") ||
    lower.includes("discord_ip_discovery") ||
    (hasFake && lower.includes("filter-udp"));

  if (hasFake && hasTls) {
    techniques.push("Fake TLS");
  }
  if (hasMultisplit) {
    techniques.push("Multisplit");
  }
  if (hasFake && hasQuic) {
    techniques.push("Fake QUIC");
  }
  if (hasDisorder) {
    techniques.push("Disorder");
  }
  if (hasSyndata) {
    techniques.push("Syndata");
  }
  if (hasFake && hasUdp && !techniques.includes("Fake QUIC")) {
    techniques.push("Fake UDP");
  } else if (hasFake && (lower.includes("discord_udp") || lower.includes("game_udp"))) {
    techniques.push("Fake UDP");
  }

  return techniques;
}

/**
 * Returns color style classes or styles for a technique tag.
 */
export function techniqueColor(tech: Technique): { bg: string; border: string; text: string } {
  switch (tech) {
    case "Fake TLS":
      return { bg: "rgba(59, 130, 246, 0.15)", border: "#3b82f6", text: "#60a5fa" };
    case "Multisplit":
      return { bg: "rgba(139, 92, 246, 0.15)", border: "#8b5cf6", text: "#a78bfa" };
    case "Fake QUIC":
      return { bg: "rgba(245, 158, 11, 0.15)", border: "#f59e0b", text: "#fbbf24" };
    case "Disorder":
      return { bg: "rgba(16, 185, 129, 0.15)", border: "#10b981", text: "#34d399" };
    case "Syndata":
      return { bg: "rgba(236, 72, 153, 0.15)", border: "#ec4899", text: "#f472b6" };
    case "Fake UDP":
      return { bg: "rgba(20, 184, 166, 0.15)", border: "#14b8a6", text: "#2dd4bf" };
  }
}

function extractPorts(block: string): string {
  const tcpMatch = /--filter-tcp=([^\s"]+)/.exec(block);
  const udpMatch = /--filter-udp=([^\s"]+)/.exec(block);
  const parts: string[] = [];
  if (tcpMatch?.[1]) parts.push(`TCP ${tcpMatch[1].replace(/%GameFilterTCP%/g, "игры")}`);
  if (udpMatch?.[1]) parts.push(`UDP ${udpMatch[1].replace(/%GameFilterUDP%/g, "игры")}`);
  return parts.join(", ");
}

/**
 * Analyzes strategy blocks and produces human-friendly service summaries.
 */
export function summarizeServices(blocks: string[]): ServiceSummary[] {
  if (!blocks || blocks.length === 0) return [];

  const summaries: ServiceSummary[] = [];

  // Group blocks by service
  const youtubeBlocks = blocks.filter((b) => {
    const l = b.toLowerCase();
    return l.includes("list-google.txt") || l.includes("googlevideo") || l.includes("youtube") || l.includes("tls_google");
  });

  const discordBlocks = blocks.filter((b) => {
    const l = b.toLowerCase();
    return l.includes("discord") || l.includes("19294-19344") || l.includes("50000-50100");
  });

  const ipsetBlocks = blocks.filter((b) => {
    const l = b.toLowerCase();
    return (
      (l.includes("list-general.txt") || l.includes("ipset-all.txt") || l.includes("ipset")) &&
      !youtubeBlocks.includes(b) &&
      !discordBlocks.includes(b)
    );
  });

  const gameBlocks = blocks.filter((b) => {
    const l = b.toLowerCase();
    return l.includes("gamefilter") || l.includes("game_udp");
  });

  if (youtubeBlocks.length > 0) {
    const text = youtubeBlocks.join(" ");
    const techs = detectTechniques(text);
    const ports = extractPorts(text) || "порты 80, 443";
    summaries.push({
      icon: "🎬",
      title: "YouTube & Google",
      details: `${techs.length > 0 ? techs.join(" + ") : "Обход DPI"} (${ports})`,
      techniques: techs,
    });
  }

  if (discordBlocks.length > 0) {
    const text = discordBlocks.join(" ");
    const techs = detectTechniques(text);
    const ports = extractPorts(text) || "голос и медиа";
    summaries.push({
      icon: "🎧",
      title: "Discord",
      details: `${techs.length > 0 ? techs.join(" + ") : "Обход DPI"} (${ports})`,
      techniques: techs,
    });
  }

  if (ipsetBlocks.length > 0) {
    const text = ipsetBlocks.join(" ");
    const techs = detectTechniques(text);
    const ports = extractPorts(text) || "веб-порты";
    summaries.push({
      icon: "⚡",
      title: "Прочие ресурсы (ipset/general)",
      details: `${techs.length > 0 ? techs.join(" + ") : "Обход DPI"} (${ports})`,
      techniques: techs,
    });
  }

  if (gameBlocks.length > 0) {
    const text = gameBlocks.join(" ");
    const techs = detectTechniques(text);
    summaries.push({
      icon: "🎮",
      title: "Игры (Game Filter)",
      details: `${techs.length > 0 ? techs.join(" + ") : "Обход DPI"} (порты фильтра)`,
      techniques: techs,
    });
  }

  // Fallback if no specific categories were found
  if (summaries.length === 0) {
    for (let i = 0; i < blocks.length; i++) {
      const b = blocks[i]!;
      const techs = detectTechniques(b);
      const ports = extractPorts(b) || "все порты";
      summaries.push({
        icon: "🛡️",
        title: `Профиль ${i + 1}`,
        details: `${techs.length > 0 ? techs.join(" + ") : "Обход DPI"} (${ports})`,
        techniques: techs,
      });
    }
  }

  return summaries;
}
