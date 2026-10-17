# Zapret Hub — маршрутизация через SOCKS5, этап 2 (TCP и домены): план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** выбранные TCP-соединения и UDP-потоки идут через локальный SOCKS5 не только по подсетям, но и по доменам (`domains` в `routes.toml`, IP узнаются из ответов DNS); TCP — отражением адресов в локальный приёмник и `CONNECT` к прокси, профиль «Discord целиком».

**Architecture:** в `crates/router` добавляются `dns.rs` (разбор ответов, таблица «IP → домен» `Names`) и `tcp.rs` (чистая таблица NAT `Nat` для отражения, `pipe`, `reset`); `rules` получает `domains`, протоколы `tcp`/`both`, `Table::{matches, wants, with_names}`; `divert` — `capture_filter` (протоколы, IP из DNS, ответы приёмника, удерживаемые адреса, слияние сверх 64 диапазонов); `socks` — `connect`. `Live` в `lib.rs` открывает третий дескриптор (DNS, SNIFF), приёмник `0.0.0.0:0` и ведёт TCP через `Nat`; `Names` живёт в `Router` и переживает паузы. Приложение и CLI меняются на несколько строк; вкладка — протокол, домены, подсказка DoH, колонка «Домен».

**Tech Stack:** новых внешних зависимостей нет. Rust 1.99.0 (edition 2024); `windows` 0.62 (+ фича `Win32_Networking_WinSock` для `setsockopt(SO_LINGER)`); `toml` 1.1.8; specta 2.0.0-rc.25 (u64 в TS запрещён); clap 4.6; tauri 2.12; Svelte 5.57.1, svelte-check 4.7.6, vitest 5.0.3. WinDivert 2.2 из DLL движка.

**Spec:** `docs/superpowers/specs/2026-10-17-zapret-hub-router-design.md` (далее «спецификация»); этап 2 = раздел 2, пункт 2: 4.1 (дескриптор 2, фильтр), 4.2 (домен в заголовке UDP), 4.3, 4.5, 5.1, 5.2, 8, 9. Стиль и решения этапа 1 — `docs/superpowers/plans/2026-10-17-zapret-hub-router-stage1.md`. Исполнитель читает спецификацию вместе с планом.

**База:** `main` на `1104113`; ветка `feat/router-stage2` от неё.

## Global Constraints

- Только Windows 11 x64, Rust stable msvc 1.99.0. Крейты не зависят от `tauri` (только derive `specta`). Логика — в `crates/router`; команды `src-tauri` — несколько строк.
- Решения этапа 1 не меняются: прокси только на loopback (`parse_proxy`); `Down` после двух неудачных проверок, возврат прокси переоткрывает потоки «напрямую из-за прокси»; не-unicast (`packet::unicast`) идёт без изменений; фильтр `outbound and !loopback and ip and (…)`, `MAX_FILTER_RANGES = 64`, совпадение сверяет хаб; `status` и `flows` — из снимка; счётчик пауз (`routing::PAUSES`); смена дескриптора перехвата — новый открыт до `shutdown(RECV)` старого, нет правил — фильтр `false` (дескриптор только инжектит); `apps` — ошибка «`` `apps` is not supported yet``» со строкой.
- Значения этапа 2: срок записи DNS — TTL, но не меньше `MIN_TTL = 300 с`; запись NAT живёт до FIN в обе стороны или RST (+ `LINGER = 10 с` на последний ACK) или `IDLE = 120 с` без пакетов; отказ `CONNECT` (`OPEN_TIMEOUT = 3 с`, отказ, обрыв) → RST клиенту и адрес «напрямую» на `DIRECT_FOR = 60 с`; SYN, не дошедший до приёмника за `ACCEPT_WAIT = 3 с`, → тоже «напрямую» на 60 с; пересборка фильтра не чаще раза в секунду (`REOPEN_GAP`).
- Дескрипторы: перехват — NETWORK, приоритет 1000, флаги 0; DNS — NETWORK, 1000, `FLAG_SNIFF | FLAG_RECV_ONLY`, фильтр `DNS_FILTER = "ip and udp and udp.SrcPort == 53"`; сокеты — `SOCKET_FILTER = "!loopback and (udp or tcp) and (event == BIND or event == CONNECT or event == CLOSE)"`. Приёмник — `TcpListener` на `0.0.0.0:0`, один на сессию.
- `routes.toml` этапа 2: `name`, `enabled`, `protocol` (`"tcp" | "udp" | "both"`), `ports`, `ips`, `domains`. Домен — LDH: метки 1–63 символа `[A-Za-z0-9-]`, не начинаются и не кончаются `-`, всего ≤ 253, не меньше двух меток, последняя метка не из одних цифр, одна точка в конце допускается; сравнение без учёта регистра; домен покрывает себя и поддомены. Профили `discord-voice`, `discord`.
- Тесты не меняют систему: чистые модульные (переписывание TCP, разбор TCP и смещения контрольных сумм, разбор DNS со сжатием, петлями и обрезкой, таблица DNS, совпадение доменов, фильтр); SOCKS5 и `pipe`/`reset` — против сокетов на loopback внутри теста; всё, что открывает WinDivert, — `#[ignore]`.
- Тексты: код, ошибки Rust, коммиты — английский; интерфейс, `strings.ts`, `docs/`, README — русский. Причины «напрямую» идут в окно перечислением, текст — в `strings.ts`.
- Коммит — одна строка Conventional Commits (scope `router`, `app`, `cli`, `ui`, `docs`), без тела и трейлеров; `git add` с явными путями. Перед коммитом с Rust — `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` чисто; с фронтендом — `npm --prefix frontend run check` 0/0 и `npm --prefix frontend test`. Тесты приложения — `cargo test -p zapret-hub --lib`. После смены DTO/команд — `cargo run -p zapret-hub --bin export-bindings`, `bindings.ts` в том же коммите.

## Review Focus

1. **Правило только с доменами до первого ответа DNS** (профиль «Discord целиком» сразу после добавления): фильтр не должен ловить весь TCP/UDP на портах правила — у правила без диапазонов нет части фильтра. Тест: `a_domain_rule_without_answers_captures_nothing` (Task 5).
2. **Брандмауэр Windows не пропускает отражённый SYN к приёмнику** на `0.0.0.0`: соединения не должны висеть в чёрной дыре — через 3 с адрес уходит «напрямую», повтор SYN проходит. Тест: `a_syn_the_acceptor_never_saw_goes_direct_after_3_s` (Task 8).
3. **Соединения, открытые до включения или до правила** (Discord уже запущен): их пакеты (не SYN, нет записи NAT) идут без изменений, а не ломаются переписыванием; ответ приёмника без записи — выбрасывается. Тест: `a_connection_older_than_the_rule_passes_unchanged` (Task 8).
4. **Правило выключено или запись DNS истекла посреди соединения** (спецификация 7: открытые TCP остаются как были): адрес живой записи NAT остаётся в фильтре. Тест: `a_live_connection_keeps_its_destination_in_the_filter` (Task 5).
5. **Чужой или битый ответ DNS** (петля указателей сжатия, указатель вперёд, обрезанный пакет, петля CNAME): разбор не паникует и не зависает. Тесты: `hostile_and_broken_messages_are_dropped`, `a_cname_loop_ends` (Task 4).

---

## Структура файлов

```
crates/router/
  Cargo.toml                 windows += "Win32_Networking_WinSock"
  src/rules.rs               Rule.domains, Preset::Discord, check доменов, covers, Imported/import,
                             Compiled.{protocol, domains}, Table::{matches, wants, with_names}
  src/packet.rs              Tcp, parse_tcp, rewrite_tcp, FIN/SYN/RST/ACK, socks_header_to, parse_socks_udp_from
  src/dns.rs                 новый: Answer, parse, MIN_TTL, Names
  src/divert.rs              capture_filter, слияние диапазонов, SOCKET_FILTER (tcp), DNS_FILTER, PROTO_TCP
  src/socks.rs               connect; текст Refused(n) без «UDP ASSOCIATE»
  src/flows.rs               FlowKey.protocol, FlowRow.domain, Stats::set_domain, idle только UDP,
                             DirectReason::{Marked, NotAccepted}
  src/tcp.rs                 новый: Nat, Verdict, IDLE/LINGER/DIRECT_FOR/ACCEPT_WAIT, pipe, reset
  src/lib.rs                 два исправления этапа 1; Live: DNS, приёмник, TCP, домены в UDP
  tests/rules.rs, packet.rs, filter.rs, socks.rs, flows.rs, router.rs, live.rs (#[ignore]); новые dns.rs, tcp.rs
src-tauri/src/commands.rs    import_route_lines(ips, domains, text) -> Imported
crates/cli/src/main.rs       PresetArg::Discord
frontend/src/lib/{strings.ts, format.ts, format.test.ts}, tabs/{RouteEditor.svelte, Routes.svelte}
README.md, docs/cli.md, CHANGELOG.md, спецификация (поправки этапа 2), версии 0.5.0
```

---

### Task 1: долги этапа 1 — снимок в `flows()` и гонка проверки прокси

**Files:**
- Modify: `crates/router/src/lib.rs` (`Router::flows`, `Live::set_proxy`, `Live::check_proxy`)

