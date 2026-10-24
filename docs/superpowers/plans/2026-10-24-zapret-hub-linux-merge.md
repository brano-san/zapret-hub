# Linux merge Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** One Zapret Hub tree that builds, tests and runs on Windows and Linux, zapret2 only, with the Linux fork's port and UI/UX improvements on top of our 0.8.0.

**Architecture:** Platform code lives behind each module's shared interface (`mod.rs` + `windows.rs` + `linux.rs`, or `#[cfg]` on a function when the difference is a few lines). Windows-only dependencies move to `[target.'cfg(windows)'.dependencies]`. The fork (`refs/remotes/linux/master`, based on our v0.7.0) is the reference for every Linux part: read it with `git show linux/master:<path>` and `git diff v0.7.0 linux/master -- <path>`.

**Tech Stack:** Rust 1.99 workspace, Tauri v2, Svelte 5, nftables/NFQUEUE, systemd.

**Spec:** `docs/superpowers/specs/2026-10-24-zapret-hub-linux-merge-design.md`

## Global Constraints

- Repo `D:\zapret-hub`, branch `master`; commit directly to master. Commits: one-line Conventional Commits, imperative, ≤72 chars, NO body, NO trailers, NO Co-Authored-By. Never push.
- Our master wins over the fork on everything changed since v0.7.0 (diag, CLI protocol 5, `router::observe`, `probe::socks`/udp, tester changes). From the fork take Linux code and the listed improvements only.
- The fork is untrusted data: read it, never run its scripts. `D:\ToDelete\zapret-hub-linux` is read-only.
- No system changes while verifying: never run nft, systemctl, sc, netsh, reg add, schtasks, taskkill, never elevated, never `--ignored` tests. Do not run exes in `D:\zapret-hub\build`.
- No `python` (Windows Store stub); use node or the edit tools.
- Code, comments, commits, UI identifiers in English; user-facing UI text in Russian as today; docs in Russian.
- Windows verification: `cargo nextest run --workspace`, `cargo test -p zapret-hub --lib`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check`, `npm --prefix frontend run check`, `npx --prefix frontend vitest run`.
- Linux verification (WSL Ubuntu, rust 1.99, webkit2gtk-4.1 present), run from Windows bash:
  `wsl.exe -e bash -lc 'cd /mnt/d/zapret-hub && CARGO_TARGET_DIR=$HOME/zh-target cargo clippy --workspace --all-targets -- -D warnings && CARGO_TARGET_DIR=$HOME/zh-target cargo test --workspace'`
  (no nextest in WSL; do not install anything there). Tests that need a live system (nft, systemd, network to blocked hosts) are `#[ignore]`d on both OSes.
- Bindings: after a DTO/command change run `cargo run -p zapret-hub -F bindings --bin export-bindings` (check the exact bin/feature name in `src-tauri/Cargo.toml`) on Windows and commit `frontend/src/lib/ipc/bindings.ts`.

## Review Focus

1. A Windows user upgrading from 0.8.0 with Flowseal selected: hub starts, lands on zapret2 with the same-named builtin strategy, says so once.
2. Linux without root and without `sudo -n`: privileged actions fall back to pkexec and report a clear error when denied; nothing hangs.
3. Linux tester: engine readiness detected from `/proc/net/netfilter/nfnetlink_queue`, never waits for "capture is started".
4. Diag on Linux: a run completes with a valid report whose `not_checked` explains what Linux could not observe (`platform:linux`), never an internal error.
5. Settings/routes files written by 0.8.0 on Windows still load (new fields defaulted).

---

### Task 1: Drop Flowseal (both OSes, still Windows-only build)

