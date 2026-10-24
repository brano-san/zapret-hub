//! The routing table as `nft -f` takes it. Fixed base chains skip what must never loop back (the
//! hub's own marked sockets, loopback, the `zapret-bypass` group, the SOCKS5 proxy itself) and jump
//! to two match chains, the only part `update` rewrites: one transaction flushes and refills them,
//! so a change of rules or resolved names never leaves a moment without a match, and live TCP
//! connections keep their conntrack NAT.
//!
//! TCP: `nat output` redirects a match to the acceptor. UDP: a `route output` chain marks a match
//! with `MARK_TPROXY`; `ip rule fwmark … lookup <table>` (`Policy`) + a `local` route send it to `lo`,
//! where `prerouting` hands it to the TPROXY socket.
//!
//! Domain IPs come in through `Table::with_names` like the rule subnets: `Names` expiry (which
//! keeps addresses of live flows) is their timeout, never an nft timeout that would cut a live UDP
//! flow mid-call.

use std::net::{Ipv4Addr, SocketAddrV4};

use crate::rules::{Protocol, Table, merge};

pub const TABLE: &str = "zapret_hub_route";
/// A UDP packet to hand to the TPROXY socket.
pub const MARK_TPROXY: u32 = 0x1000_0000;
/// The hub's own sockets (direct connections, relays, replies): never matched.
pub const MARK_OWN: u32 = 0x2000_0000;
/// Processes of this group (`sg zapret-bypass …`, as for zapret2's queue) are never routed.
pub const GROUP: &str = "zapret-bypass";

fn net((a, b): (u32, u32)) -> String {
    let size = u64::from(b - a) + 1;
    if size.is_power_of_two() && u64::from(a) % size == 0 {
        match 32 - size.trailing_zeros() {
            32 => Ipv4Addr::from(a).to_string(),
            bits => format!("{}/{bits}", Ipv4Addr::from(a)),
        }
    } else {
        format!("{}-{}", Ipv4Addr::from(a), Ipv4Addr::from(b))
    }
}

fn port((a, b): (u32, u32)) -> String {
    if a == b {
        a.to_string()
    } else {
        format!("{a}-{b}")
    }
}

fn set(items: Vec<(u32, u32)>, show: fn((u32, u32)) -> String) -> String {
    let items: Vec<String> = merge(items).into_iter().map(show).collect();
    format!("{{ {} }}", items.join(", "))
}

/// The match expressions of `protocol`, one per rule and kind, in file order: a rule's subnets
/// (and resolved names), and for a rule with programs every flow on its ports, which the acceptor
/// sorts by owner (UDP only while a program runs, as `Table::wants_owner`).
fn matches(table: &Table, protocol: Protocol) -> Vec<String> {
    let mut out = Vec::new();
    for r in &table.rules {
        if r.protocol != Protocol::Both && r.protocol != protocol {
            continue;
        }
        let ports = if r.ports.is_empty() {
            String::new()
        } else {
            let ports = r.ports.iter().map(|&(a, b)| (a.into(), b.into()));
            format!(" th dport {}", set(ports.collect(), port))
        };
        if !r.ranges.is_empty() {
            out.push(format!(" ip daddr {}{ports}", set(r.ranges.clone(), net)));
        }
        if !r.apps.is_empty() && (protocol == Protocol::Tcp || r.running) {
            out.push(ports);
        }
    }
    out
}

/// Replaces the match chains with `table`'s rules in one transaction (`apply`, new names).
pub fn update(table: &Table, acceptor: u16) -> String {
    let mut text =
        format!("flush chain inet {TABLE} tcp_match\nflush chain inet {TABLE} udp_match\n");
    for m in matches(table, Protocol::Tcp) {
        text += &format!(
            "add rule inet {TABLE} tcp_match meta l4proto tcp{m} redirect to :{acceptor}\n"
        );
    }
    for m in matches(table, Protocol::Udp) {
        text += &format!(
            "add rule inet {TABLE} udp_match meta l4proto udp{m} meta mark set meta mark | {MARK_TPROXY:#x}\n"
        );
    }
    text
}