**Interfaces:**
- Produces: `Live::update_proxy(&self, f: impl FnOnce(&ProxyState) -> Option<ProxyState>)` — решение и запись под одной блокировкой `proxy_state`, затем побочные действия нынешнего `set_proxy` (закрытие потоков, `notify`) без блокировки; `set_proxy(new)` = `update_proxy(|_| Some(new))`.

Новых тестов нет: оба исправления — область блокировок; детерминированный тест гонки требует WinDivert.

- [ ] **Шаг 1:** `Router::flows` — `let live = self.0.snapshot.lock().unwrap().1.clone();` до `match`/`map`: временный guard в выражении `match` держал `snapshot` на время блокировки `stats`.
- [ ] **Шаг 2:** `cargo test -p router` — PASS; коммит `fix(router): drop the snapshot guard before locking the flow stats`.
- [ ] **Шаг 3:** `update_proxy` по Interfaces; `check_proxy` зовёт `self.update_proxy(|current| flows::checked(current, new, &mut failures))` вместо чтения `current` и отдельного `set_proxy` (между ними `open_flow` мог поставить `NoUdp`, а проверка перезаписывала его на `Ok`).
- [ ] **Шаг 4:** `cargo test -p router` — PASS; коммит `fix(router): keep NoUdp when a proxy check races an associate`.

---

### Task 2: правила — `domains`, `tcp`/`both`, профиль «Discord целиком», импорт доменов

**Files:**
- Modify: `crates/router/src/rules.rs`
- Test: `crates/router/tests/rules.rs`; литералы `Rule` в `crates/router/tests/filter.rs`, `crates/router/tests/live.rs` (+ `domains: vec![]`)

**Interfaces:**
- Produces (`router::rules`):
  - `Rule` += `#[serde(default)] pub domains: Vec<String>` (после `ips`; в файл пишется всегда, тип в TS обязательный);
  - `Preset` += `Discord` (`"discord"`); `preset(DiscordVoice)` += `domains: ["discord.media"]`; `preset(Discord)` = `Rule { name: "Discord целиком", enabled: true, protocol: Both, ports: [], ips: [], domains: ["discord.com", "discord.gg", "discordapp.com", "discordapp.net", "discord.media"] }`;
  - `check`: любой `protocol`; каждый домен по LDH (Global Constraints) → поле `domains`, сообщения: `"{d}" is not a domain: letters, digits and - in dot-separated labels, no scheme, port or path`; `*.` в начале → `"{d}": write the domain itself, its subdomains are included`; не ASCII → `"{d}": write an international domain in its xn-- form`; последняя метка из цифр → `"{d}" is an IP address: put it in ips`; пустые `ips` и `domains` → поле `ips`, `a rule needs at least one target: ips or domains`;
  - `Raw.domains: Option<Spanned<Vec<String>>>`, строка ошибки домена — `span` поля `domains`; `apps` по-прежнему отклоняется по имени;
  - `pub fn covers(domain: &str, name: &str) -> bool` — оба уже в нижнем регистре без точки в конце: равны или `name` кончается на `.{domain}`;
  - `#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)] pub struct Imported { pub ips: Vec<String>, pub domains: Vec<String> }`;
  - `pub fn import(ips: &[String], domains: &[String], text: &str) -> Result<Imported, RulesError>` — заменяет этап-1 `import(ips, text)`: строка-подсеть → `ips`; строка только из цифр, `.` и `/`, но не подсеть → ошибка поля `ips`; иначе домен (в нижнем регистре, без точки в конце) → `domains` или ошибка поля `domains` с номером строки; повторы (домены — без учёта регистра) отбрасываются;
  - `Compiled` += `pub protocol: Protocol`, `pub domains: Vec<String>` (нижний регистр, без точки в конце);
  - `Table::matches(&self, protocol: Protocol, dst: SocketAddrV4) -> Option<&str>` — `protocol` потока `Tcp | Udp`; правило `Both` совпадает с обоими; иначе как `match_udp` этапа 1; `match_udp(dst)` = `matches(Protocol::Udp, dst)`;
  - `Table::wants(&self, name: &str) -> bool` — домен какого-то правила таблицы покрывает `name` (нижний регистр);
  - `Table::with_names(self, names: &[(Ipv4Addr, String)]) -> Table` — каждый IP диапазоном `(ip, ip)` уходит в `ranges` каждого правила, чей домен покрывает имя; `ranges` снова сортируются и сливаются тем же кодом, что в `Table::new` (вынести в `fn merge(Vec<(u32, u32)>) -> Vec<(u32, u32)>`).

- [ ] **Шаг 1: Падающие тесты** — в `tests/rules.rs`: литерал в `the_discord_voice_preset_is_the_spec` += `domains: vec!["discord.media".into()]`; в `ports_and_subnets_are_checked` замыкание `with` ставит `domains: vec![]`; `fields_of_later_stages_are_named` и `import_takes_ip_lines_once_and_refuses_the_rest` заменяются; новые:

```rust
#[test]
fn the_whole_discord_preset_is_the_spec() {
    let whole = rules::preset(Preset::Discord);
    assert_eq!((whole.name.as_str(), whole.protocol), ("Discord целиком", Protocol::Both));
    assert_eq!(whole.domains, ["discord.com", "discord.gg", "discordapp.com", "discordapp.net", "discord.media"]);
    assert!(whole.ips.is_empty() && whole.ports.is_empty());
    assert!(rules::check(&whole).is_empty());
}

#[test]
fn domains_are_ldh_names_without_scheme_or_path() {
    let with = |d: &[&str]| Rule { domains: d.iter().map(|s| s.to_string()).collect(), ips: vec![], ..voice() };
    let fields = |r: Rule| rules::check(&r).into_iter().map(|e| e.field).collect::<Vec<_>>();
    assert!(fields(with(&["discord.com", "Media.Discord.GG", "xn--d1acufc.xn--p1ai", "a-b.example.", "1e100.net"])).is_empty());
    let long = format!("{}.com", "a".repeat(64));
    for bad in ["https://discord.com", "discord.com/app", "discord.com:443", "*.discord.com", "-a.com", "a-.com",
                "a..com", "com", "дискорд.рф", "1.2.3.4", &long] {
        assert_eq!(fields(with(&[bad])), ["domains"], "{bad:?}");
    }
    assert_eq!(fields(with(&[])), ["ips"], "a rule needs ips or domains");
}

#[test]
fn tcp_and_both_parse_and_apps_is_still_named() {
    for p in ["tcp", "both"] {
        let text = format!("[[rule]]\nname = \"a\"\nprotocol = \"{p}\"\ndomains = [\"discord.com\"]\n");
        assert_eq!(rules::parse(&text).unwrap()[0].domains, ["discord.com"]);
    }
    let apps = "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\napps = [\"Discord.exe\"]\n";
    let err = rules::parse(apps).unwrap_err();
    assert_eq!(err.line, Some(5));
    assert!(err.to_string().contains("`apps` is not supported yet"), "{err}");
    let err = rules::parse("[[rule]]\nname = \"a\"\nprotocol = \"tcp\"\ndomains = [\"x/y\"]\n").unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(4), Some("domains")));
}

#[test]
fn import_sends_ip_lines_to_ips_and_names_to_domains() {
    let text = "\u{feff}# list\r\n104.29.0.0/16\r\ndiscord.media\r\nDiscord.Media\n\n35.217.0.0/16\n";
    let got = rules::import(&["104.29.0.0/16".into()], &[], text).unwrap();
    assert_eq!(got.ips, ["104.29.0.0/16", "35.217.0.0/16"]);
    assert_eq!(got.domains, ["discord.media"]);
    let err = rules::import(&[], &[], "1.1.1.1\nhttps://discord.com/\n").unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(2), Some("domains")));
    assert_eq!(rules::import(&[], &[], "300.1.1.1\n").unwrap_err().field.as_deref(), Some("ips"));
}

#[test]
fn domains_cover_themselves_and_subdomains() {
    assert!(rules::covers("discord.media", "discord.media"));
    assert!(rules::covers("discord.media", "c-fra08.discord.media"));
    assert!(!rules::covers("discord.media", "notdiscord.media"));
    assert!(!rules::covers("discord.media", "media"));
}

#[test]
fn resolved_names_add_ips_to_their_rules_by_protocol() {
    let ip = |s: &str| s.parse::<std::net::Ipv4Addr>().unwrap();
    let table = Table::new(&[voice(), rules::preset(Preset::Discord)]).with_names(&[
        (ip("162.159.135.232"), "gateway.discord.gg".into()),
        (ip("35.214.1.1"), "c-fra08.discord.media".into()),
        (ip("8.8.8.8"), "dns.google".into()),
    ]);
    let at = |p, a: &str| table.matches(p, a.parse().unwrap());
    assert_eq!(at(Protocol::Tcp, "162.159.135.232:443"), Some("Discord целиком"));
    assert_eq!(at(Protocol::Udp, "35.214.1.1:50001"), Some("Discord голос"), "file order");
    assert_eq!(at(Protocol::Tcp, "35.214.1.1:443"), Some("Discord целиком"));
    assert_eq!(at(Protocol::Tcp, "104.29.1.1:50001"), None, "the voice rule is UDP only");
    assert_eq!(table.match_udp("8.8.8.8:53".parse().unwrap()), None);
    assert!(table.wants("cdn.discordapp.com") && !table.wants("example.com"));
    let off = Rule { enabled: false, ..rules::preset(Preset::Discord) };
    assert!(!Table::new(&[off]).wants("discord.com"));
}
```

