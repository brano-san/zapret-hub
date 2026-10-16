# Zapret Hub — маршрутизация через SOCKS5, этап 1 (UDP по IP и подсетям): план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Цель:** выбранный UDP-трафик (по IPv4-подсетям и портам из `data\routes.toml`) идёт через локальный SOCKS5-прокси (`UDP ASSOCIATE`) прямо из процесса хаба, без TUN и без окна PowerShell: вкладка «Маршруты», пункт трея, команды `router`/`routes` в CLI. Прямая замена `E:\Zapret\utils\discord-udp-proxy.ps1`.

**Архитектура:** новый пакет `crates/router` (без `tauri`): `rules` (формат, проверка, профиль, импорт, таблица совпадений), `packet` (IPv4/UDP, заголовок SOCKS5 UDP), `divert` (фильтр, выбор DLL, адрес WinDivert, загрузка `WinDivert.dll` через `LoadLibraryW`/`GetProcAddress`, дескрипторы), `socks` (приветствие, `UDP ASSOCIATE`), `owner` (PID по событиям сокетов, PID прокси по `GetExtendedTcpTable`), `flows` (решение «прокси/напрямую», очередь, счётчики, строки потоков), `lib.rs` (`Router`: потоки перехвата, реле, сокетов, обслуживания). Приложение держит один `Router` в `AppState`; тонкий модуль `src-tauri/src/routing.rs` собирает `Setup` (DLL + прокси) из настроек и вызывает `Router` из команд, трея, старта, тестов и смены движка.

**Стек:** новых внешних зависимостей нет. Rust 1.99.0 (edition 2024); `windows` 0.62.2 (уже в workspace: `Win32_System_LibraryLoader::{LoadLibraryW, GetProcAddress}`, `Win32_NetworkManagement_IpHelper::{GetExtendedTcpTable, TCP_TABLE_OWNER_PID_LISTENER, MIB_TCPTABLE_OWNER_PID}`); `toml` 1.1.8 (`toml::Spanned` для номеров строк); specta 2.0.0-rc.25 (u64 в TypeScript запрещён — байты в DTO как `f64`); clap 4.6; tauri 2.12; Svelte 5.57.1, svelte-check 4.7.6, vitest 5.0.3. WinDivert 2.2 API — из DLL движка, ничего не ставится.

**Спецификация:** `docs/superpowers/specs/2026-10-17-zapret-hub-router-design.md` (далее «спецификация»); этап 1 = её раздел 2, пункт 1. Основа — `2026-10-10-zapret-hub-core-design.md` («core»), `2026-10-11-zapret-hub-cli-design.md` («cli»), `2026-10-11-zapret-hub-zapret2-design.md` («zapret2»). Исполнитель читает спецификацию вместе с планом. Справка (только чтение): `E:\Zapret\utils\discord-udp-proxy.ps1`; `E:\Zapret` не менять.

**База:** `main` на `5b50643`; ветка `feat/router` от неё.

## Общие ограничения

- Только Windows 11 x64, Rust stable msvc (`rust-toolchain.toml`: 1.99.0). Крейты не зависят от `tauri` (только derive `specta`). Логика — в `crates/router`; команда в `src-tauri/src/commands.rs` — несколько строк, тела — в `routing.rs`.
- WinDivert только динамически: `LoadLibraryW` полного пути DLL из папки движка + `GetProcAddress` для `WinDivertOpen`, `WinDivertRecv`, `WinDivertSend`, `WinDivertShutdown`, `WinDivertClose`, `WinDivertHelperCalcChecksums`. Ничего не устанавливается, `FreeLibrary` не вызывается (DLL живёт до конца процесса). Новый крейт Windows API не добавляется: `windows` 0.62 из workspace.
- DLL (спецификация 3.1): запущен `winws2.exe` → `<папка winws2.exe>\WinDivert.dll`; запущен `winws.exe` → `<zapret_dir>\bin\WinDivert.dll`; DLL запущенного движка нет — ошибка, без подмены. Ничего не запущено — Flowseal, затем zapret2.
- Дескрипторы: перехват — слой NETWORK (0), приоритет `1000`, флаги `0`; сокеты — слой SOCKET (3), приоритет `1000`, флаги `SNIFF | RECV_ONLY` (`0x1 | 0x4`). Фильтр сокетов: `!loopback and udp and (event == BIND or event == CONNECT or event == CLOSE)`.
- Фильтр перехвата: `outbound and !loopback and ip and (<части>)`; пустая таблица — дескриптор не открыт; пересборка не чаще раза в секунду, новый дескриптор открывается до закрытия старого. Больше `MAX_FILTER_RANGES = 64` диапазонов IP — подсети уходят из фильтра (остаются порты), совпадение проверяет хаб.
- Значения: ожидание прокси при открытии потока `3 с`; очередь потока `64` пакета (старые выбрасываются); поток без пакетов `30 с` закрывается; закрытые потоки видны `30 с`; ожидание PID `50 мс`; проверка прокси `10 с`; проверка времени записи `routes.toml` `2 с`.
- `routes.toml` этапа 1: поля `name`, `enabled`, `protocol`, `ports`, `ips`. `domains`, `apps` — ошибка «`<поле>` is not supported yet» с номером строки; `protocol` кроме `"udp"` — так же. Профиль только `discord-voice`.
- Только IPv4 и прокси без авторизации (спецификация 2, «вне объёма»).
- Тесты не меняют систему: модульные — чистые; SOCKS5 — против сервера на loopback внутри теста; `GetExtendedTcpTable` и `LoadLibraryW` несуществующего файла только читают; всё, что открывает WinDivert, — `#[ignore]` (нужны права администратора и `WINDIVERT_DLL=<путь к WinDivert.dll>`).
- Тексты: код, комментарии, ошибки Rust и коммиты — английский (как во всём репозитории: «`domains` is not supported yet» — смысл фразы из спецификации); интерфейс, `texts.rs`, `strings.ts`, `docs/`, README — русский. Состояния прокси и причины «напрямую» идут в окно перечислениями, русский текст — в `strings.ts` и `texts.rs`.
- Коммит — одна строка Conventional Commits (scope `router`, `app`, `cli`, `ui`, `docs`, `build`), без тела и без трейлеров (никаких `Co-Authored-By`). `git add` с явными путями.
- Перед каждым коммитом с Rust: `cargo fmt --all` и `cargo clippy --workspace --all-targets -- -D warnings` без замечаний; с фронтендом — `npm --prefix frontend run check` (0 ошибок, 0 предупреждений) и `npm --prefix frontend test`.
- Тесты приложения — `cargo test -p zapret-hub --lib` (у бинарника `requireAdministrator` и `test = false`).
- После изменения DTO или команд — `cargo run -p zapret-hub --bin export-bindings`, `frontend/src/lib/ipc/bindings.ts` в том же коммите.

## На что смотреть при ревью

1. **`routes.toml` руками в Блокноте.** BOM и CRLF читаются; битый файл не стирается правкой из окна или CLI: правка отказывает, пока файл не исправлен, а работающие правила остаются прежними. Тесты: `a_notepad_file_with_a_bom_parses` (задача 1), `an_edit_never_overwrites_a_broken_file`, `a_broken_file_keeps_the_rules_and_shows_where` (задача 6).
2. **Большой импорт** (сотни подсетей из списка ipset). Пересекающиеся и соседние подсети сливаются; больше 64 диапазонов — фильтр по портам, совпадение в хабе; WinDivert не отказывает длинным фильтром. Тесты: `overlapping_and_adjacent_subnets_merge`, `too_many_ranges_leave_the_ips_to_the_hub` (задача 2), `packets_outside_the_rules_go_back_unchanged` (задача 4).
3. **Прокси не запущен при входе в систему или упал посреди звонка.** Новые потоки сразу идут напрямую, открытие ждёт не дольше 3 с, очередь не растёт сверх 64, после возврата прокси новые потоки снова через него. Тесты: `a_silent_proxy_times_out` (задача 3), `the_queue_keeps_the_newest_64`, `new_flows_follow_the_proxy_state` (задача 4).
4. **Петля через прокси**: сервер VPN пользователя лежит в подсети правила. Трафик процесса, слушающего порт прокси, и трафик самого хаба не перехватываются. Тесты: `traffic_of_the_proxy_and_of_the_hub_goes_direct`, `the_listener_of_a_port_is_found`, `a_late_socket_event_is_waited_for_50_ms` (задача 4).
5. **Устаревший `router_enabled`.** Вкладка «Настройки», открытая до переключения, и `settings set` из CLI не включают и не выключают маршрутизацию: флаг пишет только `set_router_enabled`. Тест: `a_settings_save_keeps_router_enabled` (задача 7).

---

## Структура файлов

```
Cargo.toml                         + router в [workspace.dependencies]; версия 0.4.0 (задача 10)
crates/router/                     новый пакет
  Cargo.toml
  src/lib.rs                       Router, Setup, parse_proxy, RouterStatus и прочие DTO; потоки
  src/rules.rs                     Rule, Protocol, Preset, проверка, parse/load/save, импорт, правки, Table
  src/packet.rs                    parse_udp, build_udp, socks_header, parse_socks_udp
  src/divert.rs                    filter, SOCKET_FILTER, Engine, pick_dll, Address, Dll, Handle
  src/socks.rs                     SocksError, check, udp_associate
  src/owner.rs                     Owners, PID_WAIT, listener_pid, port_of
  src/flows.rs                     Pending, decide, Stats, FlowRow, Via, DirectReason, Traffic
  tests/rules.rs, tests/packet.rs, tests/filter.rs, tests/socks.rs, tests/flows.rs,
  tests/router.rs, tests/live.rs (#[ignore])
crates/cli/
  Cargo.toml                       + router, toml не нужен
  src/lib.rs                       Request: SetRouterEnabled, RouterStatus, ListRoutes, GetRoute, SaveRoute,
                                   SetRouteEnabled, DeleteRoute, AddRoutePreset, RouteFlows
  src/main.rs                      router on|off|status, routes list|show|add|edit|enable|disable|remove|preset|flows
src-tauri/
  Cargo.toml                       + router
  src/settings.rs                  Settings.router_enabled, to_save его не меняет
  src/state.rs                     AppState.router, Paths::routes()
  src/routing.rs                   новый: setup, sync, set_enabled, paused, switches_engine
  src/commands.rs, src/lib.rs      11 команд
  src/actions.rs                   apply со сменой движка на паузе; save_settings → routing::sync
  src/jobs.rs                      пауза на время тестов; восстановление при старте
  src/tray.rs, src/texts.rs        пункт «Маршрутизация», пометка на значке
frontend/src/
  lib/ipc/bindings.ts              (генерируется)
  lib/app.svelte.ts                Tab += "routes"
  lib/format.ts, lib/format.test.ts  routeSummary, bytes, viaLabel
  lib/strings.ts                   S.tabs.routes, S.routes
  tabs/Routes.svelte               новый: шапка, правила, потоки
  tabs/RouteEditor.svelte          новый: редактор правила
  App.svelte                       вкладка «Маршруты»
README.md, docs/cli.md, CHANGELOG.md, docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md
package.json, frontend/package.json, package-lock.json   версия 0.4.0
```

Зависимости: `router → (anyhow, log, serde, specta, toml, windows)`; `cli → router`; `src-tauri → router`. `router` не зависит от `zapret`/`zapret2`: папки и запущенный движок ему передаёт приложение.

---