/// The whole table, replacing any left by a crashed session. `bypass_group`: `GROUP` exists
/// (`nft` refuses an unknown group name).
pub fn ruleset(
    table: &Table,
    acceptor: u16,
    tproxy: u16,
    proxy: SocketAddrV4,
    bypass_group: bool,
) -> String {
    let skips = |proto: &str| {
        let group = if bypass_group {
            format!("        meta skgid {GROUP} return\n")
        } else {
            String::new()
        };
        // Answers to multicast (mDNS, SSDP) and broadcast never come back to a relay.
        // ponytail: a subnet broadcast (192.168.1.255) is not told apart; `fib daddr type` would,
        // but needs nft_fib, which some kernels (WSL) lack.
        let cast = if proto == "udp" {
            "        ip daddr { 224.0.0.0/4, 255.255.255.255 } return\n"
        } else {
            ""
        };
        format!(
            "        meta nfproto ipv6 return\n\
             \x20       oifname \"lo\" return\n\
             {cast}\
             \x20       meta mark & {MARK_OWN:#x} != 0 return\n\
             {group}\
             \x20       ip daddr {} {proto} dport {} return\n\
             \x20       meta l4proto {proto} jump {proto}_match\n",
            proxy.ip(),
            proxy.port()
        )
    };
    format!(
        "{}table inet {TABLE} {{\n\
         \x20   chain tcp_match {{\n\
         \x20   }}\n\
         \x20   chain udp_match {{\n\
         \x20   }}\n\
         \x20   chain output_tcp {{\n\
         \x20       type nat hook output priority -100; policy accept;\n\
         {}\
         \x20   }}\n\
         \x20   chain output_udp {{\n\
         \x20       type route hook output priority mangle; policy accept;\n\
         {}\
         \x20   }}\n\
         \x20   chain prerouting_udp {{\n\
         \x20       type filter hook prerouting priority mangle; policy accept;\n\
         \x20       iifname \"lo\" meta l4proto udp meta mark & {MARK_TPROXY:#x} != 0 tproxy ip to 127.0.0.1:{tproxy} accept\n\
         \x20   }}\n\
         }}\n{}",
        delete(),
        skips("tcp"),
        skips("udp"),
        update(table, acceptor)
    )
}

/// Removes the table, also when there is none (the `table` line creates it first).
pub fn delete() -> String {
    format!("table inet {TABLE}\ndelete table inet {TABLE}\n")
}

/// The `ip rule` selector of `MARK_TPROXY`: what tells our rules from any other.
const MARK_RULE: &str = "0x10000000/0x10000000";
/// Where the policy routing goes when nothing uses it: a table number seldom taken, and an early
/// priority, so a VPN's own rules (wg-quick 32764–32765, Tailscale 5210–5270) never take a marked
/// packet. Taken ones step to the next free (`pick`).
pub const ROUTE_TABLE: u32 = 0x5a48;
pub const RULE_PRIORITY: u32 = 1000;

/// One policy rule of ours: its priority and the table it looks up.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Policy {
    pub priority: u32,
    pub table: u32,
}

impl Policy {
    /// `ip` arguments that add (`true`) or remove the rule and its `local` route. The route is
    /// `replace`d, so adding twice is harmless.
    pub fn argv(self, add: bool) -> [Vec<String>; 2] {
        let (rule, route) = if add {
            ("add", "replace")
        } else {
            ("del", "del")
        };
        let (priority, table) = (self.priority.to_string(), self.table.to_string());
        let words = |w: &[&str]| w.iter().map(|w| (*w).to_owned()).collect();
        [
            words(&[
                "-4", "rule", rule, "priority", &priority, "fwmark", MARK_RULE, "lookup", &table,
            ]),
            words(&[
                "-4",
                "route",
                route,
                "local",
                "0.0.0.0/0",
                "dev",
                "lo",
                "table",
                &table,
            ]),
        ]
    }
}