- [ ] **Шаг 2:** `cargo test -p router --test rules` — ошибки компиляции (`domains`, `Preset::Discord`, `covers`).
- [ ] **Шаг 3:** реализовать по Interfaces; литералы `Rule` в `tests/filter.rs` и `tests/live.rs` — `domains: vec![]`.
- [ ] **Шаг 4:** `cargo test -p router` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): domains in rules, tcp and both, the whole Discord preset`.

---

### Task 3: пакеты TCP, переписывание адресов, домен в заголовке SOCKS5 UDP

**Files:**
- Modify: `crates/router/src/packet.rs`
- Test: `crates/router/tests/packet.rs`

**Interfaces:**
- Produces (`router::packet`):
  - `pub const FIN: u8 = 0x01, SYN: u8 = 0x02, RST: u8 = 0x04, ACK: u8 = 0x10`;
  - `pub struct Tcp<'a> { pub src: SocketAddrV4, pub dst: SocketAddrV4, pub flags: u8, pub payload: &'a [u8] }`;
  - `pub fn parse_tcp(packet: &[u8]) -> Option<Tcp<'_>>` — IPv4, протокол 6, без фрагментации (как `parse_udp`), `ihl ≥ 20`, `total ≤ len`, data offset ≥ 5 и `ihl + doff·4 ≤ total`; `flags` — байт `ihl + 13`; `payload` — после опций до `total`;
  - `pub fn rewrite_tcp(packet: &mut [u8], src: SocketAddrV4, dst: SocketAddrV4)` — только для пакета, прошедшего `parse_tcp`: адреса (12..20), порты (`ihl..ihl+4`), обнуляет контрольные суммы IP (10..12) и TCP (`ihl+16..ihl+18`); их считает `Dll::checksums`;
  - `pub fn socks_header_to(dst: SocketAddrV4, domain: Option<&str>) -> Vec<u8>` — `None` = `socks_header(dst)`; домен → `00 00 00 03 len name port`;
  - `pub fn parse_socks_udp_from(datagram: &[u8], flow_dst: SocketAddrV4) -> Option<(SocketAddrV4, &[u8])>` — `ATYP 1` как `parse_socks_udp`; `ATYP 3` (ответ прокси на запрос с доменом) → `(flow_dst, data)`: клиентский сокет ждёт ответа с того адреса, куда слал. `parse_socks_udp` этапа 1 не меняется.

- [ ] **Шаг 1: Падающие тесты** — `tests/packet.rs`:

```rust
use router::packet::{ACK, SYN, parse_socks_udp_from, parse_tcp, rewrite_tcp, socks_header, socks_header_to};

/// 192.168.1.5:50000 → 162.159.135.232:443, SYN, data offset 6 (an MSS option), checksums 0x1234 / 0xABCD.
fn syn() -> Vec<u8> {
    let mut p = vec![0x45, 0, 0, 44, 0, 1, 0x40, 0, 128, 6, 0xAB, 0xCD, 192, 168, 1, 5, 162, 159, 135, 232];
    p.extend_from_slice(&[0xC3, 0x50, 0x01, 0xBB, 0, 0, 0, 1, 0, 0, 0, 0, 0x60, 0x02, 0xFA, 0xF0, 0x12, 0x34, 0, 0]);
    p.extend_from_slice(&[2, 4, 5, 0xB4]);
    p
}

#[test]
fn a_tcp_segment_parses_with_its_flags_and_options() {
    let p = syn();
    let t = parse_tcp(&p).unwrap();
    assert_eq!((t.src.to_string(), t.dst.to_string()), ("192.168.1.5:50000".into(), "162.159.135.232:443".into()));
    assert_eq!(t.flags & (SYN | ACK), SYN);
    assert!(t.payload.is_empty(), "options are not payload");
    let mut data = p.clone();
    data.extend_from_slice(b"GET");
    data[3] = 47;
    assert_eq!(parse_tcp(&data).unwrap().payload, b"GET");
}

#[test]
fn bad_tcp_does_not_parse() {
    let p = syn();
    let (mut udp, mut fragment, mut short, mut long) = (p.clone(), p.clone(), p.clone(), p.clone());
    udp[9] = 17;
    fragment[7] = 1;
    short[32] = 0x40; // data offset 4
    long[32] = 0xF0; // 60 bytes, past the packet
    for bad in [udp, fragment, short, long, p[..39].to_vec()] {
        assert!(parse_tcp(&bad).is_none());
    }
}

#[test]
fn a_reflected_segment_has_new_addresses_and_zero_checksums() {
    let mut p = syn();
    rewrite_tcp(&mut p, "162.159.135.232:50000".parse().unwrap(), "192.168.1.5:61000".parse().unwrap());
    assert_eq!(p[12..20], [162, 159, 135, 232, 192, 168, 1, 5]);
    assert_eq!(p[20..24], [0xC3, 0x50, 0xEE, 0x48]);
    assert_eq!((&p[10..12], &p[36..38]), (&[0u8, 0][..], &[0u8, 0][..]), "checksums are left to WinDivert");
    assert_eq!((&p[24..36], &p[38..]), (&syn()[24..36], &syn()[38..]), "sequence, flags, window and options stay");
}

#[test]
fn a_known_domain_goes_into_the_socks_udp_header() {
    let dst = "35.214.1.1:50005".parse().unwrap();
    assert_eq!(socks_header_to(dst, None), socks_header(dst).to_vec());
    let named = [&[0u8, 0, 0, 3, 15][..], b"a.discord.media", &[0xC3, 0x55]].concat();
    assert_eq!(socks_header_to(dst, Some("a.discord.media")), named);
    let reply = [named.as_slice(), b"ok"].concat();
    assert_eq!(parse_socks_udp_from(&reply, dst), Some((dst, &b"ok"[..])));
    assert_eq!(parse_socks_udp_from(&reply[..12], dst), None, "a cut name");
    let ipv4 = [0, 0, 0, 1, 35, 214, 1, 1, 0xC3, 0x55, b'x'];
    assert_eq!(parse_socks_udp_from(&ipv4, "1.1.1.1:1".parse().unwrap()), Some((dst, &b"x"[..])));
}
```

- [ ] **Шаг 2:** `cargo test -p router --test packet` — ошибки компиляции.
- [ ] **Шаг 3:** реализовать по Interfaces.
- [ ] **Шаг 4:** `cargo test -p router --test packet` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): TCP segments, address rewriting and domains over SOCKS5 UDP`.

---

### Task 4: DNS — разбор ответов и таблица «IP → домен»

**Files:**
- Create: `crates/router/src/dns.rs`; Modify: `crates/router/src/lib.rs` (`pub mod dns;`)
- Test: `crates/router/tests/dns.rs`

**Interfaces:**
- Produces (`router::dns`):
  - `#[derive(Debug, Clone, PartialEq)] pub struct Answer { pub name: String, pub ips: Vec<(Ipv4Addr, u32)> }` — `name` — имя вопроса (нижний регистр, без точки в конце), `ips` — A-записи (класс 1, длина 4) с TTL в порядке сообщения, чьё имя — имя вопроса или любое имя его цепочки CNAME;
  - `pub fn parse(message: &[u8]) -> Option<Answer>` — `None`, если: короче заголовка, QR = 0, RCODE ≠ 0, QDCOUNT ≠ 1, имя или запись выходят за конец, указатель сжатия не строго назад (так петли невозможны), имя длиннее 255, метка с битами `01`/`10`. Цепочка: сначала все рёбра CNAME, затем замыкание от имени вопроса (множество имён — петля CNAME конечна), затем A-записи. Пакет с флагом TC разбирается как обычный;
  - `pub const MIN_TTL: Duration = Duration::from_secs(300)`;
  - `#[derive(Default)] pub struct Names` (`HashMap<Ipv4Addr, (String, Instant)>`):
    - `record(&mut self, answer: &Answer, now: Instant) -> bool` — срок `now + max(ttl, MIN_TTL)`, у известного IP — позднее из старого и нового; `true`, если IP новый или сменил имя (фильтр надо пересобрать);
    - `name_of(&self, ip: Ipv4Addr, now: Instant) -> Option<&str>` — только неистёкшие;
    - `expire(&mut self, now: Instant) -> bool` — удаляет записи со сроком `≤ now`; `true`, если что-то удалено;
    - `entries(&self) -> Vec<(Ipv4Addr, String)>` — по возрастанию IP.

- [ ] **Шаг 1: Падающие тесты** — `tests/dns.rs`:

```rust
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};
use router::dns::{self, Answer, MIN_TTL, Names};

fn name(n: &str) -> Vec<u8> {
    let mut out: Vec<u8> = n.split('.').flat_map(|l| [&[l.len() as u8][..], l.as_bytes()].concat()).collect();
    out.push(0);
    out
}
fn rr(owner: &[u8], kind: u16, ttl: u32, data: &[u8]) -> Vec<u8> {
    [owner, &kind.to_be_bytes(), &[0, 1], &ttl.to_be_bytes(), &(data.len() as u16).to_be_bytes(), data].concat()
}
fn message(q: &str, answers: &[Vec<u8>]) -> Vec<u8> {
    [&[0x12, 0x34, 0x81, 0x80, 0, 1, 0, answers.len() as u8, 0, 0, 0, 0][..], &name(q), &[0, 1, 0, 1], &answers.concat()].concat()
}
fn ip(s: &str) -> Ipv4Addr { s.parse().unwrap() }

#[test]
fn a_records_and_the_end_of_a_cname_chain_go_under_the_asked_name() {
    let edge = name("cdn.discordapp.com.cdn.cloudflare.net");
    let m = message("Cdn.DiscordApp.com", &[
        rr(&name("cdn.discordapp.com"), 5, 300, &edge),
        rr(&edge, 1, 60, &[162, 159, 130, 233]),
        rr(&edge, 1, 60, &[162, 159, 129, 233]),
        rr(&name("unrelated.example"), 1, 60, &[6, 6, 6, 6]),
    ]);
    let a = dns::parse(&m).unwrap();
    assert_eq!(a.name, "cdn.discordapp.com");
    assert_eq!(a.ips, [(ip("162.159.130.233"), 60), (ip("162.159.129.233"), 60)]);
}

#[test]
fn compressed_names_are_followed() {
    // question "a.discord.media" at 12..29 ("discord" label at 14), type/class to 33;
    // answer 1 at 33: owner -> 12, CNAME rdata at 45 = "x" + pointer -> 14 ("x.discord.media");
    // answer 2: owner -> 45, A 35.214.1.1, TTL 30.
    let mut m = message("a.discord.media", &[]);
    m[7] = 2;
    m.extend_from_slice(&[0xC0, 12, 0, 5, 0, 1, 0, 0, 1, 44, 0, 4, 1, b'x', 0xC0, 14]);
    m.extend_from_slice(&[0xC0, 45, 0, 1, 0, 1, 0, 0, 0, 30, 0, 4, 35, 214, 1, 1]);
    assert_eq!(dns::parse(&m).unwrap(), Answer { name: "a.discord.media".into(), ips: vec![(ip("35.214.1.1"), 30)] });
}

#[test]
fn a_cname_loop_ends() {
    let m = message("a.com", &[
        rr(&name("a.com"), 5, 60, &name("b.com")),
        rr(&name("b.com"), 5, 60, &name("a.com")),
        rr(&name("b.com"), 1, 60, &[1, 1, 1, 1]),
    ]);
    assert_eq!(dns::parse(&m).unwrap().ips, [(ip("1.1.1.1"), 60)]);
}

#[test]
fn hostile_and_broken_messages_are_dropped() {
    let ok = message("discord.com", &[rr(&name("discord.com"), 1, 60, &[1, 2, 3, 4])]);
    assert!(dns::parse(&ok).is_some());
    let at = 12 + name("discord.com").len() + 4; // the answer's owner
    let (mut query, mut nxdomain, mut looped, mut forward, mut long) = (ok.clone(), ok.clone(), ok.clone(), ok.clone(), ok.clone());
    query[2] &= !0x80;
    nxdomain[3] = 0x83;
    looped[at..at + 2].copy_from_slice(&[0xC0, at as u8]); // points at itself
    forward[12..14].copy_from_slice(&[0xC0, 40]);
    let n = long.len();
    long[n - 5] = 9; // rdlength past the end
    for bad in [query, nxdomain, looped, forward, long, ok[..ok.len() - 1].to_vec(), vec![], vec![0; 12]] {
        assert!(dns::parse(&bad).is_none(), "{bad:?}");
    }
}

#[test]
fn names_live_their_ttl_but_at_least_5_minutes() {
    let t0 = Instant::now();
    let mut names = Names::default();
    let short = Answer { name: "discord.com".into(), ips: vec![(ip("1.1.1.1"), 60)] };
    let long = Answer { name: "discord.gg".into(), ips: vec![(ip("2.2.2.2"), 3600)] };
    assert!(names.record(&short, t0) && names.record(&long, t0));
    assert!(!names.record(&short, t0 + Duration::from_secs(10)), "a known IP changes no filter");
    assert_eq!(names.name_of(ip("1.1.1.1"), t0 + Duration::from_secs(309)), Some("discord.com"));
    assert!(names.expire(t0 + Duration::from_secs(310)));
    assert_eq!(names.name_of(ip("1.1.1.1"), t0 + Duration::from_secs(310)), None);
    assert_eq!(names.entries(), [(ip("2.2.2.2"), "discord.gg".to_string())]);
    assert!(!names.expire(t0 + Duration::from_secs(3599)));
    assert!(names.expire(t0 + MIN_TTL * 12));
}
```

- [ ] **Шаг 2:** `cargo test -p router --test dns` — ошибки компиляции.
- [ ] **Шаг 3:** реализовать `dns.rs` по Interfaces.
- [ ] **Шаг 4:** `cargo test -p router --test dns` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): parse DNS answers into an IP-to-domain table`.

---

### Task 5: фильтр перехвата — TCP, IP из DNS, ответы приёмника, слияние сверх 64

**Files:**
- Modify: `crates/router/src/divert.rs`
- Test: `crates/router/tests/filter.rs`

**Interfaces:**
- Consumes: `Table::with_names`, `Compiled.protocol` (Task 2).
- Produces (`router::divert`):
  - `pub fn capture_filter(table: &Table, acceptor: Option<u16>, held: &[Ipv4Addr]) -> Option<String>`; `filter(table)` = `capture_filter(table, None, &[])` (тесты этапа 1 не меняются, кроме переписанного ниже);
  - `SOCKET_FILTER`, `DNS_FILTER` — значения из Global Constraints; `pub const PROTO_TCP: u8 = 6` (для `Address::socket_protocol`).

Формат (тела `port`/`range` этапа 1 параметризуются полем `udp`/`tcp`):

```text
part of a rule = None when its ranges are empty (a domain rule before any answer captures nothing)
  udp:  "(udp and (<udp ports>) and (<ips>))"            no ports: "(udp and (<ips>))"   — as stage 1
  tcp:  "(tcp and (<tcp ports>) and (<ips>))"            no ports: "(tcp and (<ips>))"
  both: "(((udp and (<udp ports>)) or (tcp and (<tcp ports>))) and (<ips>))"   no ports: "((udp or tcp) and (<ips>))"
held:     "(tcp and (<held ips merged into ranges>))"     when held is not empty
acceptor: "(tcp and tcp.SrcPort == <port>)"               last; only when some part is tcp/both, or held is not empty
over MAX_FILTER_RANGES ranges in all parts together: repeatedly merge, inside one part, the two neighbouring
  ranges with the smallest gap (ties: the first) until the total fits or no part has two ranges left.
  The filter becomes a superset; the hub still matches exactly (`Table` is not coarsened).
none of the parts → None
```

Это заменяет «сверх 64 — подсети уходят из фильтра» этапа 1 (Ruling 1 в самопроверке); `// ponytail:` у слияния: O(n²) по числу диапазонов, их сотни.

- [ ] **Шаг 1: Падающие тесты** — `tests/filter.rs`; `too_many_ranges_leave_the_ips_to_the_hub` заменяется на `too_many_ranges_merge_the_closest_ones`:

```rust
use std::net::Ipv4Addr;
use router::divert::capture_filter;
use router::rules::Protocol;

fn named(rule: Rule, names: &[(&str, &str)]) -> Table {
    let names: Vec<(Ipv4Addr, String)> = names.iter().map(|(ip, n)| (ip.parse().unwrap(), n.to_string())).collect();
    Table::new(&[rule]).with_names(&names)
}

#[test]
fn a_domain_rule_without_answers_captures_nothing() {
    assert_eq!(capture_filter(&Table::new(&[rules::preset(Preset::Discord)]), Some(61000), &[]), None);
    let voice = Table::new(&[rules::preset(Preset::DiscordVoice)]);
    assert_eq!(capture_filter(&voice, Some(61000), &[]), filter(&voice), "a UDP rule needs no acceptor part");
}

#[test]
fn resolved_ips_and_the_acceptor_replies_are_captured() {
    let table = named(rules::preset(Preset::Discord), &[("162.159.135.232", "discord.com"), ("162.159.136.232", "gateway.discord.gg")]);
    assert_eq!(
        capture_filter(&table, Some(61000), &[]).unwrap(),
        "outbound and !loopback and ip and (((udp or tcp) and (ip.DstAddr == 162.159.135.232 or ip.DstAddr == 162.159.136.232)) \
         or (tcp and tcp.SrcPort == 61000))"
    );
}

#[test]
fn tcp_and_both_rules_use_their_own_port_fields() {
    let tcp = Rule { name: "t".into(), protocol: Protocol::Tcp, ..rule(&["443"], &["1.1.1.1"]) };
    let both = Rule { name: "b".into(), protocol: Protocol::Both, ..rule(&["443", "50000-65535"], &["2.2.2.2"]) };
    assert_eq!(
        capture_filter(&Table::new(&[tcp, both]), Some(61000), &[]).unwrap(),
        "outbound and !loopback and ip and ((tcp and (tcp.DstPort == 443) and (ip.DstAddr == 1.1.1.1)) \
         or (((udp and (udp.DstPort == 443 or udp.DstPort >= 50000)) or (tcp and (tcp.DstPort == 443 or tcp.DstPort >= 50000))) \
         and (ip.DstAddr == 2.2.2.2)) or (tcp and tcp.SrcPort == 61000))"
    );
}

#[test]
fn a_live_connection_keeps_its_destination_in_the_filter() {
    let held = ["162.159.135.232".parse().unwrap()];
    assert_eq!(
        capture_filter(&Table::new(&[]), Some(61000), &held).unwrap(),
        "outbound and !loopback and ip and ((tcp and (ip.DstAddr == 162.159.135.232)) or (tcp and tcp.SrcPort == 61000))"
    );
}

#[test]
fn too_many_ranges_merge_the_closest_ones() {
    let ips: Vec<String> = (0..65).map(|i| format!("10.{i}.0.0/24")).collect();
    let ips: Vec<&str> = ips.iter().map(String::as_str).collect();
    let table = Table::new(&[rule(&["50000-65535"], &ips)]);
    let text = filter(&table).unwrap();
    assert_eq!(text.matches("ip.DstAddr >=").count(), 64);
    assert!(text.starts_with("outbound and !loopback and ip and ((udp and (udp.DstPort >= 50000) and ((ip.DstAddr >= 10.0.0.0 and ip.DstAddr <= 10.1.0.255) or "), "{text}");
    assert!(text.contains("(ip.DstAddr >= 10.64.0.0 and ip.DstAddr <= 10.64.0.255)"));
    assert_eq!(table.match_udp("10.0.200.1:50001".parse().unwrap()), None, "the hub matches exactly");
}
```