### Задача 1: правила — формат `routes.toml`, проверка, профиль, импорт, таблица

**Files:**
- Modify: `Cargo.toml` (`router = { path = "crates/router" }` в `[workspace.dependencies]`)
- Create: `crates/router/Cargo.toml`, `crates/router/src/lib.rs` (пока `pub mod rules;`), `crates/router/src/rules.rs`
- Test: `crates/router/tests/rules.rs`

`crates/router/Cargo.toml`: `[package]` как у `crates/zapret2` (`*.workspace = true`), `description = "Routes chosen traffic of Zapret Hub through a local SOCKS5 proxy with WinDivert."`; зависимости `anyhow`, `log`, `serde`, `specta`, `toml` (workspace), `windows = { workspace = true, features = ["Win32_Foundation", "Win32_NetworkManagement_IpHelper", "Win32_System_LibraryLoader"] }`; dev — `tempfile`.

**Interfaces:**
- Produces (`router::rules`), все типы — `Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type`:
  - `struct Rule { name: String, enabled: bool, protocol: Protocol, ports: Vec<String>, ips: Vec<String> }`;
  - `enum Protocol { Tcp, Udp, Both }`, `#[serde(rename_all = "lowercase")]`;
  - `enum Preset { DiscordVoice }`, `#[serde(rename_all = "kebab-case")]` (`"discord-voice"`), `+ clap`-независимый;
  - `struct FieldError { field: String, message: String }`;
  - `struct RulesError { line: Option<u32>, field: Option<String>, message: String }` + `Display` (`line 7: ports: …`, без отсутствующих частей) + `std::error::Error`;
  - `fn preset(preset: Preset) -> Rule`;
  - `fn check(rule: &Rule) -> Vec<FieldError>` — одно правило, без файла (редактор «на лету», CLI);
  - `fn parse(text: &str) -> Result<Vec<Rule>, RulesError>` — весь файл; `fn parse_rule_file(name: &str, text: &str) -> Result<Rule, RulesError>` — файл CLI с одной таблицей `[rule]` без `name`;
  - `fn load(path: &Path) -> Result<Vec<Rule>, RulesError>` (нет файла — пусто); `fn save(path: &Path, rules: &[Rule]) -> anyhow::Result<()>` (через `routes.toml.tmp` + `rename`, как `settings::save`; `[[rule]]`);
  - `fn import(ips: &[String], text: &str) -> Result<Vec<String>, RulesError>` — `ips` плюс новые строки файла, без повторов;
  - `fn add(rules: &mut Vec<Rule>, rule: Rule) -> anyhow::Result<()>`, `fn replace(rules: &mut [Rule], rule: Rule) -> anyhow::Result<()>`, `fn set_enabled(rules: &mut [Rule], name: &str, enabled: bool) -> anyhow::Result<()>`, `fn remove(rules: &mut Vec<Rule>, name: &str) -> anyhow::Result<Rule>`, `fn find<'a>(rules: &'a [Rule], name: &str) -> anyhow::Result<&'a Rule>` — имя без учёта регистра (`to_lowercase`, кириллица тоже); ошибки `a rule named {name} already exists`, `no rule named {name}`;
  - `struct Table { pub rules: Vec<Compiled> }`, `struct Compiled { pub name: String, pub ports: Vec<(u16, u16)>, pub ranges: Vec<(u32, u32)> }` — только включённые правила; `ranges` отсортированы и слиты; `Table::new(rules: &[Rule]) -> Table` (правила уже проверены), `Table::is_empty(&self) -> bool`, `Table::match_udp(&self, dst: SocketAddrV4) -> Option<&str>` — имя первого совпавшего правила в порядке файла (порты пусто = все).

Правила проверки (`check`, спецификация 5.1): имя после `trim` — 1–40 символов (`chars().count()`), первый — буква или цифра, остальные — латиница, кириллица (`а-яА-ЯёЁ`), цифры, пробел, `-_.` → поле `name`; `protocol` ≠ `Udp` → поле `protocol`, `protocol "tcp" is not supported yet: only "udp"`; порт — `n` или `a-b`, `1 ≤ a ≤ b ≤ 65535` → поле `ports`; IP — `a.b.c.d` или `a.b.c.d/n`, `n ≤ 32`, биты хоста допускаются и обнуляются → поле `ips`; пустые `ips` → поле `ips`, `a rule needs at least one target: ips`. Файл (`parse`): сырые структуры с `toml::Spanned` и `#[serde(deny_unknown_fields)]`, поля `domains`/`apps` (`Option<Spanned<toml::Value>>`) → `` `domains` is not supported yet ``; номер строки — `1 +` число `\n` до `span().start`; ошибка `check` получает строку своего поля; повтор имени — строка имени второго правила; синтаксис TOML — строка из `span()` ошибки `toml`. `enabled` по умолчанию `true`, `ports` и `ips` — `[]`, `protocol` обязателен. BOM (`\u{feff}`) в начале снимается.

- [ ] **Шаг 1: Падающие тесты** — `crates/router/tests/rules.rs`:

```rust
use router::rules::{self, Preset, Protocol, Rule, Table};

fn voice() -> Rule {
    rules::preset(Preset::DiscordVoice)
}

#[test]
fn the_discord_voice_preset_is_the_spec() {
    assert_eq!(
        voice(),
        Rule {
            name: "Discord голос".into(),
            enabled: true,
            protocol: Protocol::Udp,
            ports: vec!["19294-19344".into(), "50000-65535".into()],
            ips: vec!["104.29.0.0/16".into(), "35.217.0.0/16".into()],
        }
    );
    assert!(rules::check(&voice()).is_empty());
}

#[test]
fn a_file_round_trips_and_a_missing_one_is_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    assert_eq!(rules::load(&path).unwrap(), Vec::<Rule>::new());
    rules::save(&path, &[voice()]).unwrap();
    assert_eq!(rules::load(&path).unwrap(), [voice()]);
    assert!(std::fs::read_to_string(&path).unwrap().contains("[[rule]]"));
}

#[test]
fn a_notepad_file_with_a_bom_parses() {
    let text = "\u{feff}[[rule]]\r\nname = \"v\"\r\nprotocol = \"udp\"\r\nips = [\"1.2.3.0/24\"]\r\n";
    let parsed = rules::parse(text).unwrap();
    assert_eq!(parsed[0].name, "v");
    assert!(parsed[0].enabled, "enabled defaults to true");
    assert!(parsed[0].ports.is_empty());
}

#[test]
fn errors_name_the_line_and_the_field() {
    let text = "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n\n\
                [[rule]]\nname = \"b\"\nprotocol = \"udp\"\nports = [\"70000\"]\nips = [\"1.1.1.1\"]\n";
    let err = rules::parse(text).unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(9), Some("ports")));
    assert!(err.to_string().starts_with("line 9: ports: "), "{err}");
}

#[test]
fn fields_of_later_stages_are_named() {
    for (field, value) in [("domains", "[\"discord.media\"]"), ("apps", "[\"Discord.exe\"]")] {
        let text = format!("[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n{field} = {value}\n");
        let err = rules::parse(&text).unwrap_err();
        assert_eq!(err.line, Some(5));
        assert!(err.to_string().contains(&format!("`{field}` is not supported yet")), "{err}");
    }
    let tcp = "[[rule]]\nname = \"a\"\nprotocol = \"tcp\"\nips = [\"1.1.1.1\"]\n";
    let err = rules::parse(tcp).unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(3), Some("protocol")));
    assert!(rules::parse("[[rule]]\nname = \"a\"\ncolour = 1\n").unwrap_err().to_string().contains("colour"));
}

#[test]
fn names_follow_the_strategy_rules_plus_cyrillic() {
    let named = |name: &str| Rule { name: name.into(), ..voice() };
    assert!(rules::check(&named("Discord голос")).is_empty());
    for bad in ["", " ", "a/b", "-x", &"a".repeat(41)] {
        assert_eq!(rules::check(&named(bad))[0].field, "name", "{bad:?}");
    }
    let text = "[[rule]]\nname = \"Discord голос\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n\
                [[rule]]\nname = \"DISCORD ГОЛОС\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n";
    let err = rules::parse(text).unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(6), Some("name")));
}

#[test]
fn ports_and_subnets_are_checked() {
    let with = |ports: &[&str], ips: &[&str]| Rule {
        ports: ports.iter().map(|s| s.to_string()).collect(),
        ips: ips.iter().map(|s| s.to_string()).collect(),
        ..voice()
    };
    let fields = |rule: Rule| rules::check(&rule).into_iter().map(|e| e.field).collect::<Vec<_>>();
    assert_eq!(fields(with(&["443", "1-65535"], &["10.0.0.1", "10.1.2.3/8"])), Vec::<String>::new());
    for ports in [["0"], ["100-50"], ["65536"], ["x"]] {
        assert_eq!(fields(with(&ports, &["1.1.1.1"])), ["ports"], "{ports:?}");
    }
    for ips in [["300.1.1.1"], ["1.2.3.0/33"], ["discord.media"], ["::1"]] {
        assert_eq!(fields(with(&[], &ips)), ["ips"], "{ips:?}");
    }
    assert_eq!(fields(with(&[], &[])), ["ips"], "a rule needs a target");
}

#[test]
fn a_cli_rule_file_has_no_name() {
    let text = "[rule]\nprotocol = \"udp\"\nips = [\"104.29.0.0/16\"]\n";
    let rule = rules::parse_rule_file("Мой голос", text).unwrap();
    assert_eq!((rule.name.as_str(), rule.enabled), ("Мой голос", true));
    let named = "[rule]\nname = \"x\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n";
    assert_eq!(rules::parse_rule_file("x", named).unwrap_err().field.as_deref(), Some("name"));
}

#[test]
fn import_takes_ip_lines_once_and_refuses_the_rest() {
    let have = vec!["104.29.0.0/16".to_string()];
    let text = "\u{feff}# voice\r\n104.29.0.0/16\r\n\r\n35.217.0.0/16\n 35.217.0.0/16 \n";
    assert_eq!(rules::import(&have, text).unwrap(), ["104.29.0.0/16", "35.217.0.0/16"]);
    let err = rules::import(&have, "1.1.1.1\ndiscord.media\n").unwrap_err();
    assert_eq!(err.line, Some(2));
    assert!(err.to_string().contains("discord.media"), "{err}");
}

#[test]
fn edits_go_by_name_without_case() {
    let mut list = vec![voice()];
    assert!(rules::add(&mut list, Rule { name: "discord ГОЛОС".into(), ..voice() }).is_err());
    rules::set_enabled(&mut list, "DISCORD голос", false).unwrap();
    assert!(!list[0].enabled);
    assert!(rules::replace(&mut list, Rule { name: "other".into(), ..voice() }).is_err());
    assert!(rules::find(&list, "nope").is_err());
    assert_eq!(rules::remove(&mut list, "discord голос").unwrap().name, "Discord голос");
    assert!(list.is_empty());
}

#[test]
fn the_table_matches_enabled_rules_by_port_and_subnet() {
    let off = Rule { name: "off".into(), enabled: false, ips: vec!["8.8.8.0/24".into()], ..voice() };
    let table = Table::new(&[voice(), off]);
    let at = |addr: &str| table.match_udp(addr.parse().unwrap());
    assert_eq!(at("104.29.5.6:50001"), Some("Discord голос"));
    assert_eq!(at("35.217.0.1:19294"), Some("Discord голос"));
    assert_eq!(at("35.217.255.255:65535"), Some("Discord голос"));
    assert_eq!(at("35.217.0.1:19293"), None);
    assert_eq!(at("104.29.5.6:443"), None);
    assert_eq!(at("8.8.8.8:50001"), None, "a disabled rule matches nothing");
    assert!(Table::new(&[]).is_empty());
}
```

