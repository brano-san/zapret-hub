# Zapret Hub — диагностика (подпроекты 2 + 9) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Вкладка и CLI «Диагностика»: хаб наблюдает живые соединения программы, находит адреса-виновники, перебирает пути (напрямую, стратегии, приёмы zapret2, прокси), откатывает всё, объясняет причину (включая картины Р1–Р6) и применяет лучший путь по кнопке; найденные приёмы копятся в `zapret2:hub-circular`.

**Architecture:** Новый крейт `crates/diag` — вся логика без Tauri: профили, критерии виновников, разбор журналов SOCKS5-прокси/Discord, кандидаты путей, оценка и ранжирование, картины Р1–Р6, итог, отчёты, `circular.toml` и генерация `hub-circular`. Перебор идёт через трейт `Env`, который реализует `src-tauri` (ставит путь, наблюдает, пробует, спрашивает пользователя); тесты `diag` используют фейковый `Env`. `router` получает пассивное наблюдение за программой и временное правило; `probe` — пробы IP discovery, STUN и HTTPS с подстановкой адреса; `tester` открывает запуск стратегии на время; снимок `pending.json` получает раздел маршрутизации.

**Tech Stack:** Rust 2024 (toolchain из `rust-toolchain.toml`), Tauri 2 + tauri-specta, Svelte 5, WinDivert 2.2 (через `router::divert`), curl.exe, vitest.

**Spec:** `docs/superpowers/specs/2026-10-17-zapret-hub-diag-design.md` (разделы 1–16). Карта существующих интерфейсов (снята 2026-10-22, сверять с кодом): разделы ниже ссылаются на `file:line` текущего `master`.

## Global Constraints

- Только Windows; IPv6-потоки видны с пометкой «IPv6 не поддерживается», пути для них не проверяются (спец. 14).
- Крейты не зависят от `tauri`; `specta::Type` derive — можно. В экспортируемых типах нет `u64`/`i64` (specta): счётчики байтов — `f64`, времена — `u32` мс.
- Логика — в `crates/*`; команда в `src-tauri` — ~10 строк (`blocking(move || …)`).
- CLI-протокол с приложением — **5** (`crates/cli/src/lib.rs:19`); старый CLI получает отказ, как при прошлых повышениях.
- Тексты интерфейса и объяснения — русские; код, комментарии, коммиты — английские. Коммиты: одна строка, Conventional Commits, scope = область (`diag`, `router`, `probe`, `tester`, `zapret`, `zapret2`, `app`, `ui`, `cli`, `docs`), без тела и трейлеров.
- Никаких изменений системы в тестах: тесты с WinDivert, службами, правами администратора — `#[ignore = "…"]`, как `router/tests/live.rs`. Агент `--ignored` не запускает.
- Хаб **никогда не пишет** конфиг SOCKS5-прокси и журнал Discord — только читает (спец. 5).
- Значения из спецификации (использовать дословно):
  - наблюдение 30 с по умолчанию, допустимо 10–120 с; бюджет 10 мин по умолчанию;
  - TCP-виновник: ≥3 повтора SYN без ответа; RST или HTTP-редирект в первых 4 КБ ответа; ≥3 повтора данных в первых 32 КБ;
  - UDP-виновник: ≥4 пакетов туда и ≤1 обратно за окно наблюдения;
  - «плохой»: медиана RTT > 250 мс или потери UDP > 20 %;
  - группировка по домену и /24, до 3 представителей группы;
  - проба 3 раза, путь «работает» при ≥2 из 3, задержка — медиана;
  - «точечность» решает при равенстве задержек ±15 %;
  - `live` — повторное наблюдение 10–15 с (берём 12 с);
  - Р3: время исходящего соединения SOCKS5-прокси > 2 с у большинства соединений;
  - Р6: после `Sending MLS key package` ≥15 с нет `Received MLS commit`/`welcome`;
  - отчёты `data\diag\<дата-время>.json`, хранятся последние 20; профили-переопределения `data\diag\profiles\<id>.toml`; `data\diag\circular.toml`; стратегия `<папка zapret2>\strategies\hub-circular.txt`, имя `zapret2:hub-circular`;
  - имя правила «Применить» для прокси — `diag-<профиль>`.

## Review Focus

1. Хаб падает посреди перебора (стратегия и временное правило стоят, маршрутизация на паузе) → при следующем запуске служба, стратегия **и маршрутизация** (включена/правила) как до диагностики. Тест — Task 9 (`recover_restores_router_section`), Task 12 (`recover` в приложении).
2. Программа не запущена или за окно наблюдения не открыла ни одного соединения → итог «программа не соединялась за N с», а не «сеть в порядке». Тест — Task 3 (`no_flows_is_not_a_clean_bill`).
3. Журналов нет, они заблокированы или огромные (Discord `renderer_js.log` ~10 МБ) → источник пропущен с пометкой; читается только хвост после начала наблюдения. Тест — Task 4 (`missing_log_is_skipped`, `reads_only_lines_since_start`).
4. «Отмена» в момент, когда путь уже поставлен → откат выполнен, итог частичный, отчёт сохранён с `canceled: true`. Тест — Task 10 (`cancel_mid_path_rolls_back_and_keeps_partial`).
5. Прокси не настроен или не отвечает → пути через прокси пропущены с пояснением «прокси не отвечает», а не записаны как «не работает». Тест — Task 10 (`proxy_paths_skipped_when_proxy_down`).

---

## File Structure

```
crates/diag/                      новый крейт (deps: anyhow serde serde_json toml specta log exec probe router zapret zapret2)
  src/lib.rs                      pub mod-ы, общие типы (Proto, FlowKey-подобные), версии
  src/profiles.rs                 профили: встроенные TOML + переопределения
  src/builtin/{discord,dbd,youtube}.toml
  src/culprits.rs                 счётчики потока → виновник/«плохой»; группировка
  src/logs/mod.rs                 общее: чтение хвоста файла с момента времени
  src/logs/proxy_log.rs                proxy.log: домен → direct/proxy, длительности, DNS-ошибки
  src/logs/proxy_routes.rs         routes_box/<имя>: direct_domain/proxy_domain/DNS
  src/logs/discord.rs             renderer_js.log → состояние голоса (Р2, Р6)
  src/cover.rs                    что покрывает профиль стратегии: протокол, порты, l7
  src/candidates.rs               кандидаты-приёмы zapret2 для виновника
  src/circular_store.rs           data\diag\circular.toml + генерация hub-circular
  src/paths.rs                    пути, порядок, бюджет
  src/evaluate.rs                 оценка пути (пробы ×3, ≥2/3, медиана), ранжирование
  src/patterns.rs                 картины Р1–Р6
  src/summary.rs                  итог, объяснения, «Применить» (Fix), предупреждения
  src/run.rs                      трейт Env, сам перебор, отмена, откат
  src/report.rs                   отчёты: сохранить, список, последние 20
  tests/*.rs, tests/fixtures/*    по файлу на модуль
crates/router/src/observe.rs      пассивное наблюдение за программой (sniff), счётчики потоков
crates/router/src/lib.rs          + временное правило Router::set_temporary
crates/probe/src/udp.rs           IP discovery Discord, STUN
crates/probe/src/curl.rs          + https_via (--resolve, --socks5-hostname / --noproxy)
crates/tester/src/engine.rs       публичный запуск стратегии на время (из private launch)
crates/zapret/src/snapshot.rs     + раздел router в Snapshot
crates/zapret2/src/circular.rs    + UDP в merge
src-tauri/src/diag.rs             Env для приложения, задача диагностики, команды-обёртки
src-tauri/src/{state,jobs,commands,cli,lib,tray,texts,dto,settings}.rs   интеграция
crates/cli/src/{lib,main}.rs      протокол 5, группа diag
frontend/src/tabs/Diag.svelte, lib/strings.ts, lib/format.ts, App.svelte, lib/app.svelte.ts
README.md, docs/cli.md, CHANGELOG.md, Cargo.toml/package.json (0.8.0)
```