(`rule()` теста этапа 1 строится из `Preset::DiscordVoice`, его `domains` без имён диапазонов не дают.)

- [ ] **Шаг 2:** `cargo test -p router --test filter` — ошибки компиляции.
- [ ] **Шаг 3:** реализовать по Interfaces и формату.
- [ ] **Шаг 4:** `cargo test -p router --test filter` — PASS (тесты этапа 1 `the_voice_preset_makes_the_scripts_filter`, `single_values_and_missing_ports_are_short` — без изменений).
- [ ] **Шаг 5:** коммит `feat(router): capture TCP, resolved IPs and acceptor replies`.

---

### Task 6: SOCKS5 `CONNECT`

**Files:**
- Modify: `crates/router/src/socks.rs`
- Test: `crates/router/tests/socks.rs`

**Interfaces:**
- Produces: `pub fn connect(proxy: SocketAddrV4, dst: SocketAddrV4, domain: Option<&str>, timeout: Duration) -> Result<TcpStream, SocksError>` — приветствие как у `udp_associate`, запрос `05 01 00 03 len name port` (домен) или `05 01 00 01 ip port`; разбор ответа — общий с `udp_associate` (вынести `fn read_reply(stream: &mut TcpStream) -> Result<(), SocksError>`); после успеха тайм-ауты чтения и записи снимаются (`None`). `Display` для `Refused(n)`, `n ≠ 7`: `the proxy refused the request: code {n}`.

- [ ] **Шаг 1: Падающие тесты** — `tests/socks.rs` (хелпер `proxy` этапа 1 читает 10 байт запроса — ровно `CONNECT` по IP):

```rust
#[test]
fn connect_names_the_domain_and_then_carries_bytes() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = match listener.local_addr().unwrap() { std::net::SocketAddr::V4(a) => a, _ => unreachable!() };
    let server = std::thread::spawn(move || {
        let (mut s, _) = listener.accept().unwrap();
        let mut got = vec![0u8; 3 + 5 + 11 + 2];
        s.read_exact(&mut got[..3]).unwrap();
        s.write_all(&[5, 0]).unwrap();
        s.read_exact(&mut got[3..]).unwrap();
        s.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).unwrap();
        let mut ping = [0u8; 4];
        s.read_exact(&mut ping).unwrap();
        s.write_all(b"pong").unwrap();
        (got, ping)
    });
    let mut stream = socks::connect(addr, "162.159.135.232:443".parse().unwrap(), Some("discord.com"), Duration::from_secs(2)).unwrap();
    assert_eq!(stream.read_timeout().unwrap(), None, "a relayed stream has no timeout");
    stream.write_all(b"ping").unwrap();
    let mut pong = [0u8; 4];
    stream.read_exact(&mut pong).unwrap();
    let (got, ping) = server.join().unwrap();
    assert_eq!(got, [&[5u8, 1, 0, 5, 1, 0, 3, 11][..], b"discord.com", &[1, 187]].concat());
    assert_eq!((&ping, &pong), (b"ping", b"pong"));
}

#[test]
fn connect_by_ip_and_a_refusal_with_its_code() {
    let (addr, server) = proxy(&[5, 0], Some(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]));
    let err = socks::connect(addr, "1.2.3.4:80".parse().unwrap(), None, SHORT).unwrap_err();
    assert!(matches!(err, SocksError::Refused(5)));
    assert_eq!(err.to_string(), "the proxy refused the request: code 5");
    assert_eq!(server.join().unwrap(), [5, 1, 0, 5, 1, 0, 1, 1, 2, 3, 4, 0, 80]);
}
```

И в `a_silent_proxy_times_out` — второй слушатель и `assert!(matches!(socks::connect(addr2, DST.parse().unwrap(), None, SHORT), Err(SocksError::Timeout)))`.

- [ ] **Шаг 2:** `cargo test -p router --test socks` — ошибки компиляции.
- [ ] **Шаг 3:** реализовать по Interfaces.
- [ ] **Шаг 4:** `cargo test -p router --test socks` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): SOCKS5 CONNECT with the domain when it is known`.

---

### Task 7: потоки — протокол в ключе, домен в строке, новые причины

**Files:**
- Modify: `crates/router/src/flows.rs`, `crates/router/src/lib.rs` (литералы `FlowKey` — `protocol: Protocol::Udp`)
- Test: `crates/router/tests/flows.rs` (замыкание `key` в `closed_flows_stay_30_seconds_and_traffic_adds_up` += `protocol: Protocol::Udp`)

**Interfaces:**
- Produces (`router::flows`):
  - `FlowKey` += `pub protocol: Protocol` (первое поле; `rules::Protocol`, только `Tcp | Udp`);
  - `FlowRow` += `pub domain: Option<String>`; `protocol` — `"tcp"`/`"udp"` из ключа;
  - `Stats::set_domain(&mut self, key: FlowKey, domain: String)`;
  - `Stats::idle` возвращает только UDP: TCP закрывается концом соединения, не простоем;
  - `DirectReason` += `Marked` (адрес на 60 с «напрямую» после отказа прокси), `NotAccepted` (SYN не дошёл до приёмника); `proxy_failure()` для них `false`.

- [ ] **Шаг 1: Падающий тест**

```rust
#[test]
fn tcp_flows_never_go_idle_and_rows_name_protocol_and_domain() {
    let t0 = Instant::now();
    let tcp = FlowKey { protocol: Protocol::Tcp, local_port: 50000, dst: "162.159.135.232:443".parse().unwrap() };
    let udp = FlowKey { protocol: Protocol::Udp, ..tcp };
    let mut stats = Stats::default();
    stats.open(tcp, "Discord целиком", Via::Proxy, t0);
    stats.open(udp, "Discord целиком", Via::Proxy, t0);
    stats.set_domain(tcp, "discord.com".into());
    assert_eq!(stats.idle(t0 + Duration::from_secs(600)), [udp], "a quiet TCP connection is still open");
    let rows = stats.rows(t0, true);
    assert_eq!(rows.len(), 2, "same port and address, two protocols");
    let row = rows.iter().find(|r| r.protocol == "tcp").unwrap();
    assert_eq!(row.domain.as_deref(), Some("discord.com"));
}
```

- [ ] **Шаг 2:** `cargo test -p router --test flows` — ошибки компиляции.
- [ ] **Шаг 3:** реализовать по Interfaces.
- [ ] **Шаг 4:** `cargo test -p router` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): TCP flows and domains in flow rows`.

---

### Task 8: TCP — таблица NAT отражения, перекачка байтов, сброс

**Files:**
- Create: `crates/router/src/tcp.rs`; Modify: `crates/router/src/lib.rs` (`pub mod tcp;`), `crates/router/Cargo.toml` (`Win32_Networking_WinSock`)
- Test: `crates/router/tests/tcp.rs`