/// `(priority, words)` of each `ip rule show` line (`32765:\tfrom all fwmark … lookup 100`).
fn rule_lines(rules: &str) -> impl Iterator<Item = (Option<u32>, Vec<&str>)> {
    rules.lines().filter_map(|line| {
        let (priority, rest) = line.split_once(':')?;
        Some((
            priority.trim().parse().ok(),
            rest.split_whitespace().collect(),
        ))
    })
}

fn after<'a>(words: &[&'a str], key: &str) -> Option<&'a str> {
    words
        .iter()
        .position(|w| *w == key)
        .and_then(|i| words.get(i + 1).copied())
}

/// Our rules in `ip -4 rule show`, by the mark, whatever their priority and table: also those a
/// crashed session or an older version (table 100) left.
pub fn ours(rules: &str) -> Vec<Policy> {
    rule_lines(rules)
        .filter(|(_, words)| after(words, "fwmark") == Some(MARK_RULE))
        .filter_map(|(priority, words)| {
            Some(Policy {
                priority: priority?,
                table: after(&words, "lookup")?.parse().ok()?,
            })
        })
        .collect()
}

/// Table numbers an `rt_tables` file names (`100\tvpn`): someone's, even when empty now.
pub fn named_tables(text: &str) -> Vec<u32> {
    text.lines()
        .filter_map(|l| l.split_whitespace().next()?.parse().ok())
        .collect()
}