---

### Task 1: Крейт `diag` и профили

**Files:**
- Create: `crates/diag/Cargo.toml`, `crates/diag/src/lib.rs`, `crates/diag/src/profiles.rs`, `crates/diag/src/builtin/{discord,dbd,youtube}.toml`
- Modify: `Cargo.toml` (`[workspace.dependencies]` + `diag = { path = "crates/diag" }`)
- Test: `crates/diag/tests/profiles.rs`

**Interfaces:**
- Produces:
  - `pub enum ProbeKind { Https, DiscordIpDiscovery, Stun, Ping, Live }` (serde kebab-case: `https`, `discord-ip-discovery`, `stun`, `ping`, `live`)
  - `pub struct ProbeSpec { pub kind: ProbeKind, pub target: String }` (`target = "observed"` — адреса из наблюдения)
  - `pub struct Profile { pub id: String, pub name: String, pub apps: Vec<String>, pub hint: String, pub domains: Vec<String>, pub probes: Vec<ProbeSpec> }` (TOML-ключ `[[probe]]` → `probes` через `#[serde(rename = "probe")]`)
  - `pub fn builtin() -> Vec<Profile>`; `pub fn load(dir: &Path) -> anyhow::Result<Vec<Profile>>` (встроенные, файл `<dir>\<id>.toml` заменяет встроенный с тем же `id`, новые `id` добавляются в конец; битый файл — ошибка с именем файла); `pub fn any_program(exe: &str) -> Profile` (`id = "app"`, `name` = имя exe, `probes` = `[https observed, live observed]`, домены пусты)
  - Все типы — `Serialize, Deserialize, specta::Type, Clone, Debug, PartialEq`.

Встроенные (спец. 4): `discord` (apps `Discord.exe`, `DiscordPTB.exe`, `DiscordCanary.exe`; hint «Зайдите в голосовой канал»; domains `discord.com discord.gg discord.media discordapp.com discordapp.net`; probes https `discord.com`, discord-ip-discovery `observed`, stun `observed`), `dbd` (apps `DeadByDaylight-Win64-Shipping.exe`; hint «Начните поиск матча»; domains `bhvrdbd.com bhvronline.com`; probes https `steam.live.bhvrdbd.com`, live `observed`), `youtube` (apps `chrome.exe msedge.exe firefox.exe brave.exe opera.exe`; hint «Откройте видео на YouTube»; domains `youtube.com googlevideo.com`; probes https `www.youtube.com`).

- [ ] **Step 1: Тесты** `builtin_has_three_profiles` (`ids == ["discord","dbd","youtube"]`, у discord 3 пробы, первая `Https`/`discord.com`); `override_replaces_by_id_and_adds_new` (tempdir с `dbd.toml` hint «X» и `mine.toml` → hint dbd == «X», последний id == "mine", всего 4); `broken_override_names_the_file` (ошибка содержит `bad.toml`); `any_program_probes_observed` (`any_program("game.exe").probes` == https+live, обе `observed`).
- [ ] **Step 2:** `cargo nextest run -p diag --test profiles` → FAIL (нет крейта).
- [ ] **Step 3:** Крейт, `profiles.rs`, TOML через `include_str!`.
- [ ] **Step 4:** тест → PASS; `cargo clippy -p diag --all-targets -- -D warnings`.
- [ ] **Step 5:** `git commit -m "feat(diag): diagnostics crate with built-in and overridable profiles"`

### Task 2: Пассивное наблюдение за программой в `router`

**Files:**
- Create: `crates/router/src/observe.rs`
- Modify: `crates/router/src/lib.rs` (`pub mod observe;`)
- Test: `crates/router/tests/observe.rs` (чистая логика), `crates/router/tests/live.rs` (+1 `#[ignore]`)

**Interfaces:**
- Consumes: `divert::{Dll, Handle::open, Layer, FLAG_SNIFF, FLAG_RECV_ONLY, SOCKET_FILTER, DNS_FILTER, FILTER_BUDGET, MAX_FILTER_RANGES}`, `owner::{Images, image_path, process_start, table_pid}`, `dns::{Names, parse}`, `packet::{parse_tcp, parse_udp}`, `rules::{runs, Protocol}`.
- Produces:
  - `#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, specta::Type)] pub struct FlowCounters { pub protocol: Protocol, pub local_port: u16, pub dst: String /*ip:port*/, pub domain: Option<String>, pub sni: Option<String>, pub ipv6: bool, pub via_system_proxy: bool, pub up_packets: u32, pub down_packets: u32, pub up_bytes: f64, pub down_bytes: f64, pub syns: u32 /*SYN всего, с повторами*/, pub syn_answered: bool, pub syn_ack_ms: Option<u32>, pub rst_early: bool, pub redirect_early: bool, pub data_retrans: u32, pub udp_first_reply_ms: Option<u32>, pub udp_windows: u32, pub udp_silent_windows: u32, pub rtt_ms: Vec<u32> }` (`rst_early`/`redirect_early` — в первых 4096 байт ответа; `data_retrans` считается только в первых 32768 байт отправленного; `udp_windows`/`udp_silent_windows` — окна по 1 с, «тихое» = были пакеты туда и ни одного обратно)
  - `pub struct Tally(HashMap<(Protocol, u16, SocketAddr), FlowCounters>)` с `pub fn outbound(&mut self, packet: &[u8], now: Instant)`, `pub fn inbound(&mut self, packet: &[u8], now: Instant)`, `pub fn tick(&mut self, now: Instant)` (закрывает 1-секундные окна UDP), `pub fn rows(&self) -> Vec<FlowCounters>` — чистая логика, разбор через `packet::parse_*`; SNI — из первого ClientHello на TCP 443, `Host:` — из HTTP на 80; `via_system_proxy = true`, если `dst` — loopback и порт == `proxy_port`.
  - `pub fn sniff_filter(ports: &[u16]) -> Option<String>` — `ip and (tcp or udp) and (localPort == a or …)` с сжатием в диапазоны по бюджету `FILTER_BUDGET`/`MAX_FILTER_RANGES` (переиспользовать приём `fit` из `divert.rs`, вынести общий помощник, если он private); `None` для пустого списка.
  - `pub struct Observer` с `pub fn start(dll: &Path, apps: Vec<String>, proxy_port: Option<u16>) -> anyhow::Result<Observer>` (свои handles: SOCKET-sniff для портов программ — совпадение по `rules::runs(app, exe)`; DNS-sniff → `Names`; сетевой sniff по `sniff_filter`, переоткрывается при смене набора портов не чаще раза в 1 с), `pub fn snapshot(&self) -> Vec<FlowCounters>` (с `domain` из `Names`), `pub fn stop(self) -> Vec<FlowCounters>`. IPv6-пакеты: поток с `ipv6: true`, без счётчиков SYN.