**Interfaces:**
- Consumes: `packet::{Tcp, FIN, SYN, RST, ACK}` (Task 3).
- Produces (`router::tcp`):
  - `IDLE = 120 s`, `LINGER = 10 s`, `DIRECT_FOR = 60 s`, `ACCEPT_WAIT = 3 s` (`pub const … : Duration`);
  - `#[derive(Debug, Clone, Copy, PartialEq)] pub enum Verdict { Pass, Marked, Syn, Rewrite { src: SocketAddrV4, dst: SocketAddrV4 }, Drop }`;
  - `#[derive(Default)] pub struct Nat` — записи по порту клиента: исходный адрес назначения, FIN клиента, FIN приёмника, время закрытия, время SYN, принят ли, последний пакет; пометки «напрямую» по адресу назначения:
    - `from_client(&mut self, seg: &Tcp, acceptor: u16, now: Instant) -> Verdict` — живая запись того же назначения → `Rewrite { src: (seg.dst.ip, seg.src.port), dst: (seg.src.ip, acceptor) }`, учёт FIN/RST и времени; голый SYN (без ACK) без записи или с закрытой записью → `Marked`, если адрес помечен, иначе `Syn`; прочее → `Pass`;
    - `admit(&mut self, seg: &Tcp, acceptor: u16, now: Instant) -> Verdict` — новая запись, возвращает тот же `Rewrite`;
    - `from_acceptor(&mut self, seg: &Tcp, now: Instant) -> Verdict` — запись по `seg.dst.port()` с `seg.dst.ip == orig.ip` → `Rewrite { src: orig, dst: (seg.src.ip, seg.dst.port) }`, учёт FIN/RST; нет записи → `Drop`;
    - `original(&self, client_port: u16, peer: Ipv4Addr) -> Option<SocketAddrV4>`; `accepted(&mut self, client_port: u16)`;
    - `unaccepted(&mut self, now: Instant) -> Vec<(u16, SocketAddrV4)>` — непринятые записи старше `ACCEPT_WAIT`: удаляет, помечает их адреса и возвращает;
    - `mark_direct(&mut self, dst: SocketAddrV4, now: Instant)`, `clear_marks(&mut self)`;
    - `expire(&mut self, now: Instant)` — закрытые (FIN в обе стороны или RST) раньше `now − LINGER` включительно, без пакетов `IDLE` и дольше, истёкшие пометки;
    - `destinations(&self) -> Vec<SocketAddrV4>` — исходные адреса живых записей;
  - `pub fn pipe(from: &TcpStream, to: &TcpStream, stop: &dyn Fn() -> bool, count: &mut dyn FnMut(u64)) -> std::io::Result<()>` — ставит `from` тайм-аут чтения 1 с, копирует кусками до EOF (`to.shutdown(Write)`), на тайм-ауте проверяет `stop` (true → `Ok`); `count` — байты каждого куска;
  - `pub fn reset(stream: TcpStream)` — `setsockopt(SOL_SOCKET, SO_LINGER, {l_onoff: 1, l_linger: 0})` по `as_raw_socket()` и закрытие: соседу уходит RST (`std` `set_linger` нестабилен); единственный `unsafe` задачи, с `// SAFETY:`.

- [ ] **Шаг 1: Падающие тесты** — `tests/tcp.rs`:

```rust
use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::{Arc, atomic::{AtomicBool, Ordering::SeqCst}};
use std::time::{Duration, Instant};
use router::packet::{ACK, FIN, RST, SYN, Tcp};
use router::tcp::{self, ACCEPT_WAIT, DIRECT_FOR, IDLE, LINGER, Nat, Verdict};

const AP: u16 = 61000;
const CLIENT: &str = "192.168.1.5:50000";
&
fn sa(s: &str) -> std::net::SocketAddrV4 { s.parse().unwrap() }
fn seg(src: &str, dst: &str, flags: u8) -> Tcp<'static> {
    Tcp { src: src.parse().unwrap(), dst: dst.parse().unwrap(), flags, payload: &[] }
}
fn reply(port: u16, flags: u8) -> Tcp<'static> { seg("192.168.1.5:61000", &format!("162.159.135.232:{port}"), flags) }

#[test]
fn a_syn_is_reflected_to_the_acceptor_and_its_replies_back() {
    let t0 = Instant::now();
    let mut nat = Nat::default();
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0), Verdict::Syn);
    let to_acceptor = Verdict::Rewrite { src: sa("162.159.135.232:50000"), dst: sa("192.168.1.5:61000") };
    assert_eq!(nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0), to_acceptor);
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0), to_acceptor, "a retransmitted SYN");
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, ACK), AP, t0), to_acceptor);
    assert_eq!(nat.original(50000, "162.159.135.232".parse().unwrap()), Some(sa(SERVER)));
    assert_eq!(nat.original(50000, "10.0.0.7".parse().unwrap()), None, "a LAN host on the acceptor port");
    assert_eq!(nat.from_acceptor(&reply(50000, SYN | ACK), t0), Verdict::Rewrite { src: sa(SERVER), dst: sa(CLIENT) });
    assert_eq!(nat.destinations(), [sa(SERVER)]);
}

#[test]
fn a_connection_older_than_the_rule_passes_unchanged() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, ACK), AP, t0), Verdict::Pass);
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, SYN | ACK), AP, t0), Verdict::Pass, "only a bare SYN opens");
    assert_eq!(nat.from_acceptor(&reply(4444, ACK), t0), Verdict::Drop, "no entry, no reply");
}

#[test]
fn entries_end_after_fin_both_ways_or_a_reset_and_idle_ones_after_2_minutes() {
    let t0 = Instant::now();
    let mut nat = Nat::default();
    for port in 50001..=50004 {
        nat.admit(&seg(&format!("192.168.1.5:{port}"), SERVER, SYN), AP, t0);
    }
    nat.from_client(&seg("192.168.1.5:50001", SERVER, FIN | ACK), AP, t0);
    nat.from_acceptor(&reply(50001, FIN | ACK), t0);
    nat.from_acceptor(&reply(50002, RST), t0);
    nat.from_client(&seg("192.168.1.5:50003", SERVER, FIN | ACK), AP, t0);
    assert_eq!(nat.from_client(&seg("192.168.1.5:50001", SERVER, SYN), AP, t0), Verdict::Syn, "a closed entry takes a new SYN");
    nat.expire(t0 + LINGER - Duration::from_millis(1));
    assert_eq!(nat.destinations().len(), 4, "a closed entry lingers for the last ACK");
    nat.expire(t0 + LINGER);
    let peer = "162.159.135.232".parse().unwrap();
    let alive: Vec<bool> = (50001..=50004).map(|p| nat.original(p, peer).is_some()).collect();
    assert_eq!(alive, [false, false, true, true], "one FIN keeps it");
    nat.from_client(&seg("192.168.1.5:50004", SERVER, ACK), AP, t0 + Duration::from_secs(60));
    nat.expire(t0 + IDLE);
    assert_eq!(nat.destinations().len(), 1);
    nat.expire(t0 + Duration::from_secs(60) + IDLE);
    assert!(nat.destinations().is_empty());
}

#[test]
fn a_failed_address_goes_direct_for_60_s() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.mark_direct(sa(SERVER), t0);
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0 + DIRECT_FOR - Duration::from_millis(1)), Verdict::Marked);
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0 + DIRECT_FOR), Verdict::Syn);
    nat.mark_direct(sa(SERVER), t0);
    nat.clear_marks();
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0), Verdict::Syn, "a proxy that came back clears the marks");
}

#[test]
fn a_syn_the_acceptor_never_saw_goes_direct_after_3_s() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0);
    nat.admit(&seg("192.168.1.5:50009", SERVER, SYN), AP, t0);
    nat.accepted(50009);
    assert!(nat.unaccepted(t0 + ACCEPT_WAIT - Duration::from_millis(1)).is_empty());
    assert_eq!(nat.unaccepted(t0 + ACCEPT_WAIT), [(50000, sa(SERVER))]);
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0 + ACCEPT_WAIT), Verdict::Marked, "its retry goes direct");
}

fn pair() -> (TcpStream, TcpStream) {
    let l = TcpListener::bind("127.0.0.1:0").unwrap();
    let c = TcpStream::connect(l.local_addr().unwrap()).unwrap();
    (c, l.accept().unwrap().0)
}

#[test]
fn pipe_copies_until_eof_and_counts() {
    let ((mut app, hub_in), (hub_out, mut far)) = (pair(), pair());
    let worker = std::thread::spawn(move || {
        let mut n = 0;
        tcp::pipe(&hub_in, &hub_out, &|| false, &mut |b| n += b).unwrap();
        n
    });
    app.write_all(b"hello").unwrap();
    app.shutdown(Shutdown::Write).unwrap();
    let mut got = Vec::new();
    far.read_to_end(&mut got).unwrap();
    assert_eq!((got.as_slice(), worker.join().unwrap()), (&b"hello"[..], 5));
}

#[test]
fn pipe_ends_when_the_session_stops() {
    let ((_app, hub_in), (hub_out, _far)) = (pair(), pair());
    let stop = Arc::new(AtomicBool::new(false));
    let s = stop.clone();
    let worker = std::thread::spawn(move || tcp::pipe(&hub_in, &hub_out, &|| s.load(SeqCst), &mut |_| {}));
    stop.store(true, SeqCst);
    let started = Instant::now();
    worker.join().unwrap().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
}

#[test]
fn reset_sends_an_rst() {
    let (mut app, hub) = pair();
    tcp::reset(hub);
    assert_eq!(app.read(&mut [0u8; 1]).unwrap_err().kind(), ErrorKind::ConnectionReset);
}
```