- [ ] **Шаг 2: Убедиться, что падают** — `cargo test -p router --test rules`: ошибки компиляции (нет `router::rules`).
- [ ] **Шаг 3: Реализовать** `crates/router/src/rules.rs` по Interfaces и правилам проверки выше. Слияние диапазонов в `Table::new`: сортировка по началу, соседние (`next.0 <= cur.1.saturating_add(1)`) и пересекающиеся сливаются.
- [ ] **Шаг 4: Тесты проходят** — `cargo test -p router --test rules`: PASS.
- [ ] **Шаг 5: Коммит** — `git add Cargo.toml Cargo.lock crates/router` → `feat(router): routes.toml rules with validation, the voice preset and import`

---

### Задача 2: пакеты, заголовок SOCKS5 UDP, адрес и фильтр WinDivert, выбор DLL

**Files:**
- Create: `crates/router/src/packet.rs`, `crates/router/src/divert.rs` (в этой задаче — только чистая часть: `filter`, `SOCKET_FILTER`, `MAX_FILTER_RANGES`, `Engine`, `pick_dll`, `Address`)
- Modify: `crates/router/src/lib.rs` (`pub mod packet; pub mod divert;`)
- Test: `crates/router/tests/packet.rs`, `crates/router/tests/filter.rs`

**Interfaces:**
- Consumes: `rules::{Table, Compiled}`.
- Produces:
  - `packet::Udp<'a> { pub src: SocketAddrV4, pub dst: SocketAddrV4, pub payload: &'a [u8] }`;
  - `packet::parse_udp(packet: &[u8]) -> Option<Udp<'_>>` — IPv4 (`version 4`, `IHL ≥ 5`), протокол 17, не фрагмент (флаг MF или смещение ≠ 0 → `None`), длины согласованы;
  - `packet::build_udp(src: SocketAddrV4, dst: SocketAddrV4, payload: &[u8], id: u16) -> Vec<u8>` — контрольные суммы нулевые (их считает `WinDivertHelperCalcChecksums`, задача 5);
  - `packet::socks_header(dst: SocketAddrV4) -> [u8; 10]`; `packet::parse_socks_udp(datagram: &[u8]) -> Option<(SocketAddrV4, &[u8])>` — `RSV 0 0`, `FRAG 0`, `ATYP 1`, иначе `None`;
  - `divert::MAX_FILTER_RANGES: usize = 64`; `divert::SOCKET_FILTER: &str` (Общие ограничения); `divert::filter(table: &Table) -> Option<String>`;
  - `divert::Engine { Flowseal, Zapret2 }`; `divert::pick_dll(running: Option<Engine>, flowseal_root: Option<&Path>, winws2_dir: Option<&Path>, exists: impl Fn(&Path) -> bool) -> anyhow::Result<PathBuf>`;
  - `#[repr(C, align(8))] pub struct divert::Address(pub [u8; 80])` (`WINDIVERT_ADDRESS` 2.2): `outbound(&self) -> bool` (байт 10, бит `0x02`), `as_inbound(&self) -> Address` (байт 10 `&= !0x0A`: сброшены Outbound и Impostor, остальное как было), `event(&self) -> u8` (байт 9), `socket_pid(&self) -> u32` (байты 32..36, LE), `socket_local_port(&self) -> u16` (68..70, LE), `socket_protocol(&self) -> u8` (72); константа `divert::EVENT_SOCKET_CLOSE: u8 = 7`.

Формат фильтра (тесты фиксируют его целиком; тело — для однозначности):

```rust
// per rule: "(udp and (<ports>) and (<ips>))"; no ports → "(udp and (<ips>))";
// over MAX_FILTER_RANGES ranges in the whole table → every rule drops "and (<ips>)".
fn port(&(a, b): &(u16, u16)) -> String {
    match (a, b) {
        _ if a == b => format!("udp.DstPort == {a}"),
        (_, 65535) => format!("udp.DstPort >= {a}"),
        _ => format!("(udp.DstPort >= {a} and udp.DstPort <= {b})"),
    }
}
fn range(&(lo, hi): &(u32, u32)) -> String {
    let ip = |x: u32| std::net::Ipv4Addr::from(x);
    if lo == hi { format!("ip.DstAddr == {}", ip(lo)) }
    else { format!("(ip.DstAddr >= {} and ip.DstAddr <= {})", ip(lo), ip(hi)) }
}
// parts joined by " or "; Some(format!("outbound and !loopback and ip and ({parts})")), None when the table is empty
```

`pick_dll`: `Some(Flowseal)` → `<flowseal_root>\bin\WinDivert.dll`, `Some(Zapret2)` → `<winws2_dir>\WinDivert.dll`; файла нет (или папки) — ошибка `{path} is missing: the running engine's driver needs its own WinDivert.dll`; `None` → первый существующий из двух, иначе `WinDivert.dll was not found in the zapret or the zapret2 folder` (с путями, что проверены).

- [ ] **Шаг 1: Падающие тесты**

`crates/router/tests/packet.rs`:

```rust
use router::divert::Address;
use router::packet::{build_udp, parse_socks_udp, parse_udp, socks_header};

#[test]
fn a_built_packet_has_the_scripts_bytes_and_parses_back() {
    let p = build_udp("1.2.3.4:50001".parse().unwrap(), "192.168.1.5:61000".parse().unwrap(), b"hi", 7);
    assert_eq!(p.len(), 30);
    assert_eq!(p[..12], [0x45, 0, 0, 30, 0, 7, 0x40, 0, 64, 17, 0, 0]);
    assert_eq!(p[12..20], [1, 2, 3, 4, 192, 168, 1, 5]);
    assert_eq!(p[20..28], [0xC3, 0x51, 0xEE, 0x48, 0, 10, 0, 0]);
    let udp = parse_udp(&p).unwrap();
    assert_eq!((udp.src.to_string(), udp.dst.to_string(), udp.payload), ("1.2.3.4:50001".into(), "192.168.1.5:61000".into(), &b"hi"[..]));
}

#[test]
fn not_udp_fragments_and_short_packets_do_not_parse() {
    let p = build_udp("1.2.3.4:1".parse().unwrap(), "5.6.7.8:2".parse().unwrap(), b"x", 0);
    let mut tcp = p.clone();
    tcp[9] = 6;
    let mut fragment = p.clone();
    fragment[6] = 0x20; // MF
    assert!(parse_udp(&tcp).is_none() && parse_udp(&fragment).is_none());
    assert!(parse_udp(&p[..27]).is_none() && parse_udp(&[0x60; 40]).is_none());
}

#[test]
fn the_socks_udp_header_names_the_destination() {
    assert_eq!(socks_header("35.217.1.2:50005".parse().unwrap()), [0, 0, 0, 1, 35, 217, 1, 2, 0xC3, 0x55]);
    let datagram = [0, 0, 0, 1, 35, 217, 1, 2, 0xC3, 0x55, b'o', b'k'];
    let (from, data) = parse_socks_udp(&datagram).unwrap();
    assert_eq!((from.to_string(), data), ("35.217.1.2:50005".into(), &b"ok"[..]));
    let mut fragment = datagram;
    fragment[2] = 1;
    let mut domain = datagram;
    domain[3] = 3;
    assert!(parse_socks_udp(&fragment).is_none() && parse_socks_udp(&domain).is_none());
    assert!(parse_socks_udp(&datagram[..9]).is_none());
}

#[test]
fn an_outbound_address_turns_inbound_and_socket_fields_read() {
    let mut raw = [0u8; 80];
    raw[8] = 3;
    raw[9] = 4;
    raw[10] = 0b0000_1011;
    raw[16] = 0xAA;
    raw[32..36].copy_from_slice(&1234u32.to_le_bytes());
    raw[68..70].copy_from_slice(&50001u16.to_le_bytes());
    raw[72] = 17;
    let addr = Address(raw);
    assert!(addr.outbound());
    let inbound = addr.as_inbound();
    assert!(!inbound.outbound());
    assert_eq!(inbound.0[10], 0b0000_0001);
    assert_eq!((inbound.0[..10], inbound.0[11..]), (raw[..10], raw[11..]));
    assert_eq!((addr.event(), addr.socket_pid(), addr.socket_local_port(), addr.socket_protocol()), (4, 1234, 50001, 17));
}
```

`crates/router/tests/filter.rs`:

```rust
use std::path::{Path, PathBuf};
use router::divert::{Engine, filter, pick_dll};
use router::rules::{self, Preset, Rule, Table};

fn rule(ports: &[&str], ips: &[&str]) -> Rule {
    Rule {
        name: "r".into(),
        ports: ports.iter().map(|s| s.to_string()).collect(),
        ips: ips.iter().map(|s| s.to_string()).collect(),
        ..rules::preset(Preset::DiscordVoice)
    }
}

#[test]
fn the_voice_preset_makes_the_scripts_filter() {
    assert_eq!(
        filter(&Table::new(&[rules::preset(Preset::DiscordVoice)])).unwrap(),
        "outbound and !loopback and ip and ((udp and ((udp.DstPort >= 19294 and udp.DstPort <= 19344) \
         or udp.DstPort >= 50000) and ((ip.DstAddr >= 35.217.0.0 and ip.DstAddr <= 35.217.255.255) \
         or (ip.DstAddr >= 104.29.0.0 and ip.DstAddr <= 104.29.255.255))))"
    );
}

#[test]
fn single_values_and_missing_ports_are_short() {
    assert_eq!(
        filter(&Table::new(&[rule(&["443"], &["1.1.1.1"]), rule(&[], &["10.0.0.0/8"])])).unwrap(),
        "outbound and !loopback and ip and ((udp and (udp.DstPort == 443) and (ip.DstAddr == 1.1.1.1)) \
         or (udp and ((ip.DstAddr >= 10.0.0.0 and ip.DstAddr <= 10.255.255.255))))"
    );
    assert_eq!(filter(&Table::new(&[])), None, "no rules, no handle");
    assert_eq!(filter(&Table::new(&[Rule { enabled: false, ..rule(&[], &["1.1.1.1"]) }])), None);
}

#[test]
fn overlapping_and_adjacent_subnets_merge() {
    let table = Table::new(&[rule(&[], &["10.0.0.0/24", "10.0.1.0/24", "10.0.0.128/25"])]);
    assert_eq!(table.rules[0].ranges, [(0x0A00_0000, 0x0A00_01FF)]);
}

#[test]
fn too_many_ranges_leave_the_ips_to_the_hub() {
    let ips: Vec<String> = (0..65).map(|i| format!("10.{i}.0.0/24")).collect();
    let ips: Vec<&str> = ips.iter().map(String::as_str).collect();
    let text = filter(&Table::new(&[rule(&["50000-65535"], &ips)])).unwrap();
    assert_eq!(text, "outbound and !loopback and ip and ((udp and (udp.DstPort >= 50000)))");
}

#[test]
fn the_dll_comes_from_the_running_engine() {
    let flowseal = Path::new(r"E:\Zapret");
    let winws2 = Path::new(r"D:\zapret2\binaries\windows-x86_64");
    let fs_dll = PathBuf::from(r"E:\Zapret\bin\WinDivert.dll");
    let z2_dll = PathBuf::from(r"D:\zapret2\binaries\windows-x86_64\WinDivert.dll");
    let all = |_: &Path| true;
    assert_eq!(pick_dll(Some(Engine::Zapret2), Some(flowseal), Some(winws2), all).unwrap(), z2_dll);
    assert_eq!(pick_dll(Some(Engine::Flowseal), Some(flowseal), Some(winws2), all).unwrap(), fs_dll);
    assert_eq!(pick_dll(None, Some(flowseal), Some(winws2), all).unwrap(), fs_dll, "Flowseal first");
    let only_z2 = |path: &Path| path == z2_dll;
    assert_eq!(pick_dll(None, Some(flowseal), Some(winws2), only_z2).unwrap(), z2_dll);
    let err = pick_dll(Some(Engine::Flowseal), Some(flowseal), Some(winws2), only_z2).unwrap_err();
    assert!(format!("{err:#}").contains(r"E:\Zapret\bin\WinDivert.dll"), "no fallback for a running engine: {err:#}");
    assert!(pick_dll(None, None, None, all).is_err());
}
```