- [ ] **Step 1: Тесты** в `tests/observe.rs` на синтетических пакетах (`packet::build_udp` и ручная сборка TCP-заголовка в помощнике теста): `three_unanswered_syns_count` (3 исходящих SYN одного потока без SYN-ACK → `syns == 3`, `syn_answered == false`); `rst_in_first_4k_is_early` (SYN-ACK, 1000 байт данных, RST → `rst_early`), `rst_after_4k_is_not`; `http_redirect_in_first_4k` (ответ `HTTP/1.1 302` → `redirect_early`); `data_retrans_only_within_32k`; `udp_silent_windows` (5 окон: 4 пакета туда/0 обратно в 3 окнах → `udp_silent_windows == 3`); `sni_from_client_hello` (фикстура ClientHello с SNI `discord.com`); `loopback_proxy_port_marks_system_proxy`; `sniff_filter_compresses_to_budget` (300 портов → фильтр компилируемой длины: число `localPort` сравнений ≤ `MAX_FILTER_RANGES`; `Dll` не нужен — проверка по тексту); `empty_ports_no_filter`.
- [ ] **Step 2:** `cargo nextest run -p router --test observe` → FAIL.
- [ ] **Step 3:** `observe.rs`: `Tally`, `sniff_filter`, `Observer`.
- [ ] **Step 4:** Живой тест `observes_curl` (`#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL set"]`): `Observer::start(dll, vec!["curl.exe".into()], None)`, запустить `curl.exe -s https://example.com`, `stop()` → есть TCP-поток на 443 с `sni == Some("example.com")`. Не запускать.
- [ ] **Step 5:** `cargo nextest run -p router` → PASS; clippy.
- [ ] **Step 6:** `git commit -m "feat(router): passive per-program flow observation for diagnostics"`

### Task 3: Виновники и группировка

**Files:**
- Create: `crates/diag/src/culprits.rs`
- Test: `crates/diag/tests/culprits.rs`

**Interfaces:**
- Consumes: `router::observe::FlowCounters`.
- Produces:
  - `#[serde(tag = "kind")] pub enum Fault { SynUnanswered { syns: u32 }, ResetEarly, RedirectEarly, DataRetrans { count: u32 }, UdpNoReply { up: u32, down: u32 }, Slow { median_ms: u32 }, Lossy { loss_pct: u32 } }`
  - `pub fn fault(flow: &FlowCounters) -> Option<Fault>` — порядок проверки: SYN (`syns >= 3 && !syn_answered`; решение: «≥3 повтора SYN» спецификации читается как 3 SYN всего — Windows по умолчанию шлёт SYN и 2 повтора, буквальные 3 повтора не наступили бы никогда) → `rst_early` → `redirect_early` → `data_retrans >= 3` → UDP (`up_packets >= 4 && down_packets <= 1`) → «плохой» (медиана `rtt_ms` > 250 → `Slow`; потери UDP = `udp_silent_windows*100/udp_windows` > 20 → `Lossy`). IPv6 и `via_system_proxy` → `None` (у них своя обработка).
  - `pub struct Group { pub key: String /*домен или a.b.c.0/24*/, pub protocol: Protocol, pub ports: Vec<u16>, pub members: Vec<FlowCounters>, pub representatives: Vec<FlowCounters> /*≤3*/, pub fault: Fault }`
  - `pub fn group(flows: &[FlowCounters]) -> Vec<Group>` — только потоки с `fault`; ключ — `domain` (или `sni`), иначе /24 адреса; представители — первые 3 по убыванию `up_packets`; `fault` группы — у первого представителя.
  - `pub enum Verdict { NoFlows, Clean, Culprits(Vec<Group>) }` и `pub fn verdict(flows: &[FlowCounters]) -> Verdict`.

- [ ] **Step 1: Тесты:** пороги дословно из спецификации — `two_syns_are_not_enough` (`syns: 2` → `None`), `three_syns_fault`; `udp_four_up_one_down_fault`, `udp_four_up_two_down_ok`; `median_251_is_slow`, `median_250_is_not`; `loss_21_pct_lossy`; `ipv6_and_system_proxy_are_not_culprits`; `groups_by_domain_then_slash24` (10 потоков на `1.2.3.4..13` без домена → одна группа `1.2.3.0/24`, 3 представителя); `no_flows_is_not_a_clean_bill` (`verdict(&[]) == NoFlows`, а поток без сбоев → `Clean`).
- [ ] **Step 2–4:** FAIL → реализация → PASS.
- [ ] **Step 5:** `git commit -m "feat(diag): culprit criteria and grouping of observed flows"`

### Task 4: Журналы SOCKS5-прокси и Discord

**Files:**
- Create: `crates/diag/src/logs/{mod,proxy_log,proxy_routes,discord}.rs`, `crates/diag/tests/fixtures/{proxy.log,proxy-slow-dns.log,routes_box.json,discord-dave-stalled.log,discord-dave-alone.log,discord-dave-done.log}`
- Test: `crates/diag/tests/logs.rs`

Фикстуры собрать из строк раздела 16 спецификации (адреса VPN-сервера — `203.0.113.10`, не настоящие).

**Interfaces:**
- Produces:
  - `logs::tail_since(path: &Path, max_bytes: u64, keep: impl Fn(&str) -> bool) -> std::io::Result<Option<Vec<String>>>` — `Ok(None)`, если файла нет; читает не больше последних `max_bytes` (8 МБ), строки, для которых `keep` истинно; файл открывается с общим доступом на чтение/запись (журналы пишутся в это время).
  - `proxy_log::Route { Direct, Proxy, Other(String) }`; `proxy_log::Conn { pub host: String, pub port: u16, pub route: Route, pub outbound_ms: Option<u32> }`; `proxy_log::parse(lines: &[String]) -> Vec<Conn>` (пара `inbound connection to <host>:<port>` и `outbound/<type>[<tag>]: outbound connection to …` с тем же `[id …]`; `direct`/`bypass` → `Direct`, `vless`/`vmess`/`trojan`/`shadowsocks`/`hysteria*`/`tuic`/тег `proxy` → `Proxy`; длительность `[id 166ms]`/`[id 8.85s]` у второй outbound-строки → `outbound_ms`); `proxy_log::slow_dns(conns: &[Conn]) -> bool` (больше половины с `outbound_ms > 2000`, минимум 5 соединений); `proxy_log::route_of(conns, domain) -> Option<Route>` (последнее соединение к домену или его поддомену). Время строк SOCKS5-прокси — относительное (`INFO[0570]`): «с начала наблюдения» = строки после последней строки, бывшей в файле при старте (передаётся число строк/смещение).
  - `proxy_routes::Routes { pub direct: Vec<String>, pub proxy: Vec<String>, pub direct_dns: String, pub remote_dns: String }`; `proxy_routes::read(path: &Path) -> anyhow::Result<Routes>` (JSON-поля `direct_domain`/`proxy_domain` — строки с `\n`); `proxy_routes::rule_for(&Routes, domain) -> Option<(Route, String /*строка правила*/)>`.
  - `discord::Voice { pub media_server: Option<String>, pub local_address: Option<String> /*из "Connected with local address"*/, pub own_ssrc: Option<u32>, pub peers: Vec<u32> /*SSRC других из "Creating user"*/, pub dave: Dave, pub no_voice_state: bool, pub ws_closed: Vec<String> }`; `discord::Dave { NotStarted, KeyPackageSent { secs_waiting: u32 }, Done, Disabled }`; `discord::parse(lines: &[String], now: NaiveTime-подобное время из последней строки) -> Voice` (время строк — `[YYYY-MM-DD HH:MM:SS.mmm]`; `Done`, если после `Sending MLS key package` есть `Received MLS commit` или `Sending MLS commit + welcome message` или `epochAuthenticator` непустой); `discord::stalled(&Voice) -> bool` (`KeyPackageSent { secs_waiting >= 15 }`).
  - `pub fn discord_log_paths() -> Vec<PathBuf>` (`%APPDATA%\discord\logs\renderer_js.log`, `discordptb`, `discordcanary` — существующие).