- [ ] **Шаг 2:** `cargo test -p router --test tcp` — ошибки компиляции.
- [ ] **Шаг 3:** реализовать `tcp.rs` по Interfaces.
- [ ] **Шаг 4:** `cargo test -p router --test tcp` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): a NAT for reflected TCP, byte pipes and resets`.

---

### Task 9: `Router` — DNS, приёмник, TCP через прокси, домены в UDP

**Files:**
- Modify: `crates/router/src/lib.rs`
- Test: `crates/router/tests/router.rs`, `crates/router/tests/live.rs` (`#[ignore]`)

**Interfaces:**
- Consumes: Tasks 1–8. Публичный API `Router` не меняется.
- Produces (внутреннее): `Inner.names: Arc<Mutex<dns::Names>>` (переживает паузы и перезапуски: приложения держат адреса в кэше дольше паузы); `Live` += `base: Mutex<Table>` (таблица правил), `names` (клон из `Inner`), `nat: Mutex<tcp::Nat>`, `tcp_owners: Owners`, `acceptor: TcpListener` и его порт, `dns: Mutex<Option<Arc<Handle>>>`; `Relay` += `domain: Option<String>`; `Live::recompose(self: &Arc<Self>)` = `apply(base.clone())`.

Схема (тела — исполнителя; порядок решений фиксирован):

```text
open:  + acceptor = TcpListener::bind("0.0.0.0:0"); + DNS handle (DNS_FILTER, Network, FLAG_SNIFF | FLAG_RECV_ONLY);
       spawn dns loop, acceptor loop. Socket loop: socket_protocol() == PROTO_TCP → tcp_owners, else owners.
shut:  + dns handle shutdown; wake the acceptor (TcpStream::connect to 127.0.0.1:<port>); nat cleared.

apply(base): base stored; composed = base.clone().with_names(&names.entries());
   held = nat.destinations() whose composed.matches(Tcp, d) is None → their IPs;
   text = capture_filter(&composed, Some(acceptor port), &held); the table for matching = composed; rest as stage 1
   (same text → no reopen; else at most once a second).

on_packet: udp → stage 1 path; a new flow also: domain = names.name_of(dst.ip) → stats.set_domain, Relay.domain;
           Relay::send → socks_header_to(dst, domain); relay_rx → parse_socks_udp_from(datagram, key.dst)
           tcp = parse_tcp → on_tcp; anything else → send unchanged
on_tcp(seg):
  seg.src.port() == acceptor port → nat.from_acceptor: Rewrite → rewrite_tcp, dll.checksums, inject(addr.as_inbound()); Drop → nothing
  stopped or !unicast → send unchanged
  match nat.from_client(seg, port, now):
    Rewrite → rewrite_tcp + checksums + inject(addr.as_inbound())
    Pass    → send unchanged
    Marked  → table.matches(Tcp, dst) is Some(rule) → row (open + close at once) Direct{Marked}; send unchanged
    Syn     → rule = table.matches(Tcp, seg.dst) else send unchanged
              pid = tcp_owners.wait(seg.src.port(), PID_WAIT); decide(proxy_state, pid, own, proxy_pid)
                Some(reason) → row Direct{reason}, closed at once; send unchanged
                None → nat.admit → Rewrite as above; stats.open(key Tcp, rule, Opening), set_domain(names.name_of)
acceptor loop: accept → stopped? exit : spawn relay_tcp(stream, peer)
relay_tcp: orig = nat.original(peer.port, peer.ip) else reset(stream), return     // nobody from the LAN rides the proxy
   nat.accepted(peer.port); key = FlowKey{Tcp, peer.port, orig}; domain = names.name_of(orig.ip)
   socks::connect(proxy, orig, domain, OPEN_TIMEOUT)
     Err(e) → nat.mark_direct(orig); stats.set_via(Direct{from(&e)}) + close; AuthRequired → set_proxy(AuthRequired); reset(stream)
     Ok(up) → stats.set_via(Proxy); pipe client→up (count up) on a new thread, up→client (count down) here; both ended → stats.close
dns loop: recv → parse_udp → dns::parse(payload) → table.wants(&answer.name) → names.record → true → recompose()
housekeeping, every 1 s: + names.expire; nat.expire; nat.unaccepted → rows Direct{NotAccepted}, closed at once,
   log::warn!("…Windows Firewall…"); then recompose() (cheap: same text, no reopen)
set_proxy (update_proxy, Task 1): recovered → also nat.clear_marks()
close_flow: forgets owners only for UDP keys
```

`// ponytail:` поток на соединение (два потока на TCP-соединение; апгрейд — пул или IOCP, если соединений станут сотни); пауза и остановка обрывают перенаправленные TCP-соединения (переписывать больше некому; программы переподключаются).

- [ ] **Шаг 1: Тесты** — `tests/router.rs`:

```rust
#[test]
fn a_domain_rule_is_saved_and_its_names_are_wanted() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    r.edit(|list| rules::add(list, rules::preset(Preset::Discord))).unwrap();
    assert!(r.table().wants("gateway.discord.gg"));
    assert_eq!(rules::load(&dir.path().join("routes.toml")).unwrap()[0].domains.len(), 5);
}
```

`tests/live.rs` (`#[ignore]`, `WINDIVERT_DLL` и `ROUTER_SOCKS5`):
- `a_running_router_carries_tcp_through_the_proxy` — правило `tcp`, `1.1.1.1/32`, порт `80`; `start`; `TcpStream::connect("1.1.1.1:80")`, запрос `GET / HTTP/1.1\r\nHost: 1.1.1.1\r\nConnection: close\r\n\r\n`, ответ начинается с `HTTP/1.1`; в `flows(false)` строка `protocol == "tcp"`, `via: Proxy`, `down > 0`; `stop`.
- `a_domain_rule_follows_dns` — правило `both`, `domains = ["one.one.one.one"]`; `start`; тест сам шлёт DNS-запрос A `one.one.one.one` на `1.1.1.1:53` (UDP-сокет теста, системный кэш не трогается), ждёт 1,5 с (пересборка фильтра), соединяется с первым IP ответа на порт 80 как выше; строка потока с `domain == Some("one.one.one.one")` и `via: Proxy`.

- [ ] **Шаг 2:** `cargo test -p router --test router` — новый тест проходит уже после Task 2 (страхует сохранение доменов через `edit`); живые — `ignored`.
- [ ] **Шаг 3:** реализовать по схеме.
- [ ] **Шаг 4:** `cargo test -p router` — PASS, `ignored` — 6. Из повышенного терминала с NekoBox: `cargo test -p router --test live -- --ignored` — PASS; нет терминала — «live не запускались» в отчёте.
- [ ] **Шаг 5:** коммит `feat(router): TCP through SOCKS5 CONNECT and domains from DNS`.

---

### Task 10: приложение и CLI — импорт доменов, профиль `discord`

**Files:**
- Modify: `src-tauri/src/commands.rs`, `crates/cli/src/main.rs`
- Generate: `frontend/src/lib/ipc/bindings.ts`

**Interfaces:**
- Consumes: `rules::{import, Imported, Preset::Discord}`.
- Produces: команда `import_route_lines(ips: Vec<String>, domains: Vec<String>, text: String) -> Answer<Imported>` (`rules::import(&ips, &domains, &text)`); `PresetArg::Discord` (значение `discord`) → `Preset::Discord`; doc-комментарий `Flows` без «none in stage 1» не меняется (rule-less потоки — этап 3).

- [ ] **Шаг 1: Падающие тесты** — `crates/cli/src/main.rs`: в `every_command_sends_its_window_command` строка `(&["routes", "preset", "discord"], Request::AddRoutePreset { preset: Preset::Discord })`; в `routes_add_and_edit_read_one_rule_from_a_file` отказ — файл с `apps = ["Discord.exe"]` и `` fail.error.contains("`apps` is not supported yet") ``, плюс принятый файл `"[rule]\nprotocol = \"tcp\"\ndomains = [\"discord.com\"]\n"` → `Request::SaveRoute { rule, create: true }` с `rule.domains == ["discord.com"]`.
- [ ] **Шаг 2:** `cargo test -p cli` — FAIL (нет `discord`).
- [ ] **Шаг 3:** реализовать; `cargo run -p zapret-hub --bin export-bindings` — в `bindings.ts` `Rule.domains`, `Imported`, `Preset` с `"discord"`, `FlowRow.domain`, `DirectReason` с `Marked`/`NotAccepted`, новая сигнатура `importRouteLines`.
- [ ] **Шаг 4:** `cargo test -p cli`, `cargo test -p zapret-hub --lib` — PASS. `npm --prefix frontend run check` до Task 11 падает на новых обязательных полях (`domains`) — ожидаемо; коммит этой задачи — Rust и `bindings.ts`.
- [ ] **Шаг 5:** коммит `feat(cli): the discord preset; import_route_lines fills domains`.

---

### Task 11: вкладка «Маршруты» — протокол, домены, подсказка DoH, колонка «Домен»

**Files:**
- Modify: `frontend/src/lib/strings.ts`, `frontend/src/lib/format.ts`, `frontend/src/lib/format.test.ts`, `frontend/src/tabs/RouteEditor.svelte`, `frontend/src/tabs/Routes.svelte`