- [ ] **Шаг 2: Убедиться, что падают** — `cargo test -p router --test packet --test filter`: ошибки компиляции.
- [ ] **Шаг 3: Реализовать** `packet.rs` и чистую часть `divert.rs` по Interfaces; `build_udp` — байты как в тесте (`DF`, TTL 64, как скрипт).
- [ ] **Шаг 4: Тесты проходят** — та же команда: PASS.
- [ ] **Шаг 5: Коммит** — `feat(router): IPv4/UDP packets, the SOCKS5 UDP header and the WinDivert filter`

---

### Задача 3: клиент SOCKS5

**Files:**
- Create: `crates/router/src/socks.rs`; Modify: `crates/router/src/lib.rs` (`pub mod socks;`)
- Test: `crates/router/tests/socks.rs`

**Interfaces:**
- Produces (`router::socks`):
  - `OPEN_TIMEOUT: Duration = 3 s`;
  - `enum SocksError { Timeout, AuthRequired, Refused(u8), Io(std::io::Error) }` + `Display`: `the proxy did not answer in time`, `the proxy asks for authentication`, `the proxy refused UDP ASSOCIATE: code {n}` (`7` → `the proxy does not support UDP ASSOCIATE`), `{io}`; `io::ErrorKind::{TimedOut, WouldBlock}` превращаются в `Timeout`;
  - `fn check(proxy: SocketAddrV4, timeout: Duration) -> Result<Duration, SocksError>` — connect + приветствие, время ответа;
  - `struct Association { pub control: TcpStream, pub relay: SocketAddrV4 }`;
  - `fn udp_associate(proxy: SocketAddrV4, dst: SocketAddrV4, timeout: Duration) -> Result<Association, SocksError>`.

Байты: приветствие `05 01 00`, ответ `05 00`; любой другой метод (`05 02`, `05 FF`) → `AuthRequired`. Запрос `05 03 00 01 <dst ip> <dst port BE>` (адресат в запросе, как скрипт: VLESS без XUDP). Ответ `05 REP 00 ATYP …`: `REP ≠ 0` → `Refused(REP)`; адрес по ATYP (1 → 4, 4 → 16, 3 → 1 + длина) + порт. `relay = SocketAddrV4::new(*proxy.ip(), BND.PORT)` — адрес из ответа не берётся (как скрипт: прокси локальный). Тайм-аут — на connect, чтение и запись.

- [ ] **Шаг 1: Падающие тесты** — `crates/router/tests/socks.rs`; сервер — `TcpListener::bind("127.0.0.1:0")` в потоке теста:

```rust
use std::io::{Read, Write};
use std::net::{SocketAddrV4, TcpListener};
use std::time::Duration;
use router::socks::{self, SocksError};

/// A one-connection proxy that answers the greeting with `hello` and the request with `reply`.
fn proxy(hello: &'static [u8], reply: Option<&'static [u8]>) -> (SocketAddrV4, std::thread::JoinHandle<Vec<u8>>) { /* reads 3 bytes, writes hello, reads 10, writes reply, returns what it read */ }

const DST: &str = "35.217.1.2:50005";
const SHORT: Duration = Duration::from_millis(300);

#[test]
fn udp_associate_names_the_destination_and_returns_the_relay() {
    let (addr, server) = proxy(&[5, 0], Some(&[5, 0, 0, 1, 0, 0, 0, 0, 0x1F, 0x90]));
    let assoc = socks::udp_associate(addr, DST.parse().unwrap(), Duration::from_secs(2)).unwrap();
    assert_eq!(assoc.relay, SocketAddrV4::new(*addr.ip(), 8080));
    assert_eq!(server.join().unwrap(), [5, 1, 0, 5, 3, 0, 1, 35, 217, 1, 2, 0xC3, 0x55]);
}

#[test]
fn a_proxy_that_wants_a_password_is_named() {
    for hello in [&[5u8, 2][..], &[5, 0xFF]] {
        let (addr, _server) = proxy(hello, None);
        assert!(matches!(socks::udp_associate(addr, DST.parse().unwrap(), SHORT), Err(SocksError::AuthRequired)));
    }
}

#[test]
fn a_refused_associate_carries_its_code() {
    let (addr, _server) = proxy(&[5, 0], Some(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]));
    let err = socks::udp_associate(addr, DST.parse().unwrap(), SHORT).unwrap_err();
    assert!(matches!(err, SocksError::Refused(7)));
    assert!(err.to_string().contains("does not support UDP"));
}

#[test]
fn a_silent_proxy_times_out() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = match listener.local_addr().unwrap() { std::net::SocketAddr::V4(a) => a, _ => unreachable!() };
    let _hold = std::thread::spawn(move || { let _conn = listener.accept(); std::thread::sleep(Duration::from_secs(2)); });
    let started = std::time::Instant::now();
    assert!(matches!(socks::udp_associate(addr, DST.parse().unwrap(), SHORT), Err(SocksError::Timeout)));
    assert!(started.elapsed() < Duration::from_secs(1));
}

#[test]
fn a_dropped_connection_and_a_closed_port_are_io_errors() {
    let (addr, _server) = proxy(&[5, 0], Some(&[]));
    assert!(matches!(socks::udp_associate(addr, DST.parse().unwrap(), SHORT), Err(SocksError::Io(_))));
    let closed = { let l = TcpListener::bind("127.0.0.1:0").unwrap(); l.local_addr().unwrap() };
    let closed = match closed { std::net::SocketAddr::V4(a) => a, _ => unreachable!() };
    assert!(matches!(socks::check(closed, SHORT), Err(SocksError::Io(_))));
}

#[test]
fn check_greets_and_measures() {
    let (addr, _server) = proxy(&[5, 0], None);
    assert!(socks::check(addr, Duration::from_secs(2)).unwrap() < Duration::from_secs(2));
}
```

(Тело `proxy` пишет исполнитель: принять одно соединение, прочитать 3 байта, записать `hello`, при `reply` — прочитать 10 байт, записать `reply` (пустой — закрыть), вернуть прочитанное.)

- [ ] **Шаг 2: Убедиться, что падают** — `cargo test -p router --test socks`: ошибки компиляции.
- [ ] **Шаг 3: Реализовать** `socks.rs` по Interfaces.
- [ ] **Шаг 4: Тесты проходят** — PASS.
- [ ] **Шаг 5: Коммит** — `feat(router): a SOCKS5 client for UDP ASSOCIATE and proxy checks`

---

### Задача 4: решения по потокам, очередь, PID, счётчики

**Files:**
- Create: `crates/router/src/owner.rs`, `crates/router/src/flows.rs`; Modify: `crates/router/src/lib.rs`
- Test: `crates/router/tests/flows.rs`

**Interfaces:**
- Consumes: `socks::SocksError`.
- Produces:
  - `owner::PID_WAIT: Duration = 50 ms`; `struct owner::Owners` (`Mutex<HashMap<u16, u32>>` + `Condvar`; `Default`): `record(&self, local_port: u16, pid: u32)`, `forget(&self, local_port: u16)`, `wait(&self, local_port: u16, timeout: Duration) -> Option<u32>`;
  - `owner::port_of(dw_port: u32) -> u16` — порт строки `MIB_TCPROW_OWNER_PID` (младшие 16 бит, сетевой порядок); `owner::listener_pid(port: u16) -> Option<u32>` — `GetExtendedTcpTable(…, AF_INET = 2, TCP_TABLE_OWNER_PID_LISTENER)`, буфер по размеру из первого вызова; свой PID — `std::process::id()`;
  - DTO (`Serialize, Deserialize, specta::Type`, `Debug, Clone, PartialEq`):
    - `#[serde(tag = "state")] enum flows::ProxyState { Unknown, Ok { latency_ms: u32 }, Down { error: String }, AuthRequired, NoUdp }`;
    - `#[serde(tag = "kind")] enum flows::DirectReason { ProxyDown, AuthRequired, NoUdp, Timeout, Failed { error: String }, Loop }`; `impl From<&SocksError> for DirectReason` (`Timeout → Timeout`, `AuthRequired → AuthRequired`, `Refused(7) → NoUdp`, прочее → `Failed { error: err.to_string() }`);
    - `#[serde(tag = "via")] enum flows::Via { Opening, Proxy, Direct { reason: DirectReason } }`;
    - `struct flows::Traffic { flows: u32, up: f64, down: f64 }` (байты; `Default`), `struct flows::Totals { proxied: Traffic, direct: Traffic }`;
    - `struct flows::FlowRow { protocol: String /* "udp" */, destination: String, local_port: u16, rule: Option<String>, via: Via, up: f64, down: f64, seconds: u32, closed: bool }`;
  - `flows::QUEUE: usize = 64`, `flows::IDLE: Duration = 30 s`, `flows::KEEP_CLOSED: Duration = 30 s`;
  - `struct flows::Pending` (`VecDeque<(Vec<u8>, Address)>`; `Default`): `push(&mut self, packet: Vec<u8>, addr: Address)` (сверх 64 — выбрасывается самый старый), `len(&self) -> usize`, `drain(&mut self) -> Vec<(Vec<u8>, Address)>`;
  - `flows::decide(proxy: &ProxyState, pid: Option<u32>, own: u32, proxy_pid: Option<u32>) -> Option<DirectReason>` — `None` = через прокси: PID свой или прокси → `Loop`; `Down` → `ProxyDown`; `AuthRequired` → `AuthRequired`; `Unknown`, `Ok`, `NoUdp` → `None` (отказ UDP узнаётся по самому `UDP ASSOCIATE`);
  - `struct flows::FlowKey { pub local_port: u16, pub dst: SocketAddrV4 }` (`Hash, Eq, Copy`);
  - `struct flows::Stats` (`Default`): `open(&mut self, key: FlowKey, rule: &str, via: Via, now: Instant)`, `set_via(&mut self, key: FlowKey, via: Via)`, `add(&mut self, key: FlowKey, up: u64, down: u64, now: Instant)`, `close(&mut self, key: FlowKey, now: Instant)`, `idle(&self, now: Instant) -> Vec<FlowKey>` (без пакетов ≥ `IDLE`), `rows(&mut self, now: Instant, all: bool) -> Vec<FlowRow>` (открытые и закрытые моложе `KEEP_CLOSED`, старые выбрасываются; `all = false` — только с `rule`; в этапе 1 правило есть у каждого потока, фильтр нужен этапу 3), `totals(&self) -> Totals`, `rule_traffic(&self, rule: &str) -> Traffic`. Поток считается в `proxied`, если его `Via` в момент `open`/`set_via` — `Proxy` или `Opening`, иначе в `direct`; `set_via` на `Direct` переносит его счёт.