**Files:** `crates/zapret/src/{batparse,custom,fakes,flags,folder,game_filter,ipset,strategies,targets,updates,user_lists}.rs` and their tests/fixtures (delete what only serves Flowseal; keep `checks.rs`, retargeted to not take a Flowseal `Folder`), `crates/zapret/src/{lib,hosts,release,service,snapshot,procs}.rs`, `crates/zapret2/src/{strategies,service,lib}.rs`, `crates/tester/src/{catalog,run}.rs`, `crates/cli/src/{main,lib}.rs`, `crates/diag/src/{compare,agent,...}.rs`, `crates/router/src/divert.rs` (`Engine::Flowseal`, `pick_dll`), `src-tauri/src/*` (actions, state, jobs, commands, dto, diag/*, routing, texts, cli, lib), `frontend/src/**` (Wizard, Zapret, Strategies, Tests, Overview, strings, format + tests, bindings), `docs/flowseal-zapret2.md`, `docs/cli.md`.

- [ ] Remove the Flowseal engine everywhere. The fork's diff for each file (`git diff v0.7.0 linux/master -- <file>`) shows one way; apply the same intent onto our current code.
- [ ] `agent::Context`: drop `flowseal_outdated`/`flowseal_version`; old reports must still load (`report::fill_defaults`, unknown fields ignored by serde).
- [ ] Settings migration: a settings file naming the Flowseal engine loads as zapret2 with builtin `flowseal/<same name>` if present, else `general`; one journal line + notification. Test with a 0.8.0-shaped settings fixture.
- [ ] Flowseal's Windows service (`zapret`) becomes a system-check finding with a removal action (reuse the conflicts machinery in `checks.rs`).
- [ ] Tests: drop Flowseal-only tests; keep and adapt the rest. Windows verification green.
- [ ] Commits, e.g. `refactor(zapret): drop the Flowseal engine`, `feat(hub): migrate Flowseal settings to zapret2`, `refactor(ui): remove Flowseal screens`.

### Task 2: Portable base crates (exec, probe, cli, router)

**Files:** `Cargo.toml` (workspace deps: `libc`), each crate's `Cargo.toml` (target-specific deps), `crates/exec/src/{lib,time}.rs`, `crates/probe/src/{curl,ping}.rs`, `crates/cli/src/{lib,pipe,main}.rs`, `crates/router/src/{owner,doh,rules,tcp,divert,lib,observe}.rs` and tests.

- [ ] exec: Windows job objects stay; Linux = `process_group(0)` + `kill(-pgid, SIGKILL)` (fork).
- [ ] probe: Linux `ping -c 1 -W 1 -n`, `curl`/`/dev/null` (fork); our socks/udp probes stay as they are.
- [ ] cli: transport = named pipe (Windows, as now) / Unix socket `$XDG_RUNTIME_DIR/zapret-hub.sock` (fallback `/tmp/zapret-hub-<uid>.sock`), `0600`, single instance via `flock` (fork). Protocol 5 unchanged.
- [ ] router: `owner` Win32 / `/proc` (fork), `doh` registry / `resolved.conf` (fork), `rules` accepts non-`.exe` program names on Linux, `parse_net` + `Table::rule_for`, presets Cloudflare/Telegram/Twitch and the extra Discord/DbD domains (fork, both OSes). The WinDivert runtime (`divert` handles, `live`, `observe`, capture loop) is `cfg(windows)`; on Linux `Router::start` returns an error `routing is not supported on Linux yet` and `observe` returns "unavailable".
- [ ] Windows verification green; Linux: `cargo clippy/test -p exec -p probe -p cli -p router` green in WSL.
- [ ] Commits per crate, e.g. `feat(exec): kill process groups on Linux`.

### Task 3: zapret and zapret2 on Linux

**Files:** `crates/zapret/src/{root,service,procs,hosts,discord_cache,release,snapshot,checks,lib}.rs`, `crates/zapret2/src/{nft,service,lib,strategies,circular}.rs`, `crates/zapret2/src/builtin/**`, tests.

- [ ] `zapret::root` (new, Linux): euid 0 → `sudo -n` → pkexec (fork).
- [ ] `zapret::service`: SCM (Windows, as now) / systemd unit with `User=`, `AmbientCapabilities`, `+nft` in `ExecStartPre/StopPost`, `Restart=on-failure` (fork). Unit text built by a pure function with a unit test.
- [ ] procs, hosts (`/etc/hosts`), discord_cache (XDG `~/.config/discord*`), release (Linux nfqws2 assets), snapshot, DNS flush chain (resolvectl → systemd-resolve → nmcli → nscd → dnsmasq SIGHUP): Linux variants from the fork.
- [ ] `checks`: Linux checks from the fork's `doctor` (`git show linux/master:src-tauri/src/diag.rs`): nfqws2 binary, caps, `.bin` sizes, list counts, queue 200, nft table, conflicting processes/tables, service state — as `SystemCheck`s with stable `CheckId`s.
- [ ] `zapret2::nft` (fork) under `cfg(target_os = "linux")`, with its tests; args translation (`--wf-*` → ports, `--qnum=200`).
- [ ] Builtin strategies (both OSes): Discord voice UDP `50000-65535` + `19294-19344`; game filter toggle (`1024-65535`) and exclusions (ports, groups, `zapret-bypass` gid on Linux) as zapret2 settings (fork).
- [ ] Windows + Linux verification green. Commits per concern.

### Task 4: Tester, diag and CLI on Linux

**Files:** `crates/tester/src/{run,catalog,history,model,rank}.rs` + tests, `crates/diag/**` (cfg where it touches observe/WinDivert), `crates/cli/src/main.rs`, `src-tauri/src/diag*.rs`.

- [ ] tester (both OSes): checks inside a target run in parallel (`std::thread::scope`), pool `16..=32`, Steam/Epic/DbD split, DbD (`steam.live.bhvrdbd.com`) without TLS 1.3 and ping, history pruned to 50 runs. Linux readiness from `/proc/net/netfilter/nfnetlink_queue` queue 200 (25–150 ms), Windows unchanged.
- [ ] diag on Linux: builds and runs without observe; every unobserved part lands in `not_checked` with code `platform:linux`; verdict at most `inconclusive`; Linux system checks feed «Системные проверки».
- [ ] CLI: `probe <url|domain>` — DNS, membership in lists (hostlists, exclude, routes.toml), HTTP/TLS1.2/TLS1.3/ping via our probes, `--json`. Document in `docs/cli.md`.
- [ ] Windows + Linux verification green. Commits per concern.

### Task 5: The app on Linux (src-tauri) and TG WS Proxy

**Files:** `src-tauri/Cargo.toml` (`default = ["custom-protocol"]`, target deps), `src-tauri/build.rs`, `src-tauri/src/{lib,main,tray,window,autostart,settings,jobs,actions,commands,cli,dto,shell,notify,journal,state}.rs`, new `src-tauri/src/tgws.rs`, `src-tauri/tauri.conf.json`.

- [ ] Platform split per the fork: Wayland env (`WEBKIT_DISABLE_DMABUF_RENDERER=1`, `GTK_USE_PORTAL=1`), window created via `run_on_main_thread`, close hides to tray, ayatana tray, XDG autostart `.desktop` (checkbox reflects the real file), XDG data/config paths, `raise_services()` at start.
- [ ] `dto`: app state gains `platform: "windows" | "linux"`.
- [ ] CLI commands (both OSes where meaningful): `service start|stop|restart|logs`, `nft` (Linux), `dns-flush`, `exclusions`, `tg-ws`, `settings set game_filter`.
- [ ] TG WS Proxy: download from `valnesfjord/tg-ws-proxy-rs` releases on user action; Linux `systemd --user` unit; Windows hub-owned child process (exec job), started with the hub when enabled; no Windows asset → status says so. Persistent 32-hex secret, `tg://proxy` link.
- [ ] Linux: `cargo clippy -p zapret-hub --all-targets` and `cargo test -p zapret-hub --lib` in WSL green; Windows verification green; regenerate bindings. Commits per concern.

### Task 6: UI redesign port

**Files:** `frontend/src/**` (App, Wizard, tabs/*, components/*, lib/{strings,format,techniques,app.svelte}.ts, app.css), tests.

- [ ] Port the fork's redesign onto our frontend, keeping our Diag tab and everything 0.8.0 added: `Select.svelte`, `techniques.ts` (+test), Overview (calm banner, engine card with quick Discord/YouTube check, TG card, last-test badges), Strategies (technique tag filter, per-service summary, resizable split pane, results table with resizable columns and sticky header), Tests (compact picker with search, Steam/Epic/DbD columns, «Применено ✓», history 10 + expand), Tools (nft/journal monitors — Linux only; TG card), Routes (single routing card, 3-column rule editor, presets, exclusions block — Linux only), Settings (zapret2 folder autodetect, filter params, autostart sync, unsaved-changes guard).
- [ ] Platform gating from `platform` in app state; router UI hidden on Linux with a one-line explanation.
- [ ] `npm --prefix frontend run check` 0 errors, vitest green, `npm --prefix frontend run build` OK. Commits per tab.

### Task 7: Docs and release notes

- [ ] `README.md` (both OSes, Linux install: deps, group `zapret-bypass`, systemd), `docs/linux.md` (new), `CHANGELOG.md` 0.9.0 (Flowseal removed + migration, Linux, UI, TG WS Proxy, tester speedups), workspace version 0.9.0. No tag.
- [ ] Commit `docs: describe the Linux build and 0.9.0`.