- [ ] **Step 1: Тесты:** `proxy_log_pairs_inbound_with_outbound` (фикстура: `c-arn08-a4ab867f.discord.media:2096` → `Direct`, после правки — `Proxy`); `proxy_log_slow_dns` (фикстура с 6 соединениями по 5–10 с → true; обычная → false); `routes_box_rule_for_discord_media` (`direct_domain` содержит `discord.media` → `(Direct, "discord.media")`); `discord_stalled_with_peer` (фикстура stalled: `own_ssrc == 11029`, `peers == [10923]`, `local_address` = `203.0.113.10`, `stalled == true`); `discord_alone_stalled` (peers пуст); `discord_done` (`dave == Done`); `missing_log_is_skipped` (`tail_since` несуществующего → `Ok(None)`); `reads_only_lines_since_start` (keep по времени отбрасывает строки раньше старта); `tail_limits_bytes` (файл 10 МБ → прочитано ≤ 8 МБ, последняя строка на месте).
- [ ] **Step 2–4:** FAIL → реализация → PASS.
- [ ] **Step 5:** `git commit -m "feat(diag): read SOCKS5 proxy and Discord logs for routes and voice state"`

### Task 5: Пробы IP discovery, STUN, HTTPS через адрес и прокси

**Files:**
- Create: `crates/probe/src/udp.rs`
- Modify: `crates/probe/src/lib.rs`, `crates/probe/src/curl.rs`
- Test: `crates/probe/tests/udp.rs`, `crates/probe/tests/curl.rs` (+ чистые тесты аргументов)

**Interfaces:**
- Produces:
  - `pub fn ip_discovery(dst: SocketAddr, ssrc: u32, timeout: Duration) -> Option<u32 /*мс*/>` — Discord IP discovery: 74 байта, тип `0x0001`, длина `70`, SSRC big-endian, остальное нули; успех — ответ типа `0x0002` того же SSRC.
  - `pub fn stun(dst: SocketAddr, timeout: Duration) -> Option<u32>` — Binding Request (тип `0x0001`, длина 0, magic cookie `0x2112A442`, 12 байт transaction id); успех — Binding Success `0x0101` с тем же transaction id.
  - `pub enum Via { Direct, Socks5(String) }`; `pub fn https_via(host: &str, ip: Option<IpAddr>, via: &Via, cancel: Option<&AtomicBool>) -> CheckResult` — `curl.exe -I -s -m 4 --connect-timeout 2 -o NUL -w "%{http_code} %{time_total}" --show-error`, `--resolve host:443:ip` при `ip`, `--socks5-hostname addr` или `--noproxy *`; результат классифицируется существующими `classify`/`parse_code_and_time`. Аргументы собирает `pub fn https_args(host, ip, via) -> Vec<String>` (тестируемо без сети).
  - `pub fn dns_udp(name: &str, server: SocketAddr, timeout: Duration) -> Option<Vec<Ipv4Addr>>` — один A-запрос по UDP 53 (`None` — нет ответа; `Some(vec![])` — NXDOMAIN или без A); `pub fn dns_doh(name: &str, socks5: Option<&str>) -> Option<Vec<Ipv4Addr>>` — curl к `https://cloudflare-dns.com/dns-query?name=<name>&type=A` с заголовком `accept: application/dns-json`, разбор `Answer[].data` типа 1. Для картины Р4: расхождение = UDP дал пусто/NXDOMAIN, DoH — адреса.

- [ ] **Step 1: Тесты:** loopback-эхо на `UdpSocket` в потоке теста: `ip_discovery_answered` (сервер отвечает пакетом типа 2 с тем же SSRC → `Some`), `ip_discovery_wrong_ssrc_is_none`, `stun_answered`, `stun_silence_is_none` (timeout 200 мс); `https_args_resolve_and_socks` (`["…","--resolve","discord.com:443:1.2.3.4","--socks5-hostname","127.0.0.1:2080","https://discord.com"]` — порядок как в реализации, проверить наличие пар), `https_args_direct_has_noproxy`; `dns_udp_parses_a_and_nxdomain` (loopback-сервер отвечает A `1.2.3.4`, затем RCODE 3 → `Some([1.2.3.4])`, `Some([])`); `doh_json_parsed` (чистая функция `parse_doh_json(text) -> Vec<Ipv4Addr>` на образце JSON).
- [ ] **Step 2–4:** FAIL → реализация → PASS.
- [ ] **Step 5:** `git commit -m "feat(probe): Discord IP discovery, STUN and pinned-address HTTPS and DNS probes"`

### Task 6: Что покрывает стратегия и кандидаты-приёмы zapret2

**Files:**
- Create: `crates/diag/src/cover.rs`, `crates/diag/src/candidates.rs`
- Test: `crates/diag/tests/candidates.rs` (фикстуры — `crates/zapret2/tests/fixtures/ALT11.txt` и одна Flowseal `.args` из `crates/zapret/tests/fixtures`, скопировать в `crates/diag/tests/fixtures/`)

**Interfaces:**
- Consumes: `zapret2::circular::{analyze, Profile}`, `zapret2::strategies::{split, args}`, `zapret::custom::quoted_tokens`.
- Produces:
  - `pub struct Cover { pub tcp: Vec<(u16, u16)>, pub udp: Vec<(u16, u16)>, pub l7: Vec<String>, pub lists: bool /*есть --hostlist/--ipset*/ }`; `pub fn cover(profile_filter: &str) -> Cover` (разбирает `--filter-tcp=`, `--filter-udp=`, `--filter-l7=`; плейсхолдеры `%GameFilter…%` считаются диапазоном `1024-65535`); `pub fn covers(c: &Cover, protocol: Protocol, port: u16) -> bool`.
  - `pub fn strategy_covers(args: &str, protocol: Protocol, port: u16) -> bool` — для Flowseal (`final_args`) и zapret2 (`strategies::args`): хоть один профиль (`--new`) покрывает.
  - `pub struct Candidate { pub source: String /*полное имя стратегии*/, pub profile: u32, pub text: String /*стратегия из одного профиля*/ }`; `pub fn candidates(strategies: &[(String /*full name*/, String /*text*/)], group: &Group) -> Vec<Candidate>` — по профилю, чей `cover` покрывает протокол и первый порт группы; в тексте кандидата все `--hostlist*`/`--ipset*` профиля заменены на `--ipset-ip=<a.b.c.0/24>` (группа по адресу) или `--hostlist-domains=<домен>` (группа по домену); глобальная часть стратегии (`--wf-*`, `--lua-init`, `--blob`) сохраняется; дубликаты по тексту профиля убираются.