- [ ] **Шаг 1: Падающие тесты** — `crates/router/tests/flows.rs`:

```rust
use std::time::{Duration, Instant};
use router::divert::Address;
use router::flows::{self, DirectReason, FlowKey, Pending, ProxyState, Stats, Via};
use router::owner::{self, Owners, PID_WAIT};

#[test]
fn the_queue_keeps_the_newest_64() {
    let mut queue = Pending::default();
    for i in 0..70u8 {
        queue.push(vec![i], Address([0; 80]));
    }
    let left = queue.drain();
    assert_eq!(left.len(), 64);
    assert_eq!((left[0].0[0], left[63].0[0]), (6, 69));
    assert_eq!(queue.len(), 0);
}

#[test]
fn new_flows_follow_the_proxy_state() {
    let ok = ProxyState::Ok { latency_ms: 3 };
    assert_eq!(flows::decide(&ok, Some(500), 1, Some(2)), None);
    assert_eq!(flows::decide(&ProxyState::Unknown, None, 1, None), None);
    assert_eq!(flows::decide(&ProxyState::NoUdp, None, 1, None), None, "asked again per flow");
    assert_eq!(flows::decide(&ProxyState::Down { error: "refused".into() }, None, 1, None), Some(DirectReason::ProxyDown));
    assert_eq!(flows::decide(&ProxyState::AuthRequired, None, 1, None), Some(DirectReason::AuthRequired));
}

#[test]
fn traffic_of_the_proxy_and_of_the_hub_goes_direct() {
    let ok = ProxyState::Ok { latency_ms: 3 };
    assert_eq!(flows::decide(&ok, Some(2), 1, Some(2)), Some(DirectReason::Loop));
    assert_eq!(flows::decide(&ok, Some(1), 1, Some(2)), Some(DirectReason::Loop));
}

#[test]
fn socks_errors_become_reasons() {
    use router::socks::SocksError;
    assert_eq!(DirectReason::from(&SocksError::Refused(7)), DirectReason::NoUdp);
    assert_eq!(DirectReason::from(&SocksError::Timeout), DirectReason::Timeout);
    assert!(matches!(DirectReason::from(&SocksError::Refused(1)), DirectReason::Failed { .. }));
}

#[test]
fn a_late_socket_event_is_waited_for_50_ms() {
    let owners = std::sync::Arc::new(Owners::default());
    let late = owners.clone();
    let writer = std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(10)); late.record(50001, 77); });
    assert_eq!(owners.wait(50001, PID_WAIT), Some(77));
    writer.join().unwrap();
    let started = Instant::now();
    assert_eq!(owners.wait(50002, PID_WAIT), None);
    assert!(started.elapsed() >= PID_WAIT && started.elapsed() < Duration::from_millis(500));
    owners.forget(50001);
    assert_eq!(owners.wait(50001, Duration::ZERO), None);
}

#[test]
fn the_listener_of_a_port_is_found() {
    assert_eq!(owner::port_of(u32::from(2080u16.to_be())), 2080);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    assert_eq!(owner::listener_pid(port), Some(std::process::id()));
    drop(listener);
    assert_eq!(owner::listener_pid(port), None);
}

#[test]
fn closed_flows_stay_30_seconds_and_traffic_adds_up() {
    let t0 = Instant::now();
    let key = |port| FlowKey { local_port: port, dst: "104.29.1.1:50001".parse().unwrap() };
    let mut stats = Stats::default();
    stats.open(key(1), "voice", Via::Opening, t0);
    stats.add(key(1), 100, 0, t0);
    stats.set_via(key(1), Via::Proxy);
    stats.add(key(1), 0, 50, t0);
    stats.open(key(2), "voice", Via::Direct { reason: DirectReason::ProxyDown }, t0);
    stats.add(key(2), 7, 0, t0);
    let totals = stats.totals();
    assert_eq!((totals.proxied.flows, totals.proxied.up, totals.proxied.down), (1, 100.0, 50.0));
    assert_eq!((totals.direct.flows, totals.direct.up), (1, 7.0));
    assert_eq!(stats.rule_traffic("voice").up, 107.0);
    assert_eq!(stats.idle(t0 + Duration::from_secs(31)).len(), 2);
    stats.close(key(1), t0 + Duration::from_secs(1));
    let rows = stats.rows(t0 + Duration::from_secs(30), true);
    assert!(rows.iter().any(|r| r.local_port == 1 && r.closed));
    assert!(stats.rows(t0 + Duration::from_secs(32), true).iter().all(|r| r.local_port != 1));
    assert_eq!(rows[0].destination, "104.29.1.1:50001");
}
```

- [ ] **Шаг 2: Убедиться, что падают** — `cargo test -p router --test flows`: ошибки компиляции.
- [ ] **Шаг 3: Реализовать** `owner.rs` и `flows.rs` по Interfaces. `listener_pid` — единственный `unsafe` задачи; комментарий `// SAFETY:` о размере буфера.
- [ ] **Шаг 4: Тесты проходят** — PASS.
- [ ] **Шаг 5: Коммит** — `feat(router): flow decisions, the pending queue, socket owners and counters`

---

### Задача 5: WinDivert — загрузка DLL и дескрипторы

**Files:**
- Modify: `crates/router/src/divert.rs` (`Dll`, `Handle`, `Layer`)
- Test: `crates/router/tests/live.rs` (все `#[ignore]`)

**Interfaces:**
- Produces:
  - `struct divert::Dll` (`Clone`, указатели функций `unsafe extern "C" fn`, сигнатуры WinDivert 2.2: `Open(*const c_char, i32, i16, u64) -> *mut c_void`, `Recv(*mut c_void, *mut c_void, u32, *mut u32, *mut Address) -> i32`, `Send(*mut c_void, *const c_void, u32, *mut u32, *const Address) -> i32`, `Shutdown(*mut c_void, i32) -> i32`, `Close(*mut c_void) -> i32`, `CalcChecksums(*mut c_void, u32, *mut Address, u64) -> i32`);
  - `Dll::load(path: &Path) -> anyhow::Result<Dll>` — `LoadLibraryW(&HSTRING::from(path))`, затем `GetProcAddress` по каждому имени; ошибки `cannot load {path}: {windows error}`, `{path} has no {name}: is it WinDivert 2.x?`; один `Dll` на путь в процессе (`OnceLock<Mutex<HashMap<PathBuf, Dll>>>`);
  - `Dll::checksums(&self, packet: &mut [u8], addr: &mut Address)`;
  - `enum divert::Layer { Network = 0, Socket = 3 }`; `const PRIORITY: i16 = 1000`; `const FLAG_SNIFF: u64 = 0x1`, `FLAG_RECV_ONLY: u64 = 0x4`;
  - `struct divert::Handle` (`Send + Sync`): `Handle::open(dll: &Dll, filter: &str, layer: Layer, flags: u64) -> std::io::Result<Handle>` (`INVALID_HANDLE_VALUE` (−1) → `std::io::Error::last_os_error()` — текст ошибки Windows для «недоступна»); `recv(&self, buf: &mut [u8]) -> std::io::Result<(usize, Address)>` (для слоя SOCKET буфер пустой); `send(&self, packet: &[u8], addr: &Address) -> std::io::Result<()>`; `shutdown(&self)` (`WINDIVERT_SHUTDOWN_BOTH = 3`; ждущий `recv` возвращает ошибку); `Drop` → `WinDivertClose`.

- [ ] **Шаг 1: Тесты** — `crates/router/tests/live.rs`; каждый `#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL set"]`, DLL — `std::env::var("WINDIVERT_DLL")`:

```rust
#[test]
#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL set"]
fn a_network_handle_captures_and_reinjects_a_packet() {
    // filter "outbound and udp and ip.DstAddr == 192.0.2.1 and udp.DstPort == 9" (TEST-NET, nobody answers);
    // a UdpSocket on 0.0.0.0:0 sends b"zapret-hub" to 192.0.2.1:9;
    // handle.recv → parse_udp(payload) == b"zapret-hub", addr.outbound(); handle.send(packet, &addr) is Ok
}

#[test]
#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL set"]
fn the_socket_handle_names_this_process() {
    // Handle::open(SOCKET_FILTER, Layer::Socket, FLAG_SNIFF | FLAG_RECV_ONLY); bind and send as above;
    // within 2 s an event with socket_pid() == std::process::id() and socket_local_port() == the socket's port
}

#[test]
#[ignore = "opens WinDivert: run elevated with WINDIVERT_DLL set"]
fn shutdown_ends_a_waiting_recv() {
    // a handle with filter "false"; recv on another thread; shutdown(); the thread returns Err within 1 s
}
```

И не-`ignore` тест в `crates/router/tests/filter.rs` (только читает систему):

```rust
#[test]
fn a_missing_dll_is_named_with_the_windows_error() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("WinDivert.dll");
    let err = format!("{:#}", router::divert::Dll::load(&path).unwrap_err());
    assert!(err.contains("WinDivert.dll") && err.contains("cannot load"), "{err}");
}
```

- [ ] **Шаг 2: Убедиться, что падают** — `cargo test -p router --test filter --test live`: ошибки компиляции.
- [ ] **Шаг 3: Реализовать** `Dll`, `Layer`, `Handle` по Interfaces; `// SAFETY:` у каждого `unsafe`.
- [ ] **Шаг 4: Проверка** — `cargo test -p router --test filter --test live`: PASS, 3 `ignored`. Из повышенного терминала с `WINDIVERT_DLL=E:\Zapret\bin\WinDivert.dll` (при остановленной службе zapret — тот же драйвер, ничего не ставится): `cargo test -p router --test live -- --ignored` — PASS. Если повышенного терминала нет, отметить в отчёте «live не запускались».
- [ ] **Шаг 5: Коммит** — `feat(router): load WinDivert.dll from the engine folder and open handles`

---

### Задача 6: `Router` — перехват, реле, сокеты, обслуживание, горячая перезагрузка

**Files:**
- Modify: `crates/router/src/lib.rs`
- Test: `crates/router/tests/router.rs`