/// A priority no rule in `rules` (`ip rule show`, both families) has, and a table no rule looks
/// up, no `rt_tables` file names (`named`) and that holds no route (`has_routes`): the defaults
/// when free, else the next free. `None` when none of 256 tables is.
pub fn pick(rules: &str, named: &[u32], mut has_routes: impl FnMut(u32) -> bool) -> Option<Policy> {
    let lines: Vec<_> = rule_lines(rules).collect();
    let priority = (RULE_PRIORITY..).find(|p| !lines.iter().any(|(q, _)| *q == Some(*p)))?;
    let used = |t: u32| {
        named.contains(&t)
            || lines
                .iter()
                .any(|(_, w)| after(w, "lookup") == Some(t.to_string().as_str()))
    };
    let table = (ROUTE_TABLE..ROUTE_TABLE + 256).find(|&t| !used(t) && !has_routes(t))?;
    Some(Policy { priority, table })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::rules::Rule;

    fn rule(name: &str, protocol: Protocol, ports: &[&str], ips: &[&str], apps: &[&str]) -> Rule {
        let strings = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect();
        Rule {
            name: name.into(),
            enabled: true,
            protocol,
            ports: strings(ports),
            ips: strings(ips),
            domains: vec![],
            apps: strings(apps),
        }
    }

    const PROXY: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 2080);

    #[test]
    fn an_empty_table_is_the_skeleton_with_empty_match_chains() {
        let text = ruleset(&Table::default(), 40000, 40001, PROXY, true);
        assert_eq!(
            text,
            "table inet zapret_hub_route\n\
             delete table inet zapret_hub_route\n\
             table inet zapret_hub_route {\n\
             \x20   chain tcp_match {\n\
             \x20   }\n\
             \x20   chain udp_match {\n\
             \x20   }\n\
             \x20   chain output_tcp {\n\
             \x20       type nat hook output priority -100; policy accept;\n\
             \x20       meta nfproto ipv6 return\n\
             \x20       oifname \"lo\" return\n\
             \x20       meta mark & 0x20000000 != 0 return\n\
             \x20       meta skgid zapret-bypass return\n\
             \x20       ip daddr 127.0.0.1 tcp dport 2080 return\n\
             \x20       meta l4proto tcp jump tcp_match\n\
             \x20   }\n\
             \x20   chain output_udp {\n\
             \x20       type route hook output priority mangle; policy accept;\n\
             \x20       meta nfproto ipv6 return\n\
             \x20       oifname \"lo\" return\n\
             \x20       ip daddr { 224.0.0.0/4, 255.255.255.255 } return\n\
             \x20       meta mark & 0x20000000 != 0 return\n\
             \x20       meta skgid zapret-bypass return\n\
             \x20       ip daddr 127.0.0.1 udp dport 2080 return\n\
             \x20       meta l4proto udp jump udp_match\n\
             \x20   }\n\
             \x20   chain prerouting_udp {\n\
             \x20       type filter hook prerouting priority mangle; policy accept;\n\
             \x20       iifname \"lo\" meta l4proto udp meta mark & 0x10000000 != 0 tproxy ip to 127.0.0.1:40001 accept\n\
             \x20   }\n\
             }\n\
             flush chain inet zapret_hub_route tcp_match\n\
             flush chain inet zapret_hub_route udp_match\n"
        );
        let without = ruleset(&Table::default(), 40000, 40001, PROXY, false);
        assert!(!without.contains("skgid"));
        assert_eq!(
            without.len(),
            text.len() - 2 * "        meta skgid zapret-bypass return\n".len()
        );
    }

    #[test]
    fn rules_become_match_lines_per_protocol_in_file_order() {
        let table = Table::new(&[
            rule(
                "Tg",
                Protocol::Both,
                &["443", "80", "400-500"],
                &["1.2.2.0/24", "1.2.3.0/24", "9.9.9.9"],
                &[],
            ),
            rule(
                "Voice",
                Protocol::Udp,
                &["50000-65535"],
                &["5.6.7.8", "5.6.7.9", "5.6.7.10"],
                &[],
            ),
            rule("Game", Protocol::Tcp, &[], &[], &["game"]),
            rule("UdpGame", Protocol::Udp, &["3000"], &[], &["game"]),
        ]);
        assert_eq!(
            update(&table, 40000),
            "flush chain inet zapret_hub_route tcp_match\n\
             flush chain inet zapret_hub_route udp_match\n\
             add rule inet zapret_hub_route tcp_match meta l4proto tcp ip daddr { 1.2.2.0/23, 9.9.9.9 } th dport { 80, 400-500 } redirect to :40000\n\
             add rule inet zapret_hub_route tcp_match meta l4proto tcp redirect to :40000\n\
             add rule inet zapret_hub_route udp_match meta l4proto udp ip daddr { 1.2.2.0/23, 9.9.9.9 } th dport { 80, 400-500 } meta mark set meta mark | 0x10000000\n\
             add rule inet zapret_hub_route udp_match meta l4proto udp ip daddr { 5.6.7.8-5.6.7.10 } th dport { 50000-65535 } meta mark set meta mark | 0x10000000\n"
        );
        // A UDP rule's program takes its ports only while it runs.
        let running = table.with_running(&["/usr/bin/game"]);
        assert!(update(&running, 40000).ends_with(
            "add rule inet zapret_hub_route udp_match meta l4proto udp th dport { 3000 } meta mark set meta mark | 0x10000000\n"
        ));
    }

    #[test]
    fn resolved_names_join_their_rule_and_whole_space_is_one_net() {
        let mut r = rule("Cf", Protocol::Tcp, &[], &[], &[]);
        r.domains = vec!["cloudflare.com".into()];
        let table = Table::new(&[r]);
        assert_eq!(
            matches(&table, Protocol::Tcp),
            Vec::<String>::new(),
            "no names yet: no line"
        );
        let named =
            table.with_names(&[(Ipv4Addr::new(104, 16, 0, 1), "www.cloudflare.com".into())]);
        assert_eq!(matches(&named, Protocol::Tcp), [" ip daddr { 104.16.0.1 }"]);
        assert_eq!(net((0, u32::MAX)), "0.0.0.0/0");
        assert_eq!(net((1, 2)), "0.0.0.1-0.0.0.2");
        assert_eq!(net((2, 3)), "0.0.0.2/31");
    }

    #[test]
    fn delete_and_ip_routing_arguments() {
        assert_eq!(
            delete(),
            "table inet zapret_hub_route\ndelete table inet zapret_hub_route\n"
        );
        let p = Policy {
            priority: 1000,
            table: 23112,
        };
        assert_eq!(
            p.argv(true),
            [
                "-4 rule add priority 1000 fwmark 0x10000000/0x10000000 lookup 23112",
                "-4 route replace local 0.0.0.0/0 dev lo table 23112",
            ]
            .map(|l| l.split(' ').map(str::to_owned).collect::<Vec<_>>())
        );
        assert_eq!(p.argv(false)[0][2], "del");
        assert_eq!(p.argv(false)[1][2], "del");
    }

    const RULES: &str = "0:\tfrom all lookup local\n\
        1000:\tfrom all fwmark 0x10000000/0x10000000 lookup 23112\n\
        5270:\tfrom all lookup 52\n\
        32765:\tfrom all fwmark 0x10000000/0x10000000 lookup 100 proto boot\n\
        32765:\tfrom all fwmark 0x10000 lookup 23113\n\
        32766:\tfrom all lookup main\n\
        32767:\tfrom all lookup default\n";

    #[test]
    fn our_rules_are_known_by_the_mark_whatever_their_priority_and_table() {
        assert_eq!(
            ours(RULES),
            [
                Policy {
                    priority: 1000,
                    table: 23112
                },
                Policy {
                    priority: 32765,
                    table: 100
                },
            ]
        );
        assert_eq!(ours(""), []);
    }

    #[test]
    fn the_defaults_are_taken_when_free_else_the_next_free() {
        let base = "0:\tfrom all lookup local\n32766:\tfrom all lookup main\n";
        assert_eq!(
            pick(base, &[], |_| false),
            Some(Policy {
                priority: RULE_PRIORITY,
                table: ROUTE_TABLE
            })
        );
        // 1000 and the tables 23112 and 23113 are looked up by rules; 23114 is named in
        // rt_tables, 23115 holds a route.
        let named = named_tables("# reserved\n255\tlocal\n23114 mine\n");
        let p = pick(RULES, &named, |t| t == 23115).unwrap();
        assert_eq!(
            p,
            Policy {
                priority: 1001,
                table: 23116
            }
        );
        assert_eq!(pick(base, &[], |_| true), None, "every table holds routes");
        assert_eq!(named_tables("# c\n\n100 vpn\n0x10 hex\n"), [100]);
    }

    /// `nft` itself checks the text: needs `CAP_NET_ADMIN`, e.g. `unshare -rn <test binary>
    /// --ignored nft_accepts`.
    #[test]
    #[ignore = "needs nft and CAP_NET_ADMIN (a user namespace will do)"]
    fn nft_accepts_the_ruleset() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let table = Table::new(&[
            rule(
                "Tg",
                Protocol::Both,
                &["443", "80"],
                &["1.2.3.0/24", "9.9.9.9"],
                &[],
            ),
            rule("Game", Protocol::Both, &["3000-3100"], &[], &["game"]),
        ])
        .with_running(&["game"]);
        let nft = |text: &str| {
            let mut child = Command::new("nft")
                .args(["-f", "-"])
                .stdin(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(text.as_bytes())
                .unwrap();
            assert!(child.wait().unwrap().success(), "{text}");
        };
        nft(&ruleset(&table, 40000, 40001, PROXY, false));
        nft(&update(&Table::default(), 40000));
        nft(&update(&table, 40000));
        nft(&delete());
        nft(&delete());
    }
}