- [ ] **Step 1: Тесты:** `cover_reads_ports_and_l7`; `game_filter_covers_high_ports`; `alt11_covers_udp_19300` (Discord-голос); `candidate_replaces_lists_with_group_ip` (в тексте кандидата нет `--hostlist=`, есть `--ipset-ip=104.29.149.0/24`, есть исходный `--lua-desync`); `candidate_by_domain_uses_hostlist_domains`; `identical_profiles_deduplicated`.
- [ ] **Step 2–4:** FAIL → реализация → PASS.
- [ ] **Step 5:** `git commit -m "feat(diag): strategy coverage and zapret2 trick candidates for a culprit"`

### Task 7: UDP в circular и хранилище `hub-circular`

**Files:**
- Modify: `crates/zapret2/src/circular.rs` (ветка `NotTcp` ~:643)
- Create: `crates/diag/src/circular_store.rs`
- Test: `crates/zapret2/tests/circular.rs` (+), `crates/diag/tests/circular_store.rs`

UDP-ротация: профиль с `--filter-udp=` (без `--filter-tcp=`) теперь ротируется; в настройки `circular` такого профиля пишется детектор UDP (`udp_out=4:udp_in=1`, значения порогов раздела 5) и `--in-range`/`--out-range` по счёту пакетов (`-n<N>` вместо `-s<байт>`; N сверить с руководством zapret2 `docs/` — раздел circular и `standard_failure_detector`; записать выбранное в комментарий над константой и в `12`-подобный раздел решений спецификации zapret2). `NoteKind::NotTcp` остаётся только для профиля, у которого и TCP, и UDP.

**Interfaces:**
- Produces (`diag::circular_store`):
  - `pub struct Trick { pub source: String, pub text: String, pub found: String /*дата*/, pub median_ms: Option<u32>, pub ok: u32, pub total: u32 }`; `pub struct GroupTricks { pub filter: String /*протокол+порты+l7*/, pub tricks: Vec<Trick> /*по рангу*/ }`; `pub struct Store { pub groups: Vec<GroupTricks> }`
  - `pub fn load(path: &Path) -> anyhow::Result<Store>` (нет файла → пусто); `pub fn save(path: &Path, store: &Store) -> anyhow::Result<()>` (tmp + rename)
  - `pub fn add(store: &mut Store, filter: &str, found: Vec<Trick>)` — пополняет группу, сработавшие переупорядочены по (ok desc, median asc), дубли по `text` заменяются новыми измерениями.
  - `pub fn generate(current: &str /*текст текущей стратегии zapret2*/, store: &Store) -> anyhow::Result<String>` — для каждой группы с ≥2 приёмами профиль из `circular::merge` её приёмов (≥2..=5, лучшие первыми), с 1 приёмом — профиль как есть; группы встают **перед** профилями `current`; глобальная часть — из `current` плюс `--lua-init`/`--blob` приёмов без дублей. Результат проходит `zapret2::circular::analyze` без `errors`.
  - `pub const NAME: &str = "hub-circular";`

- [ ] **Step 1: Тесты:** zapret2 `udp_profiles_rotate` (две стратегии с одинаковым `--filter-udp=50000-50100` → нота `Rotates`, в тексте `circular` и `udp_out`), `mixed_tcp_udp_still_not_tcp`; diag `add_reorders_by_ok_then_median`, `generate_puts_groups_first` (первый `--new`-профиль результата — группа; последний — последний профиль `current`), `generate_single_trick_no_circular`, `generate_parses_clean`.
- [ ] **Step 2–4:** FAIL → реализация → PASS (`cargo nextest run -p zapret2 -p diag`).
- [ ] **Step 5:** два коммита: `feat(zapret2): rotate UDP profiles in circular merge` и `feat(diag): keep found tricks and build the hub-circular strategy`

### Task 8: Запуск стратегии на время (`tester::engine`)

**Files:**
- Create: `crates/tester/src/engine.rs`
- Modify: `crates/tester/src/run.rs` (private `launch`/`Launch`/`stop_engines` переезжают в `engine.rs`, `run.rs` их использует), `crates/tester/src/lib.rs`
- Test: `crates/tester/tests/engine.rs`

**Interfaces:**
- Produces:
  - `pub struct Launch { pub exe: PathBuf, pub cwd: PathBuf, pub tail: String, pub zapret2: bool }` (+ существующие поля `sha256`)
  - `pub fn launch(folder: Option<&Folder>, engine2: Option<&Engine>, name: &str) -> anyhow::Result<Launch>` (поведение как было)
  - `pub fn launch_text(engine2: &Engine, folder: Option<&Folder>, text: &str) -> anyhow::Result<Launch>` (zapret2-стратегия из текста — для кандидатов и `hub-circular`; `strategies::tail` + `check`)
  - `pub fn stop_engines() -> anyhow::Result<()>` (остановить обе службы, `kill_all` обоих образов — как в `run_strategies`)
  - `pub struct Running(procs::Winws)`; `pub fn start(launch: &Launch) -> anyhow::Result<Running>` (`stop_engines`, `Winws::start`, `wait_ready(5 с)`, пауза 300 мс / 2 с для winws2 — как в `run_one`); `impl Running { pub fn stop(self) -> String; pub fn exited(&mut self) -> Option<ExitStatus> }`

- [ ] **Step 1: Тесты** (без прав): `launch_unknown_name_errors` (`launch(None, None, "nope")` → ошибка «…folder is not set…» как раньше), `launch_text_rejects_bad_args` (fake_engine + текст с `--lua-desync=` без функции → ошибка `check`). Существующие тесты `tester` — зелёные.
- [ ] **Step 2–4:** FAIL → перенос → PASS (`cargo nextest run -p tester`).
- [ ] **Step 5:** `git commit -m "refactor(tester): expose starting a strategy for a while"`

### Task 9: Раздел маршрутизации в снимке и временное правило

**Files:**
- Modify: `crates/zapret/src/snapshot.rs`, `crates/router/src/lib.rs`
- Test: `crates/zapret/tests/snapshot.rs` (+), `crates/router/tests/router.rs` (+)

**Interfaces:**
- Produces:
  - `zapret::snapshot::RouterSnap { pub enabled: bool, pub paused: bool }` и поле `#[serde(default)] pub router: Option<RouterSnap>` в `Snapshot` (старые `pending.json` читаются). Восстановление маршрутизации делает приложение (Task 12): `zapret` о роутере не знает. Правила `routes.toml` диагностика не меняет (временное правило — в памяти), поэтому в снимке их нет.
  - `router::Router::set_temporary(&self, rule: Option<Rule>) -> Result<()>` — правило поверх правил из файла (проверка `rules::check`), **не пишется** в `routes.toml`, исчезает при `set_temporary(None)` и при `stop`; `Router::temporary(&self) -> Option<Rule>`.

- [ ] **Step 1: Тесты:** `old_pending_without_router_loads` (JSON без поля → `router == None`), `router_section_roundtrips`; `recover_restores_router_section` — сериализовать `Snapshot{router: Some(RouterSnap{enabled: true, paused: false}), ..}`, `load` → то же значение; router: `temporary_rule_not_saved` (tempdir routes.toml, `set_temporary(Some(rule))` → файл не изменился, `rules()` содержит правило; `set_temporary(None)` → нет), `temporary_rule_checked` (пустое правило → ошибка поля), `stop_clears_temporary`.
- [ ] **Step 2–4:** FAIL → реализация → PASS.
- [ ] **Step 5:** `git commit -m "feat(router): temporary rule and router state in the rollback snapshot"`

### Task 10: Перебор путей, оценка, ранжирование, отмена

**Files:**
- Create: `crates/diag/src/{paths,evaluate,run}.rs`
- Test: `crates/diag/tests/run.rs` (фейковый `Env`)