**Interfaces:**
- Consumes: всё из задач 1–5.
- Produces (`router`):
  - `struct Setup { pub dll: PathBuf, pub proxy: SocketAddrV4 }` (`Debug, Clone, PartialEq`);
  - `fn parse_proxy(socks5: Option<&str>) -> anyhow::Result<SocketAddrV4>` — `None`/пусто → `set the SOCKS5 proxy in Settings first`; не IPv4 → `the router needs an IPv4 SOCKS5 address like 127.0.0.1:2080: {text}`;
  - DTO (`specta::Type`): `#[serde(tag = "state")] enum RouterState { Off, Running, Paused, Unavailable { reason: String } }`; `struct RouterStatus { state: RouterState, proxy: ProxyState, totals: Totals, rules_error: Option<String> }`; `struct RuleRow { rule: Rule, traffic: Traffic }`; `struct RoutesView { rules: Vec<RuleRow>, error: Option<String> }`;
  - `struct Router` (`Clone` через `Arc<Inner>`, `Send + Sync`):
    - `Router::new(routes: PathBuf) -> Router` — правила читаются сразу;
    - `on_change(&self, f: impl Fn() + Send + Sync + 'static)` — зовётся без удержания блокировок `Router`, когда меняется `RouterState` или `ProxyState`;
    - `start(&self, setup: Setup) -> anyhow::Result<()>` — тот же `Setup` при `Running` ничего не делает; другой — перезапуск; при паузе только запоминает `Setup`; ошибка (`Dll::load`, `Handle::open`) → `Unavailable { reason: format!("{err:#}") }` и `Err`; счётчики обнуляются при каждом запуске;
    - `fail(&self, reason: String)` — включена, но не запускается (нет прокси, нет DLL): дескрипторы закрыты, `Unavailable`;
    - `stop(&self)` — `Off`, всё закрыто, пауза снята;
    - `pause(&self)` — при `Running` дескрипторы и потоки закрываются, `Paused`; иначе ничего;
    - `resume(&self)` — при `Paused` запуск с запомненным `Setup` (ошибка → `Unavailable`); иначе ничего;
    - `status(&self) -> RouterStatus`; `rules(&self) -> RoutesView` (перечитывает файл, если изменилось время записи; битый — прежние правила + `error`); `flows(&self, all: bool) -> Vec<FlowRow>`;
    - `edit<T>(&self, f: impl FnOnce(&mut Vec<Rule>) -> anyhow::Result<T>) -> anyhow::Result<T>` — файл с ошибкой → `routes.toml has an error, fix or delete it first: {error}` и файл не тронут; после `f` — `rules::check` каждого правила (первая ошибка → `Err`, файл не тронут), `rules::save`, сразу новая таблица и фильтр.

Потоки при `Running` (тело — исполнителя; порядок решений фиксирован):

```text
capture (one per capture handle):
  loop { (packet, addr) = handle.recv()?  // error after shutdown → exit
    udp = parse_udp(packet) else { handle.send(packet, addr); continue }
    rule = table.match_udp(udp.dst) else { handle.send(packet, addr); continue }   // port-only filter
    key = FlowKey { local_port: udp.src.port(), dst: udp.dst }
    match flows[key] {
      none    => pid = owners.wait(key.local_port, PID_WAIT);
                 match decide(proxy_state, pid, std::process::id(), proxy_pid) {
                   Some(reason) => Direct(reason); stats.open(..Direct); handle.send(packet, addr)
                   None => Opening(Pending[packet]); stats.open(..Opening); spawn opener(key, udp.src, addr.as_inbound())
                 }
      Opening => pending.push(packet, addr)
      Proxy   => relay_socket.send_to(socks_header(dst) ++ payload, relay); stats.add(up)
      Direct  => handle.send(packet, addr); stats.add(up)
    } }

opener(key, local, inbound_addr):
  udp_associate(proxy, key.dst, OPEN_TIMEOUT)
    Ok(assoc) => socket = UdpSocket::bind("127.0.0.1:0"), read timeout 1 s; flow = Proxy{assoc, socket, inbound_addr, local};
                 send every pending packet through the relay; proxy_state NoUdp → Ok; spawn relay_rx
    Err(e)    => flow = Direct(DirectReason::from(&e)); stats.set_via; handle.send every pending (packet, addr) unchanged;
                 AuthRequired / Refused(7) also set proxy_state (AuthRequired / NoUdp) → on_change

relay_rx: loop { recv_from with 1 s timeout; flow gone → exit
    from != relay → skip; (src, data) = parse_socks_udp(datagram) else skip
    packet = build_udp(src, local, data, next_id); addr = inbound_addr; dll.checksums(packet, addr); handle.send; stats.add(down) }

sockets (SOCKET handle, SOCKET_FILTER, FLAG_SNIFF | FLAG_RECV_ONLY):
  event == EVENT_SOCKET_CLOSE → owners.forget(port) else owners.record(port, pid)

housekeeping, every 1 s: close stats.idle(now) flows (drop control + socket); every 2 s: rules mtime → reload;
  every 10 s and right at start: socks::check(proxy, OPEN_TIMEOUT) → ProxyState (Ok/Down/AuthRequired;
  NoUdp stays until an associate succeeds), proxy_pid = owner::listener_pid(proxy.port()); change → on_change.

reload (rules changed): table = Table::new(rules); text = divert::filter(&table);
  at most once a second (sleep the rest); None → shut the capture handle; Some → open new, spawn its capture,
  swap it in as the injection handle, then shutdown + drop the old one. Open fails → keep the old handle,
  rules_error = "WinDivert refused the filter: {os error}".
```

Пометить `// ponytail:`: ожидание PID блокирует поток перехвата до 50 мс на новый поток (ceiling: потоки редки; апгрейд — отложенная очередь); обрыв управляющего TCP после открытия не отслеживается — поток закроется по простою.

- [ ] **Шаг 1: Падающие тесты** — `crates/router/tests/router.rs` (ничего не открывает в WinDivert):

```rust
use router::rules::{self, Preset};
use router::{Router, RouterState, Setup, parse_proxy};

fn router(dir: &std::path::Path) -> Router {
    Router::new(dir.join("routes.toml"))
}

#[test]
fn a_new_router_is_off_and_pause_stop_resume_are_harmless() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    r.pause();
    r.resume();
    r.stop();
    assert_eq!(r.status().state, RouterState::Off);
    assert!(r.flows(true).is_empty());
}

#[test]
fn a_missing_dll_makes_it_unavailable_with_the_reason() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    let setup = Setup { dll: dir.path().join("WinDivert.dll"), proxy: "127.0.0.1:2080".parse().unwrap() };
    assert!(r.start(setup).is_err());
    match r.status().state {
        RouterState::Unavailable { reason } => assert!(reason.contains("WinDivert.dll"), "{reason}"),
        other => panic!("{other:?}"),
    }
    r.fail("set the SOCKS5 proxy in Settings first".into());
    assert!(matches!(r.status().state, RouterState::Unavailable { reason } if reason.contains("SOCKS5")));
    r.stop();
    assert_eq!(r.status().state, RouterState::Off);
}

#[test]
fn an_edit_writes_the_file_and_the_view() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    r.edit(|list| rules::add(list, rules::preset(Preset::DiscordVoice))).unwrap();
    assert_eq!(r.rules().rules[0].rule.name, "Discord голос");
    assert_eq!(rules::load(&dir.path().join("routes.toml")).unwrap().len(), 1);
    let bad = r.edit(|list| { list[0].ports = vec!["0".into()]; Ok(()) });
    assert!(bad.is_err());
    assert_eq!(rules::load(&dir.path().join("routes.toml")).unwrap()[0].ports.len(), 2, "a failed edit writes nothing");
}

#[test]
fn rules_are_read_again_when_the_file_changes() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    let r = router(dir.path());
    assert!(r.rules().rules.is_empty());
    rules::save(&path, &[rules::preset(Preset::DiscordVoice)]).unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    assert_eq!(r.rules().rules.len(), 1);
}

#[test]
fn a_broken_file_keeps_the_rules_and_shows_where() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    rules::save(&path, &[rules::preset(Preset::DiscordVoice)]).unwrap();
    let r = router(dir.path());
    std::fs::write(&path, "[[rule]]\nname = \"x\"\nprotocol = \"udp\"\nips = [\"300.0.0.1\"]\n").unwrap();
    let later = std::time::SystemTime::now() + std::time::Duration::from_secs(5);
    std::fs::File::options().write(true).open(&path).unwrap().set_modified(later).unwrap();
    let view = r.rules();
    assert_eq!(view.rules[0].rule.name, "Discord голос");
    assert!(view.error.as_deref().unwrap().starts_with("line 4: ips:"), "{view:?}");
    assert_eq!(r.status().rules_error, view.error);
}

#[test]
fn an_edit_never_overwrites_a_broken_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("routes.toml");
    std::fs::write(&path, "[[rule]\n").unwrap();
    let r = router(dir.path());
    let err = r.edit(|list| rules::add(list, rules::preset(Preset::DiscordVoice))).unwrap_err();
    assert!(format!("{err:#}").contains("routes.toml has an error"), "{err:#}");
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "[[rule]\n");
}

#[test]
fn the_proxy_is_an_ipv4_address_from_the_settings() {
    assert_eq!(parse_proxy(Some("127.0.0.1:2080")).unwrap(), "127.0.0.1:2080".parse().unwrap());
    assert!(format!("{:#}", parse_proxy(None).unwrap_err()).contains("SOCKS5"));
    assert!(format!("{:#}", parse_proxy(Some(" ")).unwrap_err()).contains("SOCKS5"));
    assert!(format!("{:#}", parse_proxy(Some("[::1]:2080")).unwrap_err()).contains("IPv4"));
}
```

И в `crates/router/tests/live.rs` (`#[ignore]`, `WINDIVERT_DLL` и `ROUTER_SOCKS5`=рабочий NekoBox): `a_running_router_relays_a_flow_through_the_proxy` — правило на `1.1.1.1/32` порт `53`, `start`, UDP-запрос DNS к `1.1.1.1:53` из теста получает ответ, `flows(false)` — строка с `via: Proxy` и `down > 0`; `stop`.

- [ ] **Шаг 2: Убедиться, что падают** — `cargo test -p router --test router`: ошибки компиляции.
- [ ] **Шаг 3: Реализовать** `Router` в `lib.rs` по Interfaces и схеме потоков.
- [ ] **Шаг 4: Тесты проходят** — `cargo test -p router`: PASS, `ignored` — 4. Живой — как в задаче 5, если есть повышенный терминал.
- [ ] **Шаг 5: Коммит** — `feat(router): relay UDP flows through SOCKS5 with hot-reloaded rules`

---

### Задача 7: приложение — настройка, команды, старт, тесты, смена движка, трей

**Files:**
- Modify: `src-tauri/Cargo.toml` (`router.workspace = true`), `src-tauri/src/settings.rs`, `src-tauri/src/state.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/actions.rs`, `src-tauri/src/jobs.rs`, `src-tauri/src/tray.rs`, `src-tauri/src/texts.rs`
- Create: `src-tauri/src/routing.rs`
- Generate: `frontend/src/lib/ipc/bindings.ts`

