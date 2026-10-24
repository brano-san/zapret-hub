//! Linux: the port filters `nfqws2` cannot do itself. `--wf-tcp-out`/`--wf-udp-out` are WinDivert
//! options; on Linux an nftables table takes their place (zapret2 readme, «Механика обработки
//! трафика»): it sends the strategy's ports into queue `--qnum=200`, leaves out loopback,
//! `nfqws2`'s own marked packets, the excluded ports and every process of the `zapret-bypass`
//! group. The `zapret2` unit applies the table file before the engine and deletes the table after
//! it (`zapret::service`); the tests apply it themselves.

use std::path::Path;
use std::process::Command;
use std::time::Duration;

use anyhow::Context as _;

pub use zapret::procs::queue::QNUM;
pub use zapret::service::NFT_CONFIG as CONFIG;

/// NFQWS2_COMPAT: the mark `nfqws2` puts on its own packets (its default), kept out of the queue.
const FWMARK: &str = "0x40000000";

/// The group the table lets through whole: a program started in it (`sg zapret-bypass <program>`)
/// leaves the queue alone. `nft` names a group of `/etc/group`, so it has to exist.
pub const GROUP: &str = "zapret-bypass";

/// What `tail` becomes for `nfqws2`, and the ports its queue takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Translated {
    /// The arguments without any `--wf-*` (WinDivert's whole filter family), with `--qnum=200`
    /// unless the strategy named a queue.
    pub args: String,
    /// The `--wf-tcp-out=` ports, merged: `80,443,1024-65535`.
    pub tcp: String,
    /// The `--wf-udp-out=` ports, merged.
    pub udp: String,
}

/// `tail` as `nfqws2` reads it. Every port must be a port or a range: they go into a file root
/// applies.
pub fn translate(tail: &str) -> anyhow::Result<Translated> {
    let (mut tcp, mut udp, mut kept) = (Vec::new(), Vec::new(), Vec::new());
    for token in zapret::quoted_tokens(tail) {
        let plain = token.replace('"', "");
        if let Some(ports) = plain.strip_prefix("--wf-tcp-out=") {
            tcp.extend(ports.split(',').map(str::to_owned));
        } else if let Some(ports) = plain.strip_prefix("--wf-udp-out=") {
            udp.extend(ports.split(',').map(str::to_owned));
        } else if !plain.starts_with("--wf-") {
            kept.push(token);
        }
    }
    if !kept.iter().any(|token| token.starts_with("--qnum=")) {
        kept.push(format!("--qnum={QNUM}"));
    }
    Ok(Translated {
        args: kept.join(" "),
        tcp: merge_ports(&tcp).context("--wf-tcp-out")?,
        udp: merge_ports(&udp).context("--wf-udp-out")?,
    })
}

/// Ports and ranges as one nftables set: sorted, overlapping and touching ranges joined (an
/// anonymous set refuses overlapping intervals: `1024-65535` next to `50000-65535`).
pub fn merge_ports(ports: &[String]) -> anyhow::Result<String> {
    let mut ranges = Vec::new();
    for port in ports.iter().map(|port| port.trim()) {
        anyhow::ensure!(
            crate::port_range(port),
            "{port:?} is not a port or a port range"
        );
        let (from, to) = port.split_once('-').unwrap_or((port, port));
        ranges.push((from.parse::<u32>()?, to.parse::<u32>()?));
    }
    ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::new();
    for (from, to) in ranges {
        match merged.last_mut() {
            Some(last) if from <= last.1 + 1 => last.1 = last.1.max(to),
            _ => merged.push((from, to)),
        }
    }
    Ok(merged
        .iter()
        .map(|&(from, to)| {
            if from == to {
                from.to_string()
            } else {
                format!("{from}-{to}")
            }
        })
        .collect::<Vec<_>>()
        .join(","))
}

/// The table as `nft -f` takes it. The `table` before `delete` makes the removal not fail on the
/// first apply. Loopback stays out: queued loopback packets stall local connections, and no DPI
/// lives there. A connection's first packets only: `ct original packets`.
pub fn table(tcp: &str, udp: &str, excluded: &str) -> String {
    let mut rules = format!("        meta skgid {GROUP} return\n");
    if !excluded.is_empty() {
        rules.push_str(&format!("        th dport {{ {excluded} }} return\n"));
        rules.push_str(&format!("        th sport {{ {excluded} }} return\n"));
    }
    if !tcp.is_empty() {
        rules.push_str(&format!(
            "        tcp dport {{ {tcp} }} ct original packets 1-12 queue num {QNUM} bypass\n"
        ));
    }
    if !udp.is_empty() {
        rules.push_str(&format!(
            "        udp dport {{ {udp} }} ct original packets 1-8 queue num {QNUM} bypass\n"
        ));
    }
    format!(
        "table inet zapret_hub\n\
         delete table inet zapret_hub\n\
         table inet zapret_hub {{\n\
         \x20   chain output {{\n\
         \x20       type filter hook output priority mangle; policy accept;\n\
         \x20       meta mark & {FWMARK} != 0 return\n\
         \x20       oifname \"lo\" accept\n\
         {rules}    }}\n\
         }}\n"
    )
}

/// The table for `translated` with `excluded` ports (validated, merged).
fn table_for(translated: &Translated, excluded: &[String]) -> anyhow::Result<String> {
    let excluded = merge_ports(excluded).context("excluded ports")?;
    Ok(table(&translated.tcp, &translated.udp, &excluded))
}