**Interfaces:**
- Consumes: `bindings.ts` Task 10.
- Produces: `routeSummary(rule: Rule): string` — протокол (`UDP`, `TCP`, `TCP/UDP`), затем через « · » `N подсеть/подсети/подсетей`, если `N > 0`, и `N домен/домена/доменов`, если `N > 0`.

Строки (`S.routes`): `protocol: "Протокол"` (вместо «Протокол: UDP»), `protocols: { udp: "UDP", tcp: "TCP", both: "TCP и UDP" } satisfies Record<Protocol, string>`, `domains: "Домены (по одному в строке, поддомены входят)"`, `dohHint: "Домены узнаются из ответов DNS. Если браузер или программа включает «безопасный DNS» (DNS по HTTPS), выключите его или добавьте подсети."`, `presets` += `discord: "Discord целиком"`, `columns.domain: "Домен"`, `reasons` += `Marked: "прокси недавно не открыл это соединение"`, `NotAccepted: "брандмауэр Windows не пропустил соединение к хабу"`.

Поведение: редактор — `<select>` протокола, `textarea` доменов с ошибками поля `domains`, подсказка `dohHint` при непустых доменах (спецификация 4.5, 8); импорт — `importRouteLines(lines(ips), lines(domains), text)` заменяет оба списка. `Routes.svelte` — новое правило с `domains: []`; колонка «Домен» после «Назначение» (`flow.domain ?? "—"`).

- [ ] **Шаг 1: Падающие тесты** — `format.test.ts`: `voice` += `domains: []` и:

```ts
it("sums a rule up by protocol, subnets and domains", () => {
  const rule = (over: Partial<Rule>): Rule => ({ ...voice, ports: [...voice.ports], ips: [...voice.ips], domains: [], ...over });
  expect(routeSummary(rule({ domains: ["discord.media"] }))).toBe("UDP · 2 подсети · 1 домен");
  expect(routeSummary(rule({ protocol: "both", ips: [], domains: ["a.com", "b.com", "c.com", "d.com", "e.com"] }))).toBe("TCP/UDP · 5 доменов");
  expect(routeSummary(rule({ protocol: "tcp", ips: [], domains: ["a.com", "b.com"] }))).toBe("TCP · 2 домена");
});
it("names the TCP direct reasons", () => {
  expect(viaLabel({ via: "Direct", reason: { kind: "Marked" } })).toBe("напрямую: прокси недавно не открыл это соединение");
  expect(viaLabel({ via: "Direct", reason: { kind: "NotAccepted" } })).toBe("напрямую: брандмауэр Windows не пропустил соединение к хабу");
});
```

- [ ] **Шаг 2:** `npx --prefix frontend vitest run src/lib/format.test.ts` — FAIL.
- [ ] **Шаг 3:** реализовать по Interfaces и поведению.
- [ ] **Шаг 4:** `npm --prefix frontend test`, `npm --prefix frontend run check` (0/0), `npm --prefix frontend run build` — чисто.
- [ ] **Шаг 5:** коммит `feat(ui): protocol, domains and a DoH hint in the rule editor`.

---

### Task 12: документация, поправки спецификации, версия 0.5.0

**Files:**
- Modify: `README.md`, `docs/cli.md`, `CHANGELOG.md`, `docs/superpowers/specs/2026-10-17-zapret-hub-router-design.md`, `Cargo.toml` (`version = "0.5.0"`), `package.json`, `frontend/package.json`, `package-lock.json`, `Cargo.lock`

Содержание:
- **README**, «Маршрутизация через SOCKS5»: TCP и домены; профиль «Discord целиком»; `domains` в примере `routes.toml` (спецификация 5.1 без `apps`); домены узнаются из DNS — «безопасный DNS» в браузере выключить или добавить подсети; при первом включении Windows может спросить про брандмауэр для `zapret-hub.exe` — разрешить (иначе TCP идёт напрямую с причиной); паузы и выключение обрывают перенаправленные TCP-соединения; этап 3 позже.
- **docs/cli.md**: `routes preset discord-voice|discord`; пункт 18 — `apps` вместо `domains`; «Проверка вживую» 22–27:
  22. NekoBox, «Добавить из профиля» → «Discord целиком», включить: Discord (приложение и discord.com в браузере без «безопасного DNS») загружается; в «Потоках» строки `tcp` «прокси» с доменами `discord.com`/`gateway.discord.gg`; в журнале NekoBox — CONNECT по домену.
  23. Брандмауэр: если Windows спросила про `zapret-hub.exe` — разрешить. С правилом брандмауэра «блокировать входящие» для хаба: строки «напрямую: брандмауэр Windows не пропустил…», сайт открывается напрямую через ~3 с.
  24. Закрыть NekoBox: открытые TCP-соединения рвутся, новые идут напрямую («прокси недоступен»); вернуть — новые снова «прокси».
  25. Правило в NekoBox, блокирующее `discord.com`: соединение получает RST, строка «напрямую», следующие 60 с — «прокси недавно не открыл это соединение», потом снова через прокси.
  26. «Безопасный DNS» в браузере включён: discord.com в браузере не попадает в потоки; выключен — попадает (после перезапуска браузера).
  27. «Импорт из файла…» со списком доменов и подсетей (`.txt`) заполняет оба поля.
- **CHANGELOG** `## 0.5.0`: маршрутизация TCP и правила по доменам (этап 2): перенаправление TCP в локальный приёмник и `CONNECT` к прокси, домены из ответов DNS, профиль «Discord целиком», `discord.media` в «Discord голос», импорт доменов, колонка «Домен».
- **Спецификация** — строки «Поправка при реализации этапа 2» (как поправка этапа 1 в разделе 3): 4.1 — фильтр DNS `ip and udp and udp.SrcPort == 53` (без `inbound`: ответы локального резолвера на loopback); таблица DNS в `Router`, переживает паузы; 4.3 — запись NAT после FIN в обе стороны или RST живёт ещё 10 с (последний ACK), SYN без приёма за 3 с — «напрямую» на 60 с, возврат прокси снимает пометки, адреса живых записей остаются в фильтре; 11 — «больше 64 подсетей» переписать: ближайшие диапазоны сливаются, фильтр шире правил, совпадение сверяет хаб; новые риски — брандмауэр Windows и отражённый SYN, первое соединение после свежего ответа DNS может уйти напрямую (пересборка фильтра до 1 с), пауза обрывает перенаправленные TCP.

- [ ] **Шаг 1:** правки документов и версий (`npm install --package-lock-only`; `cargo check --workspace` обновит `Cargo.lock`).
- [ ] **Шаг 2:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --exclude zapret-hub`, `cargo test -p zapret-hub --lib`, `npm --prefix frontend run check`, `npm --prefix frontend test` — всё чисто.
- [ ] **Шаг 3:** коммит `docs: router stage 2 in the README, cli.md and the spec; version 0.5.0`.

---

## Самопроверка плана

- **Покрытие (этап 2):** 4.1 дескриптор DNS, фильтр с IP из DNS и ответами приёмника — Tasks 5, 9; 4.2 домен в заголовке UDP — 3, 9; 4.3 отражение, NAT, приёмник, CONNECT с доменом, RST + 60 с, срок записи — 3, 6, 8, 9; 4.4 петля для TCP (PID из событий `CONNECT` TCP) — 5 (`SOCKET_FILTER`), 9; 4.5 A/CNAME, сжатие, TTL ≥ 5 мин, подсказка DoH — 4, 11; 5.1 `domains`, LDH, `tcp`/`both`, `apps` по имени — 2; 5.2 профили и импорт доменов — 2, 10, 11; 7 открытые TCP при смене правил — 5 (`held`), 9; 8 колонка «Домен», сводка — 7, 11; 9 `routes preset discord` — 10; 10 тесты и чек-лист — каждая задача, 12; долги этапа 1 — 1.
- **Решения (Ruling):** 1) сверх 64 диапазонов — слияние ближайших вместо «подсети уходят из фильтра»: правило по доменам переполняет фильтр легко, а `both` без портов тогда гнал бы весь TCP через хаб; 2) DNS остаётся SNIFF (спецификация): задержанный ответ DNS при зависшем потоке хаба остановил бы DNS всей системы; фильтр без `inbound` — ради локального резолвера; 3) `Names` в `Router`, не в сессии; 4) `LINGER` 10 с и `ACCEPT_WAIT` 3 с; 5) домен — минимум две метки, IDN только в `xn--`, импорт в нижний регистр; 6) ответ SOCKS5 UDP с доменом — источник = адрес потока; 7) TCP «напрямую» — строка, закрытая сразу (хаб такое соединение не ведёт); 8) два `Owners` (UDP и TCP) вместо ключа с протоколом — сигнатуры этапа 1 целы.
- **Согласованность имён:** `Table::{matches, wants, with_names}`, `capture_filter(table, acceptor, held)`, `Nat::{from_client, admit, from_acceptor, original, accepted, unaccepted, mark_direct, clear_marks, expire, destinations}`, `Verdict::{Pass, Marked, Syn, Rewrite, Drop}`, `Names::{record, name_of, expire, entries}`, `socks::connect`, `FlowKey.protocol`, `DirectReason::{Marked, NotAccepted}`, `Imported` — одинаково в Tasks 2–11.
- **Пропорция:** тела — только формат фильтра и схема `Live`; остальное — сигнатуры и тесты.