**Interfaces:**
- Consumes: `router::{Router, Setup, parse_proxy, RouterStatus, RouterState, RoutesView, divert::{pick_dll, Engine}, rules, flows::{FlowRow, ProxyState}}`; `zapret::procs::is_running`, `zapret::service::{WINWS2, SERVICE2, active}`, `AppState::engine2`.
- Produces:
  - `Settings.router_enabled: bool` (по умолчанию `false`); `settings::to_save` всегда возвращает `router_enabled` текущих настроек (окно и pipe);
  - `Paths::routes(&self) -> PathBuf` = `data\routes.toml`; `AppState.router: router::Router` (`pub`), создаётся в `AppState::new`;
  - `routing::setup(state: &AppState) -> anyhow::Result<Setup>` — `parse_proxy(settings.socks5)`, `pick_dll(running, zapret_dir, engine2.exe().parent(), Path::is_file)`; `running` — `winws2.exe` запущен → `Zapret2`, иначе `winws.exe` → `Flowseal`, иначе `None`;
  - `routing::sync(app: &AppHandle)` — `router_enabled`: `setup` → `router.start` (ошибка уже в статусе) или `router.fail(reason)`; выключено → `router.stop()`;
  - `routing::set_enabled(app: &AppHandle, enabled: bool) -> anyhow::Result<RouterStatus>` — под `state.saving()`: при `true` сначала `setup()?` и `router.start(..)?` (ошибка — настройка не пишется, спецификация 6: «маршрутизация не включается»), затем `settings::save` с новым флагом и `set_settings`; при `false` — `stop` и запись; затем `actions::changed`;
  - `routing::paused<T>(app: &AppHandle, f: impl FnOnce() -> T) -> T` — `pause`, `f`, `sync`, `resume`;
  - `routing::switches_engine(name: &str, service: &str) -> bool` — стратегия `zapret2:` при службе не `zapret2`, или наоборот;
  - команды (`#[tauri::command] #[specta::specta]`, тела — вызов `routing`/`state.router`, `blocking` где трогают диск или систему): `set_router_enabled(handle, enabled: bool) -> Answer<RouterStatus>`, `router_status(state) -> RouterStatus`, `list_routes(state) -> RoutesView`, `get_route(state, name: String) -> Answer<Rule>`, `save_route(handle, rule: Rule, create: bool) -> Answer<()>` (`router.edit(add | replace)`), `set_route_enabled(handle, name: String, enabled: bool) -> Answer<()>`, `delete_route(handle, name: String) -> Answer<()>`, `add_route_preset(handle, preset: Preset) -> Answer<Rule>`, `route_flows(state, all: bool) -> Vec<FlowRow>`, `validate_route(rule: Rule) -> Vec<FieldError>`, `import_route_lines(ips: Vec<String>, text: String) -> Answer<Vec<String>>`; все в `collect_commands!` (`lib.rs`); изменяющие зовут `actions::changed`;
  - `tray::proxy_mark(status: &RouterStatus) -> bool` — `Running` и прокси `Down`, `AuthRequired` или `NoUdp`; `tray::pixels(light: Light, mark: bool) -> Vec<u8>` — при `mark` поверх диска круг центр `(25.0, 25.0)` (центры пикселей `+0.5`), радиус `6.0`, цвет `[0xE5, 0x48, 0x4D]`, тот же мягкий край;
  - `texts::ROUTER_ON: &str = "Маршрутизация: вкл"`, `texts::ROUTER_OFF: &str = "Маршрутизация: выкл"`, `texts::ROUTER_FAILED: &str = "Маршрутизация не включилась"`, `texts::PROXY_DOWN: &str = "Прокси недоступен, трафик идёт напрямую"`; `texts::tooltip(light, strategy, proxy_down: bool)` добавляет строку `PROXY_DOWN`.

Места вызова:
- `lib.rs` `setup`: после `tray::create` — `state.router.on_change(move || actions::changed(&handle))`.
- `jobs::startup`: после `recover` — `routing::sync(app)`; `Unavailable` при `router_enabled` → `actions::notice(Error, texts::ROUTER_FAILED, reason)`.
- `jobs::start_tests`: в потоке прогона блок `let _turn …; tester::run::run(…)` обёрнут в `routing::paused(&handle, || …)` (спецификация 7: пауза на прогон).
- `actions::apply`: при `routing::switches_engine(name, active().0)` вся смена — внутри `routing::paused` (DLL нового движка, спецификация 3.1, 7); иначе как было.
- `actions::save_settings`: в конце — `routing::sync(app)` (прокси или папка могли смениться).
- `tray::menu`: пункт `MenuItem::with_id(app, "router", ROUTER_ON | ROUTER_OFF, true, None)` после «Быстрая проверка»; `on_menu("router")` → в отдельном потоке `routing::set_enabled(app, !enabled)`, ошибка → `notify::show` + `actions::notice(Error, ROUTER_FAILED, …)`; `refresh` берёт `state.router.status()` для `pixels(light, proxy_mark(..))` и подсказки.

- [ ] **Шаг 1: Падающие тесты** (`#[cfg(test)]` модулей)

`src-tauri/src/settings.rs`: в `settings_survive_a_save` — `router_enabled: true` в литерале; в `a_missing_file_gives_the_defaults` — `assert!(!Settings::default().router_enabled);` и новый:

```rust
#[test]
fn a_settings_save_keeps_router_enabled() {
    let on = Settings { router_enabled: true, ..Settings::default() };
    let stale = Settings { theme: Theme::Dark, ..Settings::default() };
    for pipe in [false, true] {
        let saved = to_save(&on, stale.clone(), pipe).unwrap();
        assert!(saved.router_enabled, "only set_router_enabled changes it (pipe: {pipe})");
        assert_eq!(saved.theme, Theme::Dark);
        assert!(!to_save(&Settings::default(), on.clone(), pipe).unwrap().router_enabled);
    }
}
```

`src-tauri/src/tray.rs`: вызовы `pixels(x)` в тестах → `pixels(x, false)`; новые:

```rust
#[test]
fn the_icon_marks_a_proxy_that_is_down() {
    let at = |rgba: &[u8], x: usize, y: usize| rgba[(y * 32 + x) * 4..][..4].to_vec();
    assert_eq!(at(&pixels(Light::Green, true), 25, 25), [0xE5, 0x48, 0x4D, 255]);
    assert_eq!(at(&pixels(Light::Green, false), 25, 25), [0x2E, 0xB8, 0x5C, 255]);
    assert_eq!(at(&pixels(Light::Green, true), 16, 16), [0x2E, 0xB8, 0x5C, 255], "the disc stays");
}

#[test]
fn only_a_running_router_with_a_failing_proxy_is_marked() {
    use router::flows::{ProxyState, Totals};
    use router::{RouterState, RouterStatus};
    let status = |state, proxy| RouterStatus { state, proxy, totals: Totals::default(), rules_error: None };
    let down = || ProxyState::Down { error: "refused".into() };
    assert!(proxy_mark(&status(RouterState::Running, down())));
    assert!(proxy_mark(&status(RouterState::Running, ProxyState::NoUdp)));
    assert!(!proxy_mark(&status(RouterState::Running, ProxyState::Ok { latency_ms: 2 })));
    assert!(!proxy_mark(&status(RouterState::Off, down())));
    assert!(!proxy_mark(&status(RouterState::Paused, down())));
}
```

(`Totals` и `Traffic` — `#[derive(Default)]`, задача 4.)

`src-tauri/src/routing.rs`:

```rust
#[test]
fn only_a_strategy_of_the_other_engine_switches_it() {
    assert!(switches_engine("zapret2:mine", "zapret"));
    assert!(switches_engine("general (ALT11)", "zapret2"));
    assert!(!switches_engine("general (ALT11)", "zapret"));
    assert!(!switches_engine("zapret2:general", "zapret2"));
}
```

- [ ] **Шаг 2: Убедиться, что падают** — `cargo test -p zapret-hub --lib`: ошибки компиляции.
- [ ] **Шаг 3: Реализовать** по Interfaces и «Местам вызова».
- [ ] **Шаг 4: Проверка** — `cargo test -p zapret-hub --lib`: PASS; `cargo run -p zapret-hub --bin export-bindings` — в `bindings.ts` 11 новых команд и типы `Rule`, `RouterStatus`, `FlowRow`, `RoutesView`, `Preset`, `FieldError`; `npm --prefix frontend run check` — 0/0 (в `Settings` все поля необязательны, `#[serde(default)]`).
- [ ] **Шаг 5: Коммит** — `git add src-tauri frontend/src/lib/ipc/bindings.ts Cargo.lock` → `feat(app): routing commands, router_enabled, a pause for test runs and a tray item`

---

### Задача 8: CLI — `router` и `routes`

**Files:**
- Modify: `crates/cli/Cargo.toml` (`router.workspace = true`), `crates/cli/src/lib.rs`, `crates/cli/src/main.rs`, `src-tauri/src/cli.rs`

**Interfaces:**
- Consumes: команды задачи 7 (`bindings.ts`), `router::rules::{Rule, Preset, parse_rule_file}`.
- Produces:
  - `Request::{SetRouterEnabled { enabled: bool }, RouterStatus, ListRoutes, GetRoute { name: String }, SaveRoute { rule: Rule, create: bool }, SetRouteEnabled { name: String, enabled: bool }, DeleteRoute { name: String }, AddRoutePreset { preset: Preset }, RouteFlows { all: bool }}`; `WINDOW_ONLY` += `"validate_route"`, `"import_route_lines"` (`[&str; 16]`); `samples()` — по одному новому;
  - `src-tauri/src/cli.rs::dispatch` — по ветке на каждый, вызов той же команды окна;
  - clap: `Command::Router(RouterCommand { On, Off, Status })`, `Command::Routes(RoutesCommand { List, Show { name }, Add { name, #[arg(long)] file: PathBuf }, Edit { name, #[arg(long)] file: PathBuf }, Enable { name }, Disable { name }, Remove { name }, Preset { preset: PresetArg }, Flows { #[arg(long)] all: bool } })`; `PresetArg` — `ValueEnum` с одним значением `discord-voice` → `Preset::DiscordVoice`;
  - `routes add|edit` читают файл `read_text(&file)` (`-` — stdin) и `parse_rule_file(&name, &text)`; ошибка → `Fail::new(USAGE, err.to_string())`, ничего не отправляется; `add` → `create: true`, `edit` → `create: false`;
  - `needs_yes`: `Command::Routes(RoutesCommand::Remove { .. })` (окно спрашивает подтверждение, спецификация 8).

- [ ] **Шаг 1: Падающие тесты**

`crates/cli/src/main.rs`, таблица `every_command_sends_its_window_command` — строки:

```rust
(&["router", "on"], Request::SetRouterEnabled { enabled: true }),
(&["router", "off"], Request::SetRouterEnabled { enabled: false }),
(&["router", "status"], Request::RouterStatus),
(&["routes", "list"], Request::ListRoutes),
(&["routes", "show", "Discord голос"], Request::GetRoute { name: "Discord голос".into() }),
(&["routes", "enable", "v"], Request::SetRouteEnabled { name: "v".into(), enabled: true }),
(&["routes", "disable", "v"], Request::SetRouteEnabled { name: "v".into(), enabled: false }),
(&["routes", "remove", "v", "--yes"], Request::DeleteRoute { name: "v".into() }),
(&["routes", "preset", "discord-voice"], Request::AddRoutePreset { preset: Preset::DiscordVoice }),
(&["routes", "flows"], Request::RouteFlows { all: false }),
(&["routes", "flows", "--all"], Request::RouteFlows { all: true }),
```

в `what_the_window_confirms_needs_yes` — `&["routes", "remove", "v"]`; новый:

```rust
#[test]
fn routes_add_and_edit_read_one_rule_from_a_file() {
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("rule.toml");
    std::fs::write(&file, "[rule]\nprotocol = \"udp\"\nips = [\"104.29.0.0/16\"]\n").unwrap();
    let path = file.to_str().unwrap();
    let (_, asked) = drive(&["routes", "add", "v", "--file", path], Vec::new());
    let rule = router::rules::parse_rule_file("v", "[rule]\nprotocol = \"udp\"\nips = [\"104.29.0.0/16\"]\n").unwrap();
    assert_eq!(asked, [Request::SaveRoute { rule: rule.clone(), create: true }]);
    let (_, asked) = drive(&["routes", "edit", "v", "--file", path], Vec::new());
    assert_eq!(asked, [Request::SaveRoute { rule, create: false }]);
    std::fs::write(&file, "[rule]\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\ndomains = [\"discord.media\"]\n").unwrap();
    let (outcome, asked) = drive(&["routes", "add", "v", "--file", path], Vec::new());
    assert!(asked.is_empty());
    let fail = outcome.err().unwrap();
    assert_eq!(fail.code, 2);
    assert!(fail.error.contains("`domains` is not supported yet"), "{}", fail.error);
}
```