/// `{GROUP}` created as a system group when `/etc/group` lacks it. Best effort: when it fails the
/// next `nft -f` names the missing group in its own error.
fn ensure_group() {
    let known = exec::run(
        Command::new("getent").args(["group", GROUP]),
        Duration::from_secs(10),
        None,
    )
    .map(|out| out.success());
    if known.is_ok_and(|known| !known)
        && let Err(err) = zapret::root::check("groupadd", &["--system", GROUP])
    {
        log::warn!("the {GROUP} group was not created: {err:#}");
    }
}

/// Writes the table to {CONFIG}, which the service applies at every start.
pub fn write_config(translated: &Translated, excluded: &[String]) -> anyhow::Result<()> {
    let text = table_for(translated, excluded)?;
    ensure_group();
    zapret::root::write(Path::new(CONFIG), &text)
        .with_context(|| format!("cannot write the nftables table file {CONFIG}"))
}

/// Applies the table now, for an engine started without the service (the tests).
pub fn apply(translated: &Translated, excluded: &[String]) -> anyhow::Result<()> {
    let text = table_for(translated, excluded)?;
    ensure_group();
    let mut file = tempfile::NamedTempFile::new()?;
    std::io::Write::write_all(&mut file, text.as_bytes())?;
    let path = file.path().display().to_string();
    zapret::root::check("nft", &["-f", &path])
}

/// The table gone, best effort: there may be none.
pub fn clear() {
    let _ = zapret::root::run("nft", &["delete", "table", "inet", "zapret_hub"]);
}

/// The hub's table as `nft` lists it (as root), or why there is none.
pub fn rules() -> anyhow::Result<String> {
    let out = zapret::root::run("nft", &["list", "table", "inet", "zapret_hub"])?;
    anyhow::ensure!(
        out.success(),
        "the nftables table inet zapret_hub is not applied:\n{}",
        out.describe()
    );
    Ok(out.stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(items: &[&str]) -> Vec<String> {
        items.iter().map(|item| (*item).to_owned()).collect()
    }

    #[test]
    fn the_filter_options_become_a_queue_and_port_lists() {
        let t =
            translate(r#"--chdir="/tmp/z" --wf-tcp-out=443 --wf-udp-out=53,69 --lua-desync=fake"#)
                .unwrap();
        assert_eq!(t.args, r#"--chdir="/tmp/z" --lua-desync=fake --qnum=200"#);
        assert_eq!((t.tcp.as_str(), t.udp.as_str()), ("443", "53,69"));
    }

    #[test]
    fn every_windivert_option_is_out_but_nothing_else_is() {
        let t = translate(
            r#"--wf-l3=ipv4 --wf-raw-part=@"x\part.txt" --wf-dup-check=0 --wf-tcp-out=443 --intercept=0 --new --lua-desync=fake"#,
        )
        .unwrap();
        assert_eq!(t.args, "--intercept=0 --new --lua-desync=fake --qnum=200");
        assert_eq!((t.tcp.as_str(), t.udp.as_str()), ("443", ""));
    }

    #[test]
    fn a_strategy_that_named_a_queue_keeps_it() {
        let t = translate("--wf-tcp-out=443 --qnum=207 --lua-desync=fake").unwrap();
        assert_eq!(t.args, "--qnum=207 --lua-desync=fake");
    }

    #[test]
    fn ports_that_are_not_ports_never_reach_the_table() {
        assert!(translate("--wf-tcp-out=443;flush").is_err());
        assert!(translate("--wf-udp-out=1-2}").is_err());
        assert!(merge_ports(&strings(&["443 } return"])).is_err());
    }

    #[test]
    fn the_game_filter_range_swallows_the_ranges_inside_it() {
        // The built-in UDP list with the game filter on: Discord voice lies inside 1024-65535.
        let t = translate("--wf-udp-out=443,19294-19344,50000-65535,1024-65535").unwrap();
        assert_eq!(t.udp, "443,1024-65535");
        assert_eq!(
            merge_ports(&strings(&["80", "443", "81", "79", "6881-6889", "6885"])).unwrap(),
            "79-81,443,6881-6889"
        );
        assert_eq!(merge_ports(&[]).unwrap(), "");
    }

    #[test]
    fn the_table_matches_the_readme_shape() {
        let text = table("80,443", "443", "6881-6889,51413");
        for part in [
            "table inet zapret_hub\ndelete table inet zapret_hub\n",
            "chain output {",
            "type filter hook output priority mangle; policy accept;",
            "meta mark & 0x40000000 != 0 return",
            "oifname \"lo\" accept",
            "meta skgid zapret-bypass return",
            "th dport { 6881-6889,51413 } return",
            "th sport { 6881-6889,51413 } return",
            "tcp dport { 80,443 } ct original packets 1-12 queue num 200 bypass",
            "udp dport { 443 } ct original packets 1-8 queue num 200 bypass",
        ] {
            assert!(text.contains(part), "{part}: {text}");
        }
        // The exclusions come before the queue: they leave, the rest reaches it.
        assert!(text.find("meta skgid").unwrap() < text.find("queue num").unwrap());
        assert!(text.find("th dport").unwrap() < text.find("queue num").unwrap());
        let empty = table("", "", "");
        assert!(empty.contains("policy accept;") && !empty.contains("queue num"));
        assert!(empty.contains("meta skgid") && !empty.contains("th dport"));
    }

    #[test]
    fn excluded_ports_are_checked_and_merged_into_the_table() {
        let t = translate("--wf-tcp-out=443").unwrap();
        let text = table_for(&t, &strings(&["51413", "6881-6889", "6885"])).unwrap();
        assert!(
            text.contains("th dport { 6881-6889,51413 } return"),
            "{text}"
        );
        assert!(table_for(&t, &strings(&["53; flush ruleset"])).is_err());
    }
}