**Interfaces:**
- Consumes: Task 1, 3, 6.
- Produces:
  - `#[serde(tag = "kind")] pub enum PathKind { Current, Direct, ProxyPoint { rule: Rule }, ProxyNoZapret { rule: Rule }, Strategy { name: String }, Trick { candidate: Candidate }, ExtendRule { rule: String, protocol: Protocol } }`; `pub fn label(&self) -> String` (русская подпись).
  - `pub struct Ctx { pub proxy_ok: bool, pub ranking: Vec<String> /*по последнему рейтингу*/, pub strategies: Vec<(String, String)> /*полное имя, args для cover*/, pub zapret2: Vec<(String, String)> /*для кандидатов*/, pub app_rules: Vec<Rule> /*правила маршрутов с apps*/, pub profile: Profile }`
  - `pub fn plan(group: &Group, ctx: &Ctx) -> (Vec<PathKind>, Vec<String> /*пропущенные с причиной*/)` — порядок спец. 6: Current → Direct → ProxyPoint (только `proxy_ok`; иначе в пропущенные «прокси не отвечает») → ProxyNoZapret (только `proxy_ok`) → ExtendRule (если есть правило с `apps` программы профиля и протоколом, не покрывающим протокол группы; Р1) → стратегии из `ranking`, чьи args покрывают протокол+порт → Trick по `candidates`. Правило ProxyPoint: имя `diag-<profile.id>`, `ips` = ключ группы (/24) или `domains` = домен, `ports` = порты группы, `protocol` группы, `apps` = `profile.apps`.
  - `pub struct Score { pub ok: u32, pub total: u32, pub median_ms: Option<u32>, pub loss_pct: Option<u32>, pub answer: Option<Answer> }`; `pub enum Answer { Yes, No, Unknown }`; `pub fn works(s: &Score) -> bool` (`answer` Yes/No решает, иначе `ok * 3 >= total * 2`, т.е. ≥2 из 3); `pub fn rank(results: &mut [(PathKind, Score)])` — работает > не работает; медиана; потери; при медианах в пределах ±15 % — точечные (`ProxyPoint`, `Trick`, `ExtendRule`) выше `Strategy`/`Direct`.
  - `pub trait Env { fn set_path(&mut self, path: &PathKind, group: &Group) -> anyhow::Result<()>; fn probe(&mut self, group: &Group, probe: &ProbeSpec) -> Score /*3 попытки внутри*/; fn ask(&mut self, hint: &str, path: &PathKind) -> Answer; fn observe(&mut self, secs: u32) -> Vec<FlowCounters>; fn rollback(&mut self) -> anyhow::Result<()>; fn cancelled(&self) -> bool; fn elapsed(&self) -> Duration; fn progress(&mut self, step: Progress); }`
  - `#[serde(tag = "step")] pub enum Progress { Observing { secs_left: u32, flows: Vec<FlowCounters> }, Culprits { groups: Vec<Group> }, Path { group: u32, index: u32, total: u32, label: String }, Question { label: String, hint: String }, Rechecking, RollingBack }`
  - `pub enum Mode { Auto, Ask }`; `pub struct Settings { pub observe_secs: u32 /*10..=120*/, pub budget: Duration, pub mode: Mode }`
  - `pub fn run(env: &mut dyn Env, ctx: &Ctx, groups: &[Group], settings: &Settings) -> Outcome` — для каждой группы: `plan`, по пути `set_path` → пробы профиля (`Live` → `observe(12)` и `culprits::fault` по представителям; `Ask` → и `ask`) → `Score`; бюджет (`elapsed() >= budget`) → остановка, остаток в `not_checked`; `cancelled()` → остановка; **`rollback()` всегда в конце** (и при ошибке `set_path`, путь — «не удалось поставить: <ошибка>»). `pub struct Outcome { pub per_group: Vec<GroupResult>, pub canceled: bool, pub budget_hit: bool, pub rollback_error: Option<String> }`; `pub struct GroupResult { pub group: Group, pub results: Vec<(PathKind, Score)> /*ранжированы*/, pub skipped: Vec<String>, pub not_checked: Vec<String> }`.

- [ ] **Step 1: Тесты** с `struct Fake { works: HashSet<String /*label*/>, cancel_at: Option<usize>, sets: Vec<String>, rolled_back: u32, clock: Duration }`: `order_follows_spec` (`plan` даёт Current, Direct, ProxyPoint, ProxyNoZapret, Strategy…, Trick…); `proxy_paths_skipped_when_proxy_down` (`proxy_ok: false` → нет ProxyPoint/ProxyNoZapret, в `skipped` «прокси не отвечает»); `extend_rule_for_udp_only_app_rule` (правило `DBD Process` udp + TCP-группа → `ExtendRule{protocol: Both}`); `two_of_three_works` (`ok:2,total:3` → works; `1/3` → нет); `rank_prefers_pointed_within_15pct` (Strategy 100 мс и ProxyPoint 110 мс → ProxyPoint первый; 100 и 120 → Strategy первый); `budget_stops_and_lists_rest`; `cancel_mid_path_rolls_back_and_keeps_partial` (`cancel_at: Some(2)` → `canceled`, `rolled_back == 1`, результаты первых путей сохранены); `set_path_error_recorded_and_rollback_still_runs`; `ask_mode_answer_overrides_probes` (`Answer::No` при 3/3 → не работает).
- [ ] **Step 2–4:** FAIL → реализация → PASS.
- [ ] **Step 5:** `git commit -m "feat(diag): try paths per culprit, rank them and always roll back"`

### Task 11: Картины Р1–Р6, итог, «Применить», отчёты

**Files:**
- Create: `crates/diag/src/{patterns,summary,report}.rs`
- Test: `crates/diag/tests/summary.rs`