`crates/cli/src/lib.rs`: `samples()` — девять новых запросов (как в таблице, `SaveRoute` с `router::rules::preset(Preset::DiscordVoice)`); тесты `every_request_is_a_window_command_with_its_arguments` и `every_request_comes_through_the_gate_as_it_left` их проверяют.

- [ ] **Шаг 2: Убедиться, что падают** — `cargo test -p cli`: ошибки компиляции.
- [ ] **Шаг 3: Реализовать** по Interfaces; `src-tauri/src/cli.rs` — ветки `dispatch`.
- [ ] **Шаг 4: Проверка** — `cargo test -p cli` и `cargo test -p zapret-hub --lib`: PASS.
- [ ] **Шаг 5: Коммит** — `feat(cli): router on|off|status and routes commands`

---

### Задача 9: вкладка «Маршруты»

**Files:**
- Modify: `frontend/src/lib/app.svelte.ts` (`Tab` += `"routes"`), `frontend/src/App.svelte` (вкладка после «Zapret»), `frontend/src/lib/strings.ts`, `frontend/src/lib/format.ts`, `frontend/src/lib/format.test.ts`
- Create: `frontend/src/tabs/Routes.svelte`, `frontend/src/tabs/RouteEditor.svelte`

**Interfaces:**
- Consumes: `commands.{setRouterEnabled, routerStatus, listRoutes, saveRoute, setRouteEnabled, deleteRoute, addRoutePreset, routeFlows, validateRoute, importRouteLines}`, типы `Rule`, `RouterStatus`, `FlowRow`, `Via`, `ProxyState` из `bindings.ts`; `call`/`change` (`api.ts`, `app.svelte.ts`); `confirmation.ask` (`confirm.svelte.ts`); `plural` (`strings.ts`).
- Produces (`format.ts`): `routeSummary(rule: Rule): string`, `bytes(n: number): string`, `viaLabel(via: Via): string`, `proxyLabel(proxy: ProxyState): string`.

Строки (`S.tabs.routes = "Маршруты"`, `S.routes`): «Маршрутизация»; состояние — «выключена», «включена», «пауза: идёт тест», «недоступна: {reason}»; прокси — `Ok` «прокси отвечает · {n} мс», `Down` «прокси недоступен, трафик идёт напрямую», `AuthRequired` «прокси просит авторизацию, трафик идёт напрямую», `NoUdp` «прокси не поддерживает UDP: включите в NekoBox mixed inbound (SOCKS и HTTP на одном порту)», `Unknown` «прокси проверяется…»; итог «через прокси: {потоки} · ↑ {байты} ↓ {байты}» и «напрямую: …»; «Новое правило», «Добавить из профиля» → «Discord голос», «Изменить», «Удалить», вопрос «Удалить правило «{name}»?»; поля редактора «Имя», «Включено», «Протокол: UDP», «Порты (по одному в строке, пусто — все)», «Подсети IPv4 (по одной в строке)», «Импорт из файла…»; потоки — «Потоки», «только по правилам» / «все», колонки «Протокол», «Назначение», «Правило», «Маршрут», «↑», «↓», «Время»; маршрут — «прокси», «открывается», «напрямую: {причина}» с причинами `ProxyDown` «прокси недоступен», `AuthRequired` «прокси просит авторизацию», `NoUdp` «прокси не поддерживает UDP», `Timeout` «прокси не ответил за 3 с», `Failed` «{error}», `Loop` «трафик прокси или хаба».

Поведение:
- `Routes.svelte`: при видимой вкладке раз в секунду `routerStatus`, `listRoutes`, `routeFlows(all)`; переключатель — `change(…, setRouterEnabled(!on))`; `rules_error`/`RoutesView.error` — красная плашка с текстом ошибки (место в файле); строка правила — переключатель (`setRouteEnabled`), имя, `routeSummary`, трафик правила (`bytes`), «Изменить», «Удалить» (через `confirmation.ask`, `danger: true`); закрытые потоки — приглушённые; длительность `seconds` как `м:сс`.
- `RouteEditor.svelte`: имя (только чтение при правке: переименования нет), «Включено», поля портов и подсетей (`textarea`, строки → массив без пустых); на каждом изменении `validateRoute(rule)` → ошибка под своим полем, «Сохранить» выключено при ошибках; «Импорт из файла…» — `<input type="file" accept=".txt">`, `await file.text()` → `importRouteLines(ips, text)` → замена списка подсетей, ошибка — в уведомления (`call`).

- [ ] **Шаг 1: Падающие тесты** — `frontend/src/lib/format.test.ts`:

```ts
describe("routes", () => {
  const voice = { name: "Discord голос", enabled: true, protocol: "udp", ports: ["19294-19344", "50000-65535"], ips: ["104.29.0.0/16", "35.217.0.0/16"] } as const;
  it("sums a rule up by protocol and subnets", () => {
    expect(routeSummary({ ...voice, ports: [...voice.ports], ips: [...voice.ips] })).toBe("UDP · 2 подсети");
    expect(routeSummary({ ...voice, ports: [], ips: ["1.1.1.1"] })).toBe("UDP · 1 подсеть");
    expect(routeSummary({ ...voice, ports: [], ips: ["1", "2", "3", "4", "5"] })).toBe("UDP · 5 подсетей");
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
```

- [ ] **Шаг 2: Убедиться, что падают** — `npx --prefix frontend vitest run src/lib/format.test.ts`: `routeSummary` не экспортируется.
- [ ] **Шаг 3: Реализовать** `format.ts`, `strings.ts`, обе вкладки, `App.svelte`, `app.svelte.ts`.
- [ ] **Шаг 4: Проверка** — `npm --prefix frontend test` и `npm --prefix frontend run check` (0/0); `npm --prefix frontend run build` проходит.
- [ ] **Шаг 5: Коммит** — `feat(ui): the Routes tab with rules, an editor and live flows`

---

### Задача 10: документация, версия 0.4.0

**Files:**
- Modify: `README.md`, `docs/cli.md`, `CHANGELOG.md`, `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md`, `Cargo.toml` (`version = "0.4.0"`), `package.json`, `frontend/package.json`, `package-lock.json`, `Cargo.lock`

Содержание:
- **README**: раздел «Маршрутизация через SOCKS5» после «zapret2»: что делает (UDP по подсетям через локальный SOCKS5 без TUN, замена `discord-udp-proxy.ps1` — выключить скрипт), как включить (адрес SOCKS5 в «Настройках», NekoBox с mixed inbound, «Маршруты» → «Добавить из профиля» → «Discord голос» → переключатель), формат `data\routes.toml` (пример из спецификации 5.1 без `domains`/`apps`, правила проверки, перечитывание раз в 2 с), прокси недоступен → напрямую, пауза на время тестов, WinDivert из папки движка, этапы 2–3 позже. «Где данные» — `data\routes.toml`; «Что приложение меняет в системе» — дескрипторы WinDivert, пока маршрутизация включена (драйвер движка, своё ничего не ставится; закрытие хаба возвращает трафик как был).
- **docs/cli.md**: строки таблицы «Команды» (`router on|off|status` → `set_router_enabled`, `router_status`; `routes list|show|add|edit|enable|disable|remove --yes|preset discord-voice|flows [--all]` → свои команды); в «Команд окна, которых нет в CLI» — `validate_route`, `import_route_lines` (редактор окна); раздел «Маршруты» с примером `rule.toml` (`[rule]` без `name`) и тем, что `routes add` проверяет файл до отправки (код 2); «Проверка вживую» — пункты 13–18:
  13. NekoBox с mixed inbound запущен, скрипт `discord-udp-proxy.ps1` закрыт. «Маршруты» → «Добавить из профиля» → «Discord голос», переключатель «Маршрутизация» → в шапке «прокси отвечает», голосовой канал Discord соединяется, в «Потоках» строки «прокси» с растущими ↑/↓; в журнале NekoBox — UDP к `104.29.*`/`35.217.*`.
  14. Приоритет: служба zapret запущена со стратегией; `winws.exe --debug=1` той же стратегии вручную (служба остановлена) при включённой маршрутизации не печатает пакетов к подсетям правила (спецификация 3.1, риск 11).
  15. Закрыть NekoBox посреди звонка: не позже чем через 10 с в шапке «прокси недоступен, трафик идёт напрямую», на значке трея красная точка; новый звонок идёт напрямую (если zapret это пропускает); запустить NekoBox — новые потоки снова «прокси».
  16. «Тесты» → прогон двух стратегий: в шапке «пауза: идёт тест», после прогона — «включена».
  17. `apply zapret2:general` (и обратно `apply general`) при включённой маршрутизации: служба другого движка стартует, маршрутизация снова «включена» (DLL нового движка), звонок работает.
  18. CLI: `router status` → JSON; `routes add mine --file rule.toml` с `domains` → код 2 и `` `domains` is not supported yet``; `routes remove mine` → код 2, с `--yes` → код 0; `router off` → вкладка показывает «выключена», `zapret-hub.toml` — `router_enabled = false`.
- **CHANGELOG** `## 0.4.0`: маршрутизация UDP через SOCKS5 (этап 1) — правила, вкладка, трей, CLI, пауза на время тестов.
- **core-спецификация**: поправка к 4.3/13 по спецификации 12 — `router_enabled` в `zapret-hub.toml`, `data\routes.toml`.

- [ ] **Шаг 1:** правки документов и версий (`npm install --package-lock-only` для `package-lock.json`; `cargo check --workspace` обновит `Cargo.lock`).
- [ ] **Шаг 2: Полная проверка** — `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --exclude zapret-hub`, `cargo test -p zapret-hub --lib`, `npm --prefix frontend run check`, `npm --prefix frontend test`: всё чисто.
- [ ] **Шаг 3: Коммит** — `docs: router stage 1 in the README, cli.md and the changelog; version 0.4.0`

---

## Самопроверка плана

- **Покрытие спецификации (этап 1):** 3 крейт и модули — задачи 1–6 (`udp`/`stats` спецификации сведены в `flows.rs`: одна ответственность, меньше файлов); 3.1 DLL, ошибка, приоритет — 2, 5, 7, проверка вживую 14; 4.1 перехват/сокеты, пересборка ≤ 1/с — 2, 6; 4.2 UDP (заголовок, отражение адреса, 30 с, 3 с, 64) — 2–4, 6; 4.4 петля и 50 мс — 4, 6; 5.1 формат, проверка, строки, горячая перезагрузка — 1, 6; 5.2 профиль и импорт — 1, 9; 6 прокси — 3, 4, 6, 9; 7 тесты, движок, правила — 6, 7; 8 вкладка и трей — 7, 9; 9 CLI — 8; 10 тесты — каждая задача, `#[ignore]` — 5, 6, чек-лист — 10; 12 поправка к core — 10.
- **Согласованность имён:** `Setup`, `Router::{start, fail, stop, pause, resume, edit, rules, flows, status, on_change}`, `RouterStatus { state, proxy, totals, rules_error }`, `Via`/`DirectReason`/`ProxyState` с тегами `via`/`kind`/`state` — одинаково в задачах 4, 6, 7, 9; `Preset::DiscordVoice` = `discord-voice` в 1, 7, 8.
- **Пропорция:** тела — только формат фильтра и схема потоков (их тесты не определяют полностью).
