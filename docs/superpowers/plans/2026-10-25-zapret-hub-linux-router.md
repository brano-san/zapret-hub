# Linux router Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** `routes.toml` rules route matching IPv4 TCP/UDP through the local SOCKS5 proxy on Linux, as on Windows.

**Architecture:** A `cfg(target_os = "linux")` implementation of `router::live::Live` (same `open/shut/apply/proxy/stats` interface as the Windows one): nftables `redirect` for TCP, TPROXY + policy routing for UDP, `AF_PACKET` DNS sniffing for domain rules, `/proc` owners for program rules, `SO_MARK` on the hub's own sockets. In-process `CAP_NET_ADMIN`/`CAP_NET_RAW` via `setcap` on the hub binary (one root batch).

**Spec:** `docs/superpowers/specs/2026-10-25-zapret-hub-linux-router-design.md`

## Global Constraints

- Repo `D:\zapret-hub`, branch `master`; commit directly. One-line Conventional Commits, imperative, ≤72 chars, NO body, NO trailers, NO Co-Authored-By. Never push.
- Build memory (user): Windows cargo `CARGO_BUILD_JOBS=1`, WSL `CARGO_BUILD_JOBS=2`; never two cargo processes at once; iterate with `-p <crate>`, full runs once at the end.
- No system changes (no nft, ip, setcap, sudo, systemctl), never run exes in `D:\zapret-hub\build`, no python, no git stash, no subagents. Live tests needing caps/nft are `#[ignore]`.
- Windows router behavior unchanged; reuse `rules::Table`, `socks.rs`, `flows.rs`, `dns.rs`, `owner::procfs`, `zapret::root` (Batch, tool lookup, `LC_ALL=C`).
- Windows verification: fmt check, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace --exclude zapret-hub`, `cargo test -p zapret-hub --lib`, `npm --prefix frontend run check`, `npm --prefix frontend test`.
- Linux verification: `wsl.exe -e bash -lc 'cd /mnt/d/zapret-hub && export CARGO_TARGET_DIR=$HOME/zh-target CARGO_BUILD_JOBS=2 && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace'`.

## Review Focus

1. Hub's own direct/relay sockets are never redirected back to itself (loop).
2. A crash leaves no stale nft table / ip rule that blackholes traffic: cleared on next start and on `shut`.
3. No caps → `Unavailable` with a clear reason and the grant hint, never a panic or hang.
4. `apply` swaps sets without dropping live connections.
5. Program rules: a non-matching program's connection on a rule port still works (direct, marked).

---

### Task 1: Linux plumbing (pure parts + caps)

**Files:** `crates/router/src/live/linux/{mod,nft,caps,sockopt,sniff}.rs` (new; adjust layout to the crate's style), `crates/router/src/lib.rs` (cfg selection), tests.

- [ ] `nft::ruleset(table, acceptor_tcp, tproxy_udp, proxy) -> String`: table `inet zapret_hub_route`, sets for rule subnets / domain IPs (with timeout) / ports, TCP `nat output` redirect, UDP `mangle output` mark + `mangle prerouting` tproxy, skips: `lo` except TPROXY path, `meta mark & 0x20000000`, `meta skgid zapret-bypass` (if group exists), the SOCKS5 address. Plus `set_update(...)` text for `apply`, and ip rule/route add/del argv. Unit tests on exact text.
- [ ] `caps`: read `/proc/self/status` CapEff for NET_ADMIN/NET_RAW (pure parse + test); raise ambient caps for children (`prctl`); `grant(exe)` = one `root::Batch` running `setcap cap_net_admin,cap_net_raw+eip <exe>` (via `root::tool`).
- [ ] `sockopt`: `SO_ORIGINAL_DST` getsockopt, `SO_MARK` setsockopt, `IP_TRANSPARENT`, `IP_RECVORIGDSTADDR` + cmsg parse (pure parse tested).
- [ ] `sniff`: `AF_PACKET` socket reader; pure `udp_payload_from_frame(frame, cooked/ethernet)` → `(src, dst, payload)` for IPv4 UDP sport 53; tests with crafted frames; feeds `dns.rs`.
- [ ] Commits per concern; WSL `cargo clippy/test -p router` green; Windows `-p router` green.

### Task 2: Linux Live session

**Files:** `crates/router/src/live/linux/{mod,tcp,udp}.rs`, maybe small shared refactors in `live.rs`/`flows.rs` (keep Windows intact).

- [ ] `Live::open`: caps check (→ error text with hint), cleanup of stale table/rules, bind TCP acceptor (127.0.0.1:0) and UDP TPROXY socket, install ruleset + ip rule/route, start threads (accept loop, UDP loop, DNS sniffer, proxy health like Windows `check_proxy`, housekeeping).
- [ ] TCP: accept → `SO_ORIGINAL_DST` → decide (`Table::matches_flow` with owner exe from procfs) → SOCKS5 CONNECT (reuse `socks.rs`) or direct marked connect → splice both ways (reuse `tcp::pipe`-style code if portable) → `Stats`/`FlowRow` updates.
- [ ] UDP: recvmsg with orig dst → flow table keyed by (src, dst) → SOCKS5 UDP ASSOCIATE relay or direct marked socket → replies via transparent socket bound to orig dst → idle expiry (`flows::IDLE`).
- [ ] DNS: sniffed answers → `Names` → `apply`-style set update with TTL.
- [ ] `apply(Some(table))`: atomic set swap; `shut`: delete table + ip rule/route, stop threads; both idempotent.
- [ ] Tests: decision logic and flow bookkeeping pure-tested; an `#[ignore]` live test (needs root/caps) that routes a TCP and UDP flow through a local fake SOCKS5 (reuse existing test SOCKS server if any).
- [ ] WSL + Windows verification green.

### Task 3: App, CLI, UI, docs

- [ ] src-tauri: routing on Linux uses the router; remove the Linux "not supported" gating; `routes grant` CLI + a command for the UI; status reason when caps are missing; journal lines.
- [ ] Frontend: Routes tab on Linux shows the router; card with reason + «Разрешить маршрутизацию» when caps are missing; remove the old one-line "not supported" text.
- [ ] Docs: `docs/linux.md` «Маршрутизация» (how it works, setcap, re-grant after update, IPv4 only, DoT/DoH caveat), `docs/cli.md`, README, CHANGELOG `## 0.9.1`; bump version to 0.9.1.
- [ ] Full Windows + WSL verification green.