**Interfaces:**
- Consumes: Task 3, 4, 10.
- Produces:
  - `#[serde(tag = "id")] pub enum Pattern { R1RuleMissesProtocol { rule: String, has: Protocol, needs: Protocol }, R2VoiceUdp { server: String, via_vpn: bool }, R3SlowProxyDns { median_ms: u32, direct_dns: String }, R4DnsSpoofed { domain: String }, R5Whitelist, R6DaveStalled { alone: bool, control_route: Option<String> } }`
  - `pub struct Evidence { pub flows: Vec<FlowCounters>, pub proxy_log: Option<Vec<proxy_log::Conn>>, pub proxy_routes: Option<Routes>, pub voice: Option<discord::Voice>, pub proxy_ip: Option<String>, pub dns_mismatch: Vec<String> /*домены: UDP 53 и DoH расходятся*/, pub ru_ok: Option<bool> /*https ya.ru*/, pub foreign_syn_dead: Option<bool> }`
  - `pub fn detect(evidence: &Evidence, outcome: &Outcome, ctx: &Ctx) -> Vec<Pattern>` — все подходящие, не первая (спец. 16): Р1 — есть `ExtendRule`, и он работает; Р2 — UDP-группа на `104.29.0.0/16` или `35.217.0.0/16`, `via_vpn` = `voice.local_address == proxy_ip`; Р3 — `proxy_log::slow_dns`; Р4 — `dns_mismatch` не пуст; Р5 — `ru_ok == Some(true) && foreign_syn_dead == Some(true)`; Р6 — `discord::stalled(voice)`, `alone = peers.is_empty()`, `control_route` — маршрут `*.discord.media` из `proxy_log`.
  - `#[serde(tag = "kind")] pub enum Fix { AddRule { rule: Rule }, EditRule { name: String, protocol: Protocol }, ApplyStrategy { name: String }, ApplyHubCircular { tricks: Vec<Trick>, filter: String }, TurnOff { what: String }, Manual { text: String } }`
  - `pub struct Summary { pub id: String /*дата-время*/, pub profile: String, pub verdict_text: String, pub groups: Vec<GroupSummary>, pub patterns: Vec<Pattern>, pub explanations: Vec<String>, pub fixes: Vec<Fix>, pub warnings: Vec<String>, pub canceled: bool, pub budget_hit: bool, pub rollback_error: Option<String>, pub sources_missing: Vec<String> }`; `GroupSummary { group: Group, seen: String /*«UDP уходит, ответов нет»*/, results: Vec<(PathKind, Score)>, best: Option<u32>, explanation: String }`
  - `pub fn summarize(verdict: &Verdict, outcome: &Outcome, patterns: &[Pattern], ctx: &Ctx, id: String) -> Summary` — тексты по шаблонам спец. 8 и 16 (дословно: «провайдер блокирует адрес; zapret не помогает; через прокси работает»; «мешает текущая стратегия/маршрутизация»; «DPI провайдера; помогает приём X»; «адрес недоступен ни одним путём, включая прокси: вероятно, проблема на стороне сервиса или прокси»; Р6 — «сеть в порядке: соединение с голосовым сервером есть, но собеседник не прислал ключи шифрования» + «пусть собеседник перезайдёт или перезапустит Discord; проверить себя — зайти в канал одному»; Р5 — «провайдер пускает только в разрешённые сети; ни zapret, ни прокси это не обходят»; SOCKS5-прокси — «Настройки → Маршрутизация → Basic: убрать `<домен>` из Direct, добавить в Proxy»). `Verdict::NoFlows` → «программа не соединялась за N с: запустите её и повторите»; `Clean` → «соединения программы проходят; если проблема есть, она не в сети».
  - `pub fn warn_breaks(before: &[(String, bool)], after: &[(String, bool)]) -> Vec<String>` — «сломает: <профиль> (<что не отвечает>)» для профиля, рабочего до и нерабочего после.
  - `report::dir(data: &Path) -> PathBuf` (`data\diag`); `report::save(dir, &Summary) -> anyhow::Result<()>` (`<id>.json`, затем удалить старые сверх 20); `report::list(dir) -> anyhow::Result<Vec<(String, String /*профиль*/)>>` (новые первыми); `report::load(dir, id) -> anyhow::Result<Summary>`.

- [ ] **Step 1: Тесты:** по картине на синтетике — `r1_from_extend_rule`, `r2_voice_via_vpn`, `r3_slow_dns`, `r4_mismatch`, `r5_whitelist_skips_paths`, `r6_with_peer_blames_peer`, `r6_alone_blames_us`, `several_patterns_at_once` (Р2+Р6 из одного разбора); тексты: `only_proxy_works_text`, `nothing_works_text`, `no_flows_text`; `warn_breaks_only_regressions`; отчёты: `keeps_last_20` (25 сохранений → 20 файлов, самые новые), `list_newest_first`.
- [ ] **Step 2–4:** FAIL → реализация → PASS.
- [ ] **Step 5:** `git commit -m "feat(diag): failure patterns, explanations, fixes and reports"`

### Task 12: Приложение: задача диагностики, Env, откат, команды

**Files:**
- Create: `src-tauri/src/diag.rs`
- Modify: `src-tauri/src/{state,jobs,commands,lib,dto,texts,settings,routing}.rs`, `src-tauri/Cargo.toml` (`diag`)
- Test: unit-тесты в `src-tauri/src/diag.rs` и `state.rs` (`cargo test -p zapret-hub --lib`)

**Interfaces:**
- Consumes: всё выше; `AppState`, `routing::paused`, `snapshot::begin`, `tester::engine`, `actions::apply`, `Router::{edit, set_temporary, set_enabled}`.
- Produces:
  - `AppState`: флаг `diagnosing: AtomicBool` + `begin_diag()/end_diag()/cancel_diag()`; `change_any`/`begin_testing` отказывают при идущей диагностике текстом `const DIAGNOSING: &str = "diagnostics are running: wait for them or cancel them";`, `start_diag` — при идущем тесте `TESTING`. `Paths::diag() -> PathBuf` (`data\diag`).
  - `Settings`: `diag_observe_secs: u32` (30), `diag_budget_min: u32` (10), `proxy_client_dir: Option<String>` (None → путь процесса `proxy-client.exe`/`proxy-client-core.exe` через `zapret::procs::find`, папка `config` рядом). По чек-листу добавления настройки (карта интерфейсов, раздел 7); `to_save` их не защищает.
  - `diag::Live { pub request: DiagRequest, pub progress: Option<diag::Progress>, pub question: Option<String> }`; `DiagRequest { pub profile: Option<String>, pub app: Option<String>, pub mode: diag::Mode }`.
  - `pub fn start_diag(app: &AppHandle, request: DiagRequest) -> anyhow::Result<()>` (как `jobs::start_tests`): поток → `routing::paused`-область только на время путей «напрямую»/стратегий (временное правило ставится при `ProxyPoint`, роутер снимается с паузы), `snapshot::begin(folder, "diag", …)` с `router: Some(RouterSnap{enabled, paused})`, наблюдение (`Observer` с `routing::setup(state).dll` и портом прокси), `diag::run` с `AppEnv`, `rollback` = `snapshot.restore` + `set_temporary(None)` + роутер в состояние снимка, сбор `Evidence` (журналы, DoH-сравнение доменов профиля, `ya.ru`), `detect`, `summarize`, `report::save`, перепроверка профилей для `fixes` (спец. 8: пробы остальных профилей при пути-кандидате и при исходном → `warn_breaks`), `Snapshot::clear`; событие `DiagFinished { id: Option<String> }`, уведомление Windows, если окно скрыто.
  - `AppEnv: diag::Env` — `set_path`: Current — ничего; Direct — `tester::engine::stop_engines` + `router.pause()`; Strategy — `tester::engine::start(launch(...))`; Trick — `launch_text`; ProxyPoint — `set_temporary(Some(rule))` + запуск роутера; ProxyNoZapret — то же + `stop_engines`; ExtendRule — `set_temporary` копии правила с новым протоколом. `probe`: `Https` → `probe::curl::https_via` (адрес представителя, 3 раза), `DiscordIpDiscovery`/`Stun` → `probe::udp` к представителям, `Ping` → `probe::ping`; `ask` — `Progress::Question` и ожидание `diag_answer` (канал), уведомление Windows, если окно скрыто.
  - `jobs::recover`: после `snapshot.restore` — если `snapshot.router` есть: `routing::set_enabled(app, enabled)`, `paused` → `router.pause()`; `texts::operation("diag")` → «диагностика».
  - Команды (`#[tauri::command] #[specta::specta]`, тело через `blocking`): `diag_profiles() -> Vec<Profile>`, `start_diag(request: DiagRequest)`, `diag_live() -> Option<Live>`, `diag_answer(answer: diag::Answer)`, `cancel_diag()`, `diag_reports() -> Vec<(String,String)>`, `diag_report(id: String) -> Summary`, `diag_apply(id: String, fix: u32) -> QuickCheck-или-()`; события `DiagProgress(diag::Progress)`, `DiagFinished { id: Option<String> }` в `collect_events!`. `diag_apply`: `AddRule` → `router.edit(add)` + `routing::set_enabled(true)`; `EditRule` → `edit`; `ApplyStrategy` → `actions::apply`; `ApplyHubCircular` → `circular_store::add` + `save` + `generate(текущая)` (текущая = активная стратегия zapret2; если активна Flowseal `<имя>` — её копия подпроекта 8 `zapret2:<имя>`, для `general` — `zapret2:general (Flowseal)`; своя Flowseal-стратегия без копии или ничего не активно — пустая основа) → `zapret2::service::save(.., "hub-circular", text, create)` → `actions::apply("zapret2:hub-circular")`; `TurnOff`/`Manual` → ошибка «это делается вручную» (кнопки нет).
  - `cargo run -p zapret-hub --bin export-bindings` → `frontend/src/lib/ipc/bindings.ts`.

- [ ] **Step 1: Тесты** (`--lib`, без прав): `diag_blocks_changes` (`begin_diag` → `change_any` ошибка `DIAGNOSING`), `tests_block_diag` (`begin_testing` → `start_diag` проверка флага отказывает), `recover_restores_router_section` (функция `router_after_restore(snap: &Snapshot) -> Option<(bool, bool)>` — чистая, тестируемая), `settings_survive_a_save` (+ новые поля), `operation_diag_named`.
- [ ] **Step 2–4:** FAIL → реализация → PASS; `cargo clippy --workspace --all-targets -- -D warnings`; bindings пересобраны.
- [ ] **Step 5:** `git commit -m "feat(app): run diagnostics with rollback, answers and apply"`

### Task 13: CLI: протокол 5 и группа `diag`

**Files:**
- Modify: `crates/cli/src/lib.rs` (PROTOCOL = 5; варианты `Request`: `DiagProfiles`, `StartDiag { request: DiagRequest }`, `DiagStatus`, `DiagAnswer { answer }`, `CancelDiag`, `DiagReports`, `DiagReport { id }`, `DiagApply { id, fix }` + `samples()`), `crates/cli/src/main.rs`, `src-tauri/src/cli.rs` (`dispatch`)
- Test: `crates/cli/src/main.rs` `mod tests`, `crates/cli/src/lib.rs` roundtrip

Команды (спец. 11): `diag profiles`; `diag run (--profile <id> | --app <exe>) [--mode auto|ask] [--observe <с>] [--budget <мин>] [--wait]` (`--wait` печатает итог; ход — строки в stderr, опрос `DiagStatus` каждые 2 с, как `wait_for_run`); `diag status`; `diag answer yes|no|skip`; `diag cancel`; `diag show [<id>]`; `diag apply [<id>] [--fix <n>] --yes` (`needs_yes`). `--observe` вне 10–120 → USAGE. Профиль только с пробой `live` в режиме `auto` → итог содержит «не проверяется автоматически: нужен режим „со мной“» (это делает `diag::summarize`, CLI лишь печатает).

- [ ] **Step 1: Тесты:** `protocol_is_5`; `diag_run_requires_profile_or_app` (USAGE); `diag_observe_out_of_range` (5 → USAGE); `diag_apply_needs_yes`; `diag_run_wait_prints_progress_and_summary` (фейковый `Call`: два `DiagStatus` с прогрессом, затем завершено → stderr 2 строки, stdout JSON итога); roundtrip новых `Request`.
- [ ] **Step 2–4:** FAIL → реализация → PASS (`cargo nextest run -p cli`, `cargo test -p zapret-hub --lib`).
- [ ] **Step 5:** `git commit -m "feat(cli): diag commands on protocol 5"`

### Task 14: Вкладка «Диагностика», быстрая проверка, трей

**Files:**
- Create: `frontend/src/tabs/Diag.svelte`
- Modify: `frontend/src/App.svelte` (TABS: после `routes`), `frontend/src/lib/app.svelte.ts` (`Tab` + `diagLive`, слушатели `diagProgress`/`diagFinished`), `frontend/src/lib/strings.ts` (`S.tabs.diag = "Диагностика"`, `S.diag`, `S.failed.diag*`), `frontend/src/lib/format.ts` (`pathLabel`, `faultText`, `patternTitle` — чистые), `frontend/src/tabs/Zapret.svelte` (раздел «Быстрая проверка» → строка состояния со ссылкой на вкладку), `src-tauri/src/tray.rs` (пункт «Диагностика…» открывает вкладку; во время диагностики подсказка значка — «идёт диагностика»), `src-tauri/src/texts.rs`
- Test: `frontend/src/lib/format.test.ts`

Шаги на одной странице (спец. 10): выбор (профиль из `diagProfiles` или «Любая программа» с полем exe; режим «сам»/«со мной»; «Начать») → наблюдение (таймер, живой список адресов из `Progress::Observing`) → виновники (галочки; по умолчанию все) → перебор (прогресс, таблица путей: путь, работает, задержка, потери, ответ) → итог (объяснения, картины, «Применить» на каждом `Fix` кроме `Manual`/`TurnOff`, предупреждения рядом, «Подробнее» — JSON отчёта, «Проверить снова» для `Manual`). Карточка-вопрос «Проверьте: <подсказка>. Работает?» — «Да / Нет / Не знаю» поверх прогресса. «Отмена» на любом шаге. Сверху — «Быстрая проверка» (перенесена с вкладки Zapret). История: список последних отчётов.

- [ ] **Step 1: Тесты** vitest: `pathLabel` для каждого `PathKind`; `faultText` («UDP уходит, ответов нет» для `UdpNoReply`, «SYN без ответа (3)» для `SynUnanswered{syns:3}`); `patternTitle` для Р1–Р6.
- [ ] **Step 2–4:** FAIL → реализация → PASS; `npm --prefix frontend run check` — 0 ошибок; `cargo test -p zapret-hub --lib`.
- [ ] **Step 5:** `git commit -m "feat(ui): diagnostics tab with live steps, questions and apply"`

### Task 15: Документация и выпуск 0.8.0

**Files:**
- Modify: `README.md` (раздел «Диагностика»: что делает, режимы, что хаб читает и что никогда не меняет — SOCKS5-прокси, журналы; где отчёты), `docs/cli.md` (группа `diag`, пример `version` → `"protocol": 5`), `CHANGELOG.md` (`## 0.8.0`: диагностика, `hub-circular`, UDP в circular, протокол CLI 5 — «обновите CLI и приложение вместе»), `docs/superpowers/specs/2026-10-11-zapret-hub-zapret2-design.md` (поправка: UDP в circular, выбранные `-n` диапазоны), `Cargo.toml`, `package.json`, `frontend/package.json` (0.8.0)

- [ ] **Step 1:** Правки; `cargo nextest run --workspace --exclude zapret-hub`, `cargo test -p zapret-hub --lib`, `npm --prefix frontend test`, `npm --prefix frontend run check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all --check` — всё зелёное.
- [ ] **Step 2:** Ручной чек-лист (спец. 13) — записать в `docs/cli.md`, выполнить с пользователем после сборки: голос Discord с плохой стратегией → диагностика находит голосовой сервер и путь; DbD в поиске матча; «со мной»; отмена посреди перебора → всё как было; падение хаба посреди перебора → откат при запуске; предупреждение по другому профилю; `hub-circular` применяется и голос работает.
- [ ] **Step 3:** `git commit -m "chore: release 0.8.0"` (тег `v0.8.0` — только по слову пользователя).
