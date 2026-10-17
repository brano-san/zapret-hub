# Zapret Hub — маршрутизация через SOCKS5, этап 3 (правила по программе): план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** правило совпадает не только по подсетям и доменам, но и по программе (`apps` в `routes.toml`: имя `.exe` без учёта регистра или полный путь); в «Потоках» колонка «Программа», фильтр «только по правилам / все» показывает и потоки без правила; профиль «Dead by Daylight», `routes preset dbd`; версия 0.6.0.

**Architecture:** `rules` получает `apps`, `Table::{matches_flow, wants_owner, with_running, idle_apps, starts}` и профиль `Dbd`; `owner` — PID по таблицам Windows для сокетов без события (`table_pid`), запоминание «владелец неизвестен» (`Owners::pid`), путь exe с кэшем по PID и времени старта (`Images`); `flows` — программа в строке, потоки без правила (`DirectReason::NoRule`, предел строк); `divert::capture_filter` — для правил по программе SYN всегда, UDP только пока программа из правила запущена. `Live` решает TCP на SYN, UDP на первом пакете потока, не совпавшее отправляет без изменений. Приложение — `bindings.ts`, CLI — `dbd` и протокол 3, вкладка — поле «Программы» и колонка.

**Tech Stack:** новых внешних зависимостей нет. Rust 1.99.0 (edition 2024); `windows` 0.62 (+ фича `Win32_System_Threading`: `OpenProcess`, `GetProcessTimes`, `QueryFullProcessImageNameW`; `GetExtendedUdpTable` — уже включённая `Win32_NetworkManagement_IpHelper`; сигнатуры 0.62 сверить на docs.rs: возвращают `Result`); `toml` 1.1.8; specta 2.0.0-rc.25 (u64 в TS запрещён); clap 4.6; tauri 2.12; Svelte 5.57.1, svelte-check 4.7.6, vitest 5.0.3. WinDivert 2.2 из DLL движка.

**Spec:** `docs/superpowers/specs/2026-10-17-zapret-hub-router-design.md` (далее «спецификация»); этап 3 = раздел 2, пункт 3: 4.1 (дескриптор сокетов для правил по программе), 4.4, 4.6, 5.1 (`apps`), 5.2 (профиль DbD), 8 (колонка «Программа», фильтр потоков), 9 (`dbd`), 10 (чек-лист). Стиль и решения этапов 1–2 — `docs/superpowers/plans/2026-10-17-zapret-hub-router-stage1.md`, `…-stage2.md`. Исполнитель читает спецификацию вместе с планом.

**База:** `main` на `b386de5`; ветка `feat/router-stage3` от неё.

## Global Constraints

- Только Windows 11 x64, Rust stable msvc 1.99.0. Крейты не зависят от `tauri` (только derive `specta`). Логика — в `crates/router`; команды `src-tauri` — несколько строк, новых команд нет.
- Решения этапов 1–2 не меняются: прокси только на loopback (`parse_proxy`); `Down` после двух неудачных проверок подряд, возврат прокси переоткрывает потоки «напрямую из-за прокси» и снимает пометки; не-unicast (`packet::unicast`) идёт без изменений; фильтр `outbound and !loopback and ip and (…)`, `FILTER_BUDGET = 200` тестов, `MAX_FILTER_RANGES = 64` со слиянием ближайших, совпадение сверяет хаб; удержание SYN до фильтра, который его соединение ловит (`Waiting`), без паузы `REOPEN_GAP`; `status` и `flows` — из снимка; счётчик пауз `routing::PAUSES`; смена дескриптора — новый открыт до `shutdown(RECV)` старого, нет правил — фильтр `false` (дескриптор только инжектит); полузакрытие реле; `Names` в `Router` переживает паузы.
- Значения этапа 3: ожидание PID `PID_WAIT = 50 мс` (как 4.4) — только если PID нет ни в событиях сокетов, ни в таблицах Windows; порт, для которого PID не нашёлся, больше не ждётся до события сокета или его закрытия; строк потоков без правила не больше `MAX_UNRULED = 500`; живость процессов из кэша exe проверяется раз в `RULES_CHECK_TICKS` (2 с).
- `apps`: имя файла (без `\` и `/`, без `:*?"<>|`) или абсолютный путь (`Path::is_absolute`: `C:\…`, `C:/…`, `\\server\share\…`), в обоих случаях оканчивается на `.exe` без учёта регистра. Сравнение без учёта регистра (`to_lowercase`, `/` → `\`): элемент с `\` — весь путь exe, иначе — имя файла exe. Цели правила — `ips`, `domains`, `apps`, совпадение любой (5.1).
- Профиль `dbd`: `Rule { name: "Dead by Daylight", enabled: true, protocol: Both, ports: [], ips: [], domains: ["bhvrdbd.com"], apps: ["DeadByDaylight-Win64-Shipping.exe"] }`.
- Протокол CLI ↔ приложение — 3 (`cli::PROTOCOL`): `Rule.apps` — новое обязательное поле запроса `save_route`, старое приложение молча выбросило бы его.
- Тесты не меняют систему: чистые модульные (правила, совпадение по программе, фильтр, кэш exe с подставным запросом, запоминание владельцев, строки потоков); чтение имени и времени старта собственного процесса теста и PID собственных сокетов на loopback — только чтение; всё, что открывает WinDivert, — `#[ignore]`.
- Тексты: код, ошибки Rust, коммиты — английский; интерфейс, `strings.ts`, `docs/`, README — русский.
- Коммит — одна строка Conventional Commits (scope `router`, `app`, `cli`, `ui`, `docs`), без тела и трейлеров; `git add` с явными путями. Перед коммитом с Rust — `cargo fmt --all`, `cargo clippy --workspace --all-targets -- -D warnings` чисто; с фронтендом — `npm --prefix frontend run check` 0/0 и `npm --prefix frontend test`. Тесты приложения — `cargo test -p zapret-hub --lib`. После смены DTO — `cargo run -p zapret-hub --bin export-bindings`, `bindings.ts` в том же коммите.

### Решение о стоимости перехвата для правил по программе (Ruling A)

Спецификация 4.6: «фильтр включает весь трафик с портами и протоколом правила». У профиля DbD портов нет, то есть дословно — весь исходящий TCP и UDP машины через хаб, всегда, пока правило включено. Решение:

- **TCP — только первые SYN** (`tcp and tcp.Syn and !tcp.Ack` на портах правила, та же часть `Kind::Syn`, что у правил по доменам, и не дороже её). Решение на SYN; совпавшее соединение получает запись NAT, его адрес назначения входит в часть `Held` (механизм этапа 2), а SYN ждёт в `Waiting`, пока фильтр с этим адресом не встанет (без `REOPEN_GAP`). Остальные сегменты чужих соединений через хаб не идут.
- **UDP — только пока запущена программа из правила.** Часть `udp [and (<порты>)]` без IP входит в фильтр, когда в кэше exe (`Images`) есть живой процесс, чьё exe совпало с UDP-правилом по программе (`Compiled.running`). Процесс узнаётся по событию сокета (BIND/CONNECT) или по первому потоку; при включении после паузы — из кэша, который живёт в `Router`. Пока игра не запущена — UDP не перехватывается совсем.
- **Не совпавшее — сразу назад.** Новый поток (UDP) или SYN без правила: PID из событий, затем из `GetExtended{Tcp,Udp}Table`, только потом ожидание ≤ 50 мс, и «неизвестен» запоминается: захват не встаёт на 50 мс на каждый поток процесса, чей сокет открыт до сеанса (торрент, браузер). Решение запоминается на поток (`FlowState::Direct(NoRule)`), дальше — один поиск в таблице потоков и `send`.
- **Цена:** пока игра запущена, весь исходящий UDP машины (на портах правила) идёт через хаб — подсказка в редакторе советует порты; браузер в `apps` переоткрывает фильтр на каждый новый адрес назначения (`// ponytail:` у удержания SYN, апгрейд — часть `tcp.SrcPort` по портам совпавших сокетов). Первые пакеты UDP-потока программы, чей первый сокет — UDP и открыт раньше, чем фильтр пересобрался, уходят напрямую (как риск этапа 2 «поток UDP во время смены фильтра»).

## Review Focus

1. **Программа запущена до включения маршрутизации или пережила паузу теста** (DbD посреди матча): событий её сокетов хаб не видел — PID должен найтись по таблицам Windows, а не «неизвестен → напрямую». Тест: `an_unknown_owner_is_looked_up_then_remembered`, `the_owner_of_a_bound_port_is_found_in_the_tables` (Task 3); живой — `an_app_rule_follows_curl` (Task 6).
2. **Правило по программе без портов, программа не запущена** (DbD включён в профилях, игра закрыта): фильтр не должен ловить UDP вообще, TCP — только SYN. Тест: `an_app_rule_captures_first_syns_and_udp_only_while_its_program_runs` (Task 5).
3. **Сокеты, чей владелец так и не нашёлся** (системные, закрытые раньше пакета): захват не должен ждать 50 мс на каждый их поток. Тест: `an_unknown_owner_is_looked_up_then_remembered` (Task 3).
4. **PID переиспользован другим процессом** после выхода игры: exe из кэша не должен совпасть с правилом. Тест: `a_process_image_is_cached_by_pid_and_start_time` (Task 3).
5. **Поток без правила в изобилии** (DHT торрента, пока игра запущена): строки «все» ограничены, итоги «через прокси / напрямую» не раздуваются. Тест: `rule_less_flows_show_only_in_all_with_their_program` (Task 4).

---

## Структура файлов

```
crates/router/
  Cargo.toml          windows += "Win32_System_Threading"
  src/rules.rs        Rule.apps, Preset::Dbd, проверка apps, runs, Compiled.{apps, running},
                      Table::{matches_flow, wants_owner, with_running, idle_apps, starts}
  src/owner.rs        Owners: HashMap<u16, Option<u32>>, Owners::pid; table_pid; process_start, image_path, Images
  src/flows.rs        FlowRow.program, rule None для потоков без правила, DirectReason::NoRule,
                      Stats::set_program, MAX_UNRULED, итоги без потоков без правила
  src/divert.rs       capture_filter: SYN и UDP правил по программе; refused_for_good
  src/tcp.rs          Nat::forget
  src/lib.rs          Inner.images, Live.images, Waiting.held, release, on_packet (UDP), socket loop, apply,
                      housekeeping; память отказа фильтра только для ошибки 87
  src/live_tcp.rs     on_syn по программе, release(None) забывает запись NAT
  tests/rules.rs, flows.rs, filter.rs, router.rs, live.rs (#[ignore])
crates/cli/src/{lib.rs, main.rs}   PROTOCOL = 3, PresetArg::Dbd, apps в файле правила
frontend/src/lib/{strings.ts, format.ts, format.test.ts, ipc/bindings.ts}, tabs/{RouteEditor.svelte, Routes.svelte}
README.md, docs/cli.md, CHANGELOG.md, спецификация (поправки этапа 3), версии 0.6.0
```

---

### Task 1: долги этапа 2 — память отказа фильтра, список причин в тесте восстановления, уведомление проверки

**Files:**
- Modify: `crates/router/src/divert.rs` (`refused_for_good`), `crates/router/src/lib.rs` (`Live::apply`), `frontend/src/tabs/RouteEditor.svelte`
- Test: `crates/router/tests/filter.rs` (`a_refused_capture_filter_names_too_many_rules`), `crates/router/tests/flows.rs` (`only_flows_sent_direct_by_a_proxy_failure_retry_it`)

**Interfaces:**
- Produces: `pub fn refused_for_good(e: &std::io::Error) -> bool` — `e.raw_os_error() == Some(87)` (фильтр не влез; другие ошибки — нехватка памяти, занятый драйвер — повторяются со следующей пересборкой).

- [ ] **Шаг 1: Падающие тесты** — в `a_refused_capture_filter_names_too_many_rules`: `assert!(refused_for_good(&invalid) && !refused_for_good(&other));`. В `only_flows_sent_direct_by_a_proxy_failure_retry_it` второй список += `DirectReason::Marked`, `DirectReason::NotAccepted`.
- [ ] **Шаг 2:** `cargo test -p router --test filter` — ошибка компиляции (`refused_for_good`).
- [ ] **Шаг 3:** `refused_for_good` по Interfaces; в `Live::apply` при `Err(e)` от `Handle::open` `filter.failed = Some((text, reason))` — только если `divert::refused_for_good(&e)`, иначе `failed` не трогается (причина всё равно возвращается и видна как ошибка правил).
- [ ] **Шаг 4:** `cargo test -p router` — PASS; коммит `fix(router): remember a refused capture filter only when it did not fit`.
- [ ] **Шаг 5:** `RouteEditor.svelte`: обычная (не `$state`) переменная `let validateFailing = false`; `.then` сбрасывает её в `false`; `.catch` кладёт уведомление `S.failed.validate`, только если `asked === draft && !validateFailing`, и ставит `true` — один сбой IPC не даёт уведомления на каждую клавишу.
- [ ] **Шаг 6:** `npm --prefix frontend run check` 0/0, `npm --prefix frontend test` — PASS; коммит `fix(ui): notify a failing rule check once, not per keystroke`.

---

### Task 2: правила — `apps`, совпадение по программе, профиль «Dead by Daylight»

**Files:**
- Modify: `crates/router/src/rules.rs`
- Test: `crates/router/tests/rules.rs`; литералы `Rule` в `crates/router/tests/filter.rs`, `crates/router/tests/live.rs` (+ `apps: vec![]`)

**Interfaces:**
- Produces (`router::rules`):
  - `Rule` += `pub apps: Vec<String>` (после `domains`, как `domains` — обязательное поле, в файл пишется всегда); профили `DiscordVoice`, `Discord` — `apps: vec![]`;
  - `Preset` += `Dbd` (serde `"dbd"`: `#[serde(rename = "dbd")]` поверх `kebab-case`); `preset(Dbd)` — значение из Global Constraints;
  - `check`: каждый элемент `apps` по Global Constraints → поле `apps`, сообщение `"{a}" is not a program: write its file name like Discord.exe or its full path like C:\Games\Discord.exe`; пустые `ips`, `domains` и `apps` → поле `ips`, `a rule needs at least one target: ips, domains or apps`;
  - `Raw.apps: Option<Spanned<Vec<String>>>` вместо отказа «`` `apps` is not supported yet``»; строка ошибки поля `apps` — `span` этого поля;
  - `pub fn runs(app: &str, exe: &str) -> bool` — `app` уже нормализован (как в `Compiled.apps`), `exe` нормализуется здесь; правило сравнения — Global Constraints;
  - `Compiled` += `pub apps: Vec<String>` (нормализованные), `pub running: bool` (`Table::new` — `false`);
  - `Table::matches_flow(&self, protocol: Protocol, dst: SocketAddrV4, exe: Option<&str>) -> Option<&str>` — первое правило в порядке файла: протокол и порт как в `matches`, и (`dst` в `ranges` или `exe` есть и `runs` для какого-то `app`); `matches` не меняется (только адреса);
  - `Table::wants_owner(&self, protocol: Protocol, port: u16) -> bool` — у какого-то правила с `apps` совпали протокол и порт;
  - `Table::with_running(self, exes: &[&str]) -> Table` — `running = true` правилам `Udp | Both` с `apps`, у которых какой-то `exe` из списка совпал;
  - `Table::idle_apps(&self) -> bool` — есть правило `Udp | Both` с `apps` и `!running`;
  - `Table::starts(&self, exe: &str) -> bool` — такое правило (`Udp | Both`, `!running`) совпало с `exe`.

- [ ] **Шаг 1: Падающие тесты** — `tests/rules.rs`: литерал в `the_discord_voice_preset_is_the_spec` += `apps: vec![]`; замыкания `with` в `ports_and_subnets_are_checked` и `domains_are_ldh_names_without_scheme_or_path` ставят `apps: vec![]`, в последнем `"a rule needs ips or domains"` остаётся полем `ips`; `tcp_and_both_parse_and_apps_is_still_named` переименовать в `tcp_and_both_parse` без части про `apps`; новые:

```rust
const DBD: &str = r"D:\Steam\steamapps\common\Dead by Daylight\DeadByDaylight\Binaries\Win64\DeadByDaylight-Win64-Shipping.exe";

fn app_rule(name: &str, protocol: Protocol, ports: &[&str], apps: &[&str]) -> Rule {
    Rule {
        name: name.into(),
        enabled: true,
        protocol,
        ports: ports.iter().map(|s| s.to_string()).collect(),
        ips: vec![],
        domains: vec![],
        apps: apps.iter().map(|s| s.to_string()).collect(),
    }
}

#[test]
fn the_dbd_preset_is_the_spec() {
    let dbd = rules::preset(Preset::Dbd);
    assert_eq!((dbd.name.as_str(), dbd.protocol), ("Dead by Daylight", Protocol::Both));
    assert_eq!(dbd.apps, ["DeadByDaylight-Win64-Shipping.exe"]);
    assert_eq!(dbd.domains, ["bhvrdbd.com"]);
    assert!(dbd.ips.is_empty() && dbd.ports.is_empty());
    assert!(rules::check(&dbd).is_empty());
}

#[test]
fn apps_are_exe_names_or_full_paths() {
    let fields = |apps: &[&str]| rules::check(&app_rule("a", Protocol::Udp, &[], apps)).into_iter().map(|e| e.field).collect::<Vec<_>>();
    assert!(fields(&["Discord.exe", "DeadByDaylight-Win64-Shipping.EXE", r"C:\Games\DbD\x.exe", "D:/Games/y.exe", r"\\nas\games\z.exe"]).is_empty());
    for bad in ["Discord", "", " ", r"Games\x.exe", "*.exe", "a?.exe", "x.exe:stream", r"C:\Games\notes.txt"] {
        assert_eq!(fields(&[bad]), ["apps"], "{bad:?}");
    }
    assert_eq!(fields(&[]), ["ips"], "a rule needs ips, domains or apps");
}

#[test]
fn apps_parse_and_a_bad_one_names_its_line() {
    let ok = "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\napps = [\"Discord.exe\"]\n";
    assert_eq!(rules::parse(ok).unwrap()[0].apps, ["Discord.exe"]);
    let bad = "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\napps = [\"Discord\"]\n";
    let err = rules::parse(bad).unwrap_err();
    assert_eq!((err.line, err.field.as_deref()), (Some(5), Some("apps")));
    let old = "[[rule]]\nname = \"a\"\nprotocol = \"udp\"\nips = [\"1.1.1.1\"]\n";
    assert!(rules::parse(old).unwrap()[0].apps.is_empty(), "a stage-2 file still loads");
}

#[test]
fn a_flow_matches_by_program_name_or_full_path_without_case() {
    let table = Table::new(&[
        app_rule("by name", Protocol::Udp, &[], &["deadbydaylight-win64-shipping.exe"]),
        app_rule("by path", Protocol::Tcp, &["443"], &[r"C:\Games\Discord.exe"]),
    ]);
    let dst = "52.1.1.1:7777".parse().unwrap();
    let https = "1.1.1.1:443".parse().unwrap();
    assert_eq!(table.matches_flow(Protocol::Udp, dst, Some(DBD)), Some("by name"));
    assert_eq!(table.matches_flow(Protocol::Tcp, dst, Some(DBD)), None, "the rule is UDP only");
    assert_eq!(table.matches_flow(Protocol::Udp, dst, None), None, "no owner, no program");
    assert_eq!(table.matches_flow(Protocol::Tcp, https, Some(r"c:\games\DISCORD.exe")), Some("by path"));
    assert_eq!(table.matches_flow(Protocol::Tcp, https, Some(r"D:\Discord.exe")), None, "a full path is the whole path");
    assert_eq!(table.matches_flow(Protocol::Tcp, "1.1.1.1:80".parse().unwrap(), Some(r"C:\Games\Discord.exe")), None, "ports still count");
    assert_eq!(table.matches(Protocol::Udp, dst), None, "address matching ignores programs");
    assert!(table.wants_owner(Protocol::Udp, 7777) && table.wants_owner(Protocol::Tcp, 443));
    assert!(!table.wants_owner(Protocol::Tcp, 80) && !Table::new(&[voice()]).wants_owner(Protocol::Udp, 50000));
}

#[test]
fn a_udp_app_rule_runs_while_its_program_runs() {
    let table = Table::new(&[rules::preset(Preset::Dbd)]);
    assert!(table.idle_apps() && table.starts(DBD) && !table.starts(r"C:\x\chrome.exe"));
    let running = table.with_running(&[r"C:\x\chrome.exe", DBD]);
    assert!(running.rules[0].running && !running.idle_apps() && !running.starts(DBD));
    let tcp = Table::new(&[app_rule("t", Protocol::Tcp, &[], &["x.exe"])]);
    assert!(!tcp.idle_apps() && !tcp.with_running(&[r"C:\x.exe"]).rules[0].running, "TCP needs only SYNs");
}
```

- [ ] **Шаг 2:** `cargo test -p router --test rules` — ошибки компиляции (`apps`, `Preset::Dbd`, `matches_flow`).
- [ ] **Шаг 3:** реализовать по Interfaces; литералы `Rule` в `tests/filter.rs` и `tests/live.rs` — `apps: vec![]`.
- [ ] **Шаг 4:** `cargo test -p router` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): apps in rules and the Dead by Daylight preset`.

---

### Task 3: владельцы — таблицы Windows, «неизвестен» без повторного ожидания, путь exe с кэшем

**Files:**
- Modify: `crates/router/src/owner.rs`, `crates/router/Cargo.toml` (`Win32_System_Threading`)
- Test: `crates/router/tests/flows.rs` (там уже тесты `owner`)

**Interfaces:**
- Produces (`router::owner`):
  - `Owners` хранит `HashMap<u16, Option<u32>>` (`None` — «искали, не нашли»); `record`, `forget`, `wait` — те же сигнатуры, `wait` возвращает сразу и для запомненного `None`;
  - `Owners::pid(&self, local_port: u16, timeout: Duration, lookup: impl FnOnce(u16) -> Option<u32>) -> Option<u32>` — запись есть → она; иначе `lookup(port)` → найден → `record`; иначе `wait(port, timeout)`; не дождались → запомнить `None`. Событие сокета позже перезаписывает `None`;
  - `pub fn table_pid(protocol: Protocol, local_port: u16) -> Option<u32>` — `Udp`: `GetExtendedUdpTable(UDP_TABLE_OWNER_PID)`, `Tcp`: `GetExtendedTcpTable(TCP_TABLE_OWNER_PID_ALL)`, IPv4, первая строка с этим локальным портом (`port_of`); цикл буфера — общий с `listener_pid` (вынести `fn owner_rows<Row>(…)`);
  - `pub fn process_start(pid: u32) -> Option<u64>` — `OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION)` + `GetProcessTimes`: время создания (`FILETIME` как `u64`); `None`, если не открылся или время выхода ≠ 0 (процесс завершён, но его держит чужой дескриптор);
  - `pub fn image_path(pid: u32) -> Option<String>` — `QueryFullProcessImageNameW(PROCESS_NAME_WIN32)`, буфер 32 768 `u16`;
  - `#[derive(Default)] pub struct Images(Mutex<HashMap<u32, (u64, Arc<str>)>>)`:
    - `lookup(&self, pid: u32, start: u64, image: impl FnOnce() -> Option<String>) -> Option<Arc<str>>` — тот же `start` → кэш; иначе `image()` и запись;
    - `get(&self, pid: u32) -> Option<Arc<str>>` = `process_start(pid)` → `lookup(pid, start, || image_path(pid))`;
    - `prune(&self, alive: impl Fn(u32) -> Option<u64>)` — удаляет записи, у которых `alive(pid) != Some(start)`;
    - `paths(&self) -> Vec<Arc<str>>`.
  - `unsafe` — только вызовы Win32 с `// SAFETY:`; дескриптор процесса закрывается (`CloseHandle`) на всех путях.

- [ ] **Шаг 1: Падающие тесты** — `tests/flows.rs` (импорты `owner::{Images, table_pid}`, `rules::Protocol`, `std::net::{TcpListener, UdpSocket}`):

```rust
#[test]
fn an_unknown_owner_is_looked_up_then_remembered() {
    let owners = Owners::default();
    assert_eq!(owners.pid(5000, PID_WAIT, |_| Some(42)), Some(42), "from the tables");
    assert_eq!(owners.pid(5000, PID_WAIT, |_| panic!("known")), Some(42));
    let started = Instant::now();
    assert_eq!(owners.pid(5001, PID_WAIT, |_| None), None);
    assert!(started.elapsed() >= PID_WAIT);
    let started = Instant::now();
    assert_eq!(owners.pid(5001, PID_WAIT, |_| panic!("remembered")), None);
    assert!(started.elapsed() < PID_WAIT, "a port without an owner is not waited for again");
    owners.record(5001, 7);
    assert_eq!(owners.pid(5001, PID_WAIT, |_| None), Some(7), "a late event still lands");
    owners.forget(5001);
    assert_eq!(owners.pid(5001, Duration::ZERO, |_| Some(8)), Some(8), "a closed socket is looked up again");
}

#[test]
fn the_owner_of_a_bound_port_is_found_in_the_tables() {
    let udp = UdpSocket::bind("127.0.0.1:0").unwrap();
    let tcp = TcpListener::bind("127.0.0.1:0").unwrap();
    let me = Some(std::process::id());
    assert_eq!(table_pid(Protocol::Udp, udp.local_addr().unwrap().port()), me);
    assert_eq!(table_pid(Protocol::Tcp, tcp.local_addr().unwrap().port()), me);
}

#[test]
fn a_process_image_is_cached_by_pid_and_start_time() {
    let images = Images::default();
    let dbd = r"C:\Games\DeadByDaylight-Win64-Shipping.exe";
    assert_eq!(images.lookup(10, 1, || Some(dbd.into())).as_deref(), Some(dbd));
    assert_eq!(images.lookup(10, 1, || panic!("cached")).as_deref(), Some(dbd));
    assert_eq!(images.lookup(10, 2, || Some(r"C:\x\other.exe".into())).as_deref(), Some(r"C:\x\other.exe"), "a reused PID");
    images.prune(|pid| (pid == 10).then_some(2));
    assert_eq!(images.paths().len(), 1);
    images.prune(|_| None);
    assert!(images.paths().is_empty());
}

#[test]
fn this_process_has_a_start_time_and_an_image() {
    let pid = std::process::id();
    let start = owner::process_start(pid).unwrap();
    assert_eq!(owner::process_start(pid), Some(start));
    let exe = owner::image_path(pid).unwrap();
    let me = std::env::current_exe().unwrap();
    assert_eq!(exe.to_lowercase(), me.to_string_lossy().to_lowercase());
    assert_eq!(Images::default().get(pid).as_deref(), Some(exe.as_str()));
    assert_eq!(owner::process_start(0), None, "the idle process does not open");
}
```

- [ ] **Шаг 2:** `cargo test -p router --test flows` — ошибки компиляции.
- [ ] **Шаг 3:** реализовать по Interfaces (`a_late_socket_event_is_waited_for_50_ms`, `the_listener_of_a_port_is_found` — без изменений).
- [ ] **Шаг 4:** `cargo test -p router --test flows` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): find socket owners in the Windows tables and cache process images`.

---

### Task 4: потоки — программа, потоки без правила, предел строк

**Files:**
- Modify: `crates/router/src/flows.rs`
- Test: `crates/router/tests/flows.rs`

**Interfaces:**
- Produces (`router::flows`):
  - `DirectReason` += `NoRule` (поток, пойманный ради правила по программе, но без правила); `proxy_failure()` — `false`;
  - `FlowRow` += `pub program: Option<String>` (полный путь exe, после `domain`); `rule` — `None` для потока без правила (в `Stats` это пустое имя, как сейчас);
  - `Stats::set_program(&mut self, key: FlowKey, program: String)`;
  - `pub const MAX_UNRULED: usize = 500` — `Stats::open` с пустым `rule` ничего не открывает, если строк без правила (открытых и закрытых) уже столько;
  - `Stats::totals` не считает потоки без правила; `rows(now, false)` — только с правилом (уже так); doc-комментарий `rows` — без «stage 3 adds».

- [ ] **Шаг 1: Падающий тест**

```rust
#[test]
fn rule_less_flows_show_only_in_all_with_their_program() {
    let t0 = Instant::now();
    let key = |local_port| FlowKey { protocol: Protocol::Udp, local_port, dst: "8.8.8.8:443".parse().unwrap() };
    let none = || Via::Direct { reason: DirectReason::NoRule };
    let mut stats = Stats::default();
    stats.open(key(1), "Dead by Daylight", Via::Proxy, t0);
    stats.set_program(key(1), r"C:\Games\DeadByDaylight-Win64-Shipping.exe".into());
    stats.open(key(2), "", none(), t0);
    stats.set_program(key(2), r"C:\x\chrome.exe".into());
    stats.add(key(2), 100, 0, t0);
    let ruled = stats.rows(t0, false);
    assert_eq!(ruled.len(), 1);
    assert_eq!(ruled[0].program.as_deref(), Some(r"C:\Games\DeadByDaylight-Win64-Shipping.exe"));
    let rows = stats.rows(t0, true);
    let row = rows.iter().find(|r| r.local_port == 2).unwrap();
    assert_eq!((row.rule.as_deref(), row.program.as_deref()), (None, Some(r"C:\x\chrome.exe")));
    assert_eq!((stats.totals().direct.flows, stats.totals().proxied.flows), (0, 1), "rule-less flows are not in the totals");
    for port in 0..MAX_UNRULED as u16 + 10 {
        stats.open(key(100 + port), "", none(), t0);
    }
    assert_eq!(stats.rows(t0, true).len(), MAX_UNRULED + 1, "rule-less rows are capped");
}
```

И в `only_flows_sent_direct_by_a_proxy_failure_retry_it` второй список += `DirectReason::NoRule`.

- [ ] **Шаг 2:** `cargo test -p router --test flows` — ошибки компиляции.
- [ ] **Шаг 3:** реализовать по Interfaces.
- [ ] **Шаг 4:** `cargo test -p router` — PASS.
- [ ] **Шаг 5:** коммит `feat(router): programs and rule-less flows in flow rows`.

---

### Task 5: фильтр перехвата — правила по программе

**Files:**
- Modify: `crates/router/src/divert.rs` (`capture_filter`)
- Test: `crates/router/tests/filter.rs`

**Interfaces:**
- Consumes: `Compiled.{apps, running}`, `Table::with_running` (Task 2).
- Produces: `capture_filter(table, acceptor, held)` — сигнатура та же; для правила (порядок частей в правиле: адреса, SYN, UDP программы):
  - SYN-часть (`Kind::Syn`, на портах правила, одна на одинаковые порты) — если `protocol != Udp` и (`domains` или `apps` не пусты);
  - UDP-часть программы — `Part { kind: Kind::Udp, ports, ranges: vec![] }` (`(udp)` или `(udp and (<порты>))`), если `protocol != Tcp`, `apps` не пусты и `running`; одинаковые такие части — одна.

- [ ] **Шаг 1: Падающий тест**

```rust
const DBD: &str = r"D:\Games\DeadByDaylight-Win64-Shipping.exe";

fn app_rule(protocol: Protocol, ports: &[&str]) -> Rule {
    Rule { protocol, domains: vec![], apps: vec!["DeadByDaylight-Win64-Shipping.exe".into()], ..rule(ports, &[]) }
}

#[test]
fn an_app_rule_captures_first_syns_and_udp_only_while_its_program_runs() {
    let both = Table::new(&[app_rule(Protocol::Both, &[])]);
    assert_eq!(
        capture_filter(&both, Some(61000), &[]).unwrap(),
        "outbound and !loopback and ip and ((tcp and tcp.Syn and !tcp.Ack) or (tcp and tcp.SrcPort == 61000))",
        "the game is not running: SYNs only"
    );
    assert_eq!(
        capture_filter(&both.with_running(&[DBD]), Some(61000), &[]).unwrap(),
        "outbound and !loopback and ip and ((tcp and tcp.Syn and !tcp.Ack) or (udp) or (tcp and tcp.SrcPort == 61000))"
    );
    let udp = Table::new(&[app_rule(Protocol::Udp, &["7000-8000"])]);
    assert_eq!(filter(&udp), None, "nothing while the game is not running");
    assert_eq!(filter(&udp.clone().with_running(&[r"C:\x\chrome.exe"])), None);
    assert_eq!(
        filter(&udp.with_running(&[DBD])).unwrap(),
        "outbound and !loopback and ip and ((udp and ((udp.DstPort >= 7000 and udp.DstPort <= 8000))))"
    );
    let dbd = Table::new(&[rules::preset(Preset::Dbd)]);
    assert_eq!(filter(&dbd).unwrap(), "outbound and !loopback and ip and ((tcp and tcp.Syn and !tcp.Ack))", "domain and program share the SYN part");
}
```

- [ ] **Шаг 2:** `cargo test -p router --test filter` — FAIL (нет частей правил по программе).
- [ ] **Шаг 3:** реализовать по Interfaces.
- [ ] **Шаг 4:** `cargo test -p router --test filter` — PASS (тесты этапов 1–2 без изменений, кроме `apps: vec![]` из Task 2).
- [ ] **Шаг 5:** коммит `feat(router): capture first SYNs and a running program's UDP for app rules`.

---

### Task 6: `Router` — решение по программе на SYN и первом пакете UDP

**Files:**
- Modify: `crates/router/src/lib.rs`, `crates/router/src/live_tcp.rs`, `crates/router/src/tcp.rs` (`Nat::forget`)
- Test: `crates/router/tests/tcp.rs`, `crates/router/tests/router.rs`, `crates/router/tests/live.rs` (`#[ignore]`)

**Interfaces:**
- Consumes: Tasks 2–5. Публичный API `Router` не меняется.
- Produces (внутреннее): `Nat::forget(&mut self, client_port: u16) -> Option<SocketAddrV4>` — удаляет запись без пометки; `Inner.images: Arc<owner::Images>` (переживает паузы, как `names`), `Live.images` — клон; `Waiting` += `held: Vec<Ipv4Addr>` (удерживаемые адреса фильтра в силе) и `fn covers(&self, dst: SocketAddrV4) -> bool` = `active.matches(Tcp, dst).is_some() || held.contains(dst.ip())`; `Live::release(self: &Arc<Self>, active: Option<(Arc<Table>, Vec<Ipv4Addr>)>)`.

Схема (тела — исполнителя; порядок решений фиксирован):

```text
apply(base): table = base.with_names(names).with_running(&images.paths()); held, text — как в этапе 2;
   release(Some((table, held))) вместо release(Some(table)).
socket loop (не CLOSE): record как сейчас; затем, если table.idle_apps():
   images.get(pid) → table.starts(&exe) → spawn recompose   // программа из UDP-правила появилась
housekeeping, каждые RULES_CHECK_TICKS: + images.prune(owner::process_start) перед inner.refresh()
   (tcp_tick и так пересобирает фильтр каждый тик: вышедшая игра снимает свою UDP-часть ≤ 2,5 с)
on_packet UDP, новый поток (ключа нет в flows):
   addr = table.matches(Udp, dst)
   addr нет и !table.wants_owner(Udp, dst.port()) → send без изменений, без строки   // как этап 2
   pid  = owners.pid(port, PID_WAIT, |p| owner::table_pid(Udp, p)); exe = pid → images.get
   rule = table.matches_flow(Udp, dst, exe)
     None → flows[key] = Direct(NoRule); stats.open(key, "", Direct{NoRule}) + set_program + add; send
     Some → путь этапа 2 (decide → Direct-строка или Opening + open_flow) + set_program
on_syn (live_tcp.rs):
   addr = table.matches(Tcp, dst); addr нет и !table.wants_owner(Tcp, dst.port()) → out без изменений
   pid  = tcp_owners.pid(port, PID_WAIT, |p| owner::table_pid(Tcp, p)); exe = images.get
   rule = table.matches_flow(Tcp, dst, exe) нет → direct_row(key, "", NoRule) + set_program; out
   decide(...) Some(reason) → direct_row(rule, reason) + set_program; out
   lock waiting: !covers(dst) и syns.len() == QUEUE → out без изменений (без записи NAT)
   nat.admit; open_row(Opening) + set_program
   covers(dst) → reflect; иначе SYN в waiting.syns, spawn recompose   // адрес войдёт в Held
release(None) (фильтр не открылся): у каждого SYN nat.forget(port), строка → Direct{Failed{error:
   "the capture filter did not open"}} и close; SYN уходит без изменений
```

Удержание SYN теперь после `admit` (и для правил по адресам): при `release(Some)` повторный `on_tcp` находит запись и переписывает (`from_client` → `Rewrite`). `// ponytail:` у удержания: программа с сотнями адресов (браузер в `apps`) переоткрывает фильтр на каждый новый адрес; апгрейд — часть `tcp.SrcPort == p` по портам совпавших сокетов.

- [ ] **Шаг 1: Тесты** — `tests/tcp.rs`:

```rust
#[test]
fn a_forgotten_entry_leaves_no_mark() {
    let (t0, mut nat) = (Instant::now(), Nat::default());
    nat.admit(&seg(CLIENT, SERVER, SYN), AP, t0);
    assert_eq!(nat.forget(50000), Some(sa(SERVER)));
    assert!(nat.destinations().is_empty());
    assert_eq!(nat.from_client(&seg(CLIENT, SERVER, SYN), AP, t0), Verdict::Syn, "not marked direct");
}
```

`tests/router.rs`:

```rust
#[test]
fn an_app_rule_is_saved_with_its_programs() {
    let dir = tempfile::tempdir().unwrap();
    let r = router(dir.path());
    r.edit(|list| rules::add(list, rules::preset(Preset::Dbd))).unwrap();
    assert!(r.table().wants_owner(rules::Protocol::Udp, 7777));
    assert_eq!(rules::load(&dir.path().join("routes.toml")).unwrap()[0].apps, ["DeadByDaylight-Win64-Shipping.exe"]);
}
```

`tests/live.rs` (`#[ignore]`, `WINDIVERT_DLL` и `ROUTER_SOCKS5`), `an_app_rule_follows_curl`: `running(Rule { name: "curl", protocol: Tcp, ports: ["80"], apps: ["curl.exe"], ips: [], domains: [] })`; `curl("1.1.1.1", "1.1.1.1")` начинается с `HTTP/1.1`; в `flows(false)` строка `protocol == "tcp"`, `destination == "1.1.1.1:80"`, `via: Proxy`, `program` оканчивается на `curl.exe` без учёта регистра; затем `r.edit` меняет `apps` на `["nothing-here.exe"]`, повтор `curl` тоже отвечает, в `flows(true)` строка к `1.1.1.1:80` с `rule: None` и `program` на `curl.exe`, в `flows(false)` новых строк нет; `stop`.

- [ ] **Шаг 2:** `cargo test -p router --test tcp` — ошибка компиляции (`forget`); `--test router` — новый тест проходит уже после Task 2.
- [ ] **Шаг 3:** реализовать по схеме.
- [ ] **Шаг 4:** `cargo test -p router` — PASS, `ignored` — 7. Из повышенного терминала с NekoBox: `cargo test -p router --test live -- --ignored` — PASS; нет терминала — «live не запускались» в отчёте.
- [ ] **Шаг 5:** коммит `feat(router): route connections and flows by their program`.

---

### Task 7: приложение и CLI — профиль `dbd`, `apps` в файле правила, протокол 3

**Files:**
- Modify: `crates/cli/src/lib.rs` (`PROTOCOL = 3`), `crates/cli/src/main.rs`
- Generate: `frontend/src/lib/ipc/bindings.ts`

**Interfaces:**
- Consumes: `rules::{Preset::Dbd, Rule.apps}`, `FlowRow.program`, `DirectReason::NoRule`.
- Produces: `PresetArg::Dbd` (значение `dbd`) → `Preset::Dbd`; doc-комментарий `Flows`: `--all adds flows without a rule (seen for app rules)`.

- [ ] **Шаг 1: Падающие тесты** — `crates/cli/src/main.rs`: в `every_command_sends_its_window_command` строка `(&["routes", "preset", "dbd"], Request::AddRoutePreset { preset: Preset::Dbd })`; в `routes_add_and_edit_read_one_rule_from_a_file` файл с `apps = ["Discord.exe"]` теперь даёт `Request::SaveRoute { rule, create: true }` с `rule.apps == ["Discord.exe"]`, а отказ — файл с `apps = ["Discord"]`: код 2 и `fail.error.contains("is not a program")`; ожидаемый `"protocol": 2` в тесте версии → `3`.
- [ ] **Шаг 2:** `cargo test -p cli` — FAIL.
- [ ] **Шаг 3:** реализовать; `cargo run -p zapret-hub --bin export-bindings` — в `bindings.ts` `Rule.apps`, `Preset` с `"dbd"`, `FlowRow.program`, `DirectReason` с `NoRule`.
- [ ] **Шаг 4:** `cargo test -p cli`, `cargo test -p zapret-hub --lib` — PASS. `npm --prefix frontend run check` до Task 8 падает на `apps` — ожидаемо; коммит — Rust и `bindings.ts`.
- [ ] **Шаг 5:** коммит `feat(cli): the dbd preset, apps in rule files and protocol 3`.

---

### Task 8: вкладка «Маршруты» — поле «Программы», колонка «Программа», профиль DbD

**Files:**
- Modify: `frontend/src/lib/strings.ts`, `frontend/src/lib/format.ts`, `frontend/src/lib/format.test.ts`, `frontend/src/tabs/RouteEditor.svelte`, `frontend/src/tabs/Routes.svelte`

**Interfaces:**
- Consumes: `bindings.ts` Task 7.
- Produces: `routeSummary` += `N программа/программы/программ` после доменов, если `N > 0`; `routeDraft` += `apps: lines(form.apps)`; `programName(path: string | null): string` — часть после последнего `\` или `/`, `null` → `"—"`.

Строки (`S.routes`): `apps: "Программы (имя файла .exe или полный путь, по одному в строке)"`, `appsHint: "Правило по программе без портов: пока программа запущена, весь исходящий UDP компьютера идёт через хаб, а он отправляет чужие пакеты дальше без изменений. Укажите порты, если они известны."`, `presets` += `dbd: "Dead by Daylight"`, `columns.program: "Программа"`, `reasons.NoRule: "нет правила"`.

Поведение: редактор — `textarea` программ после доменов с ошибками поля `apps`, `appsHint` при непустых `apps` и пустых портах (Ruling A); `Routes.svelte` — новое правило с `apps: []`; колонка «Программа» после «Домен»: `programName(flow.program)`, `title` — полный путь.

- [ ] **Шаг 1: Падающие тесты** — `format.test.ts`: `voice` += `apps: []` и:

```ts
it("counts programs and names a program by its file", () => {
  const dbd: Rule = { name: "Dead by Daylight", enabled: true, protocol: "both", ports: [], ips: [], domains: ["bhvrdbd.com"], apps: ["DeadByDaylight-Win64-Shipping.exe"] };
  expect(routeSummary(dbd)).toBe("TCP/UDP · 1 домен · 1 программа");
  expect(routeSummary({ ...dbd, domains: [], apps: ["a.exe", "b.exe"] })).toBe("TCP/UDP · 2 программы");
  expect(programName("C:\\Games\\DbD\\DeadByDaylight-Win64-Shipping.exe")).toBe("DeadByDaylight-Win64-Shipping.exe");
  expect(programName(null)).toBe("—");
  expect(viaLabel({ via: "Direct", reason: { kind: "NoRule" } })).toBe("напрямую: нет правила");
});
```

- [ ] **Шаг 2:** `npx --prefix frontend vitest run src/lib/format.test.ts` — FAIL.
- [ ] **Шаг 3:** реализовать по Interfaces и поведению.
- [ ] **Шаг 4:** `npm --prefix frontend test`, `npm --prefix frontend run check` (0/0), `npm --prefix frontend run build` — чисто.
- [ ] **Шаг 5:** коммит `feat(ui): programs in the rule editor and the flows`.

---

### Task 9: документация, поправки спецификации, версия 0.6.0

**Files:**
- Modify: `README.md`, `docs/cli.md`, `CHANGELOG.md`, `docs/superpowers/specs/2026-10-17-zapret-hub-router-design.md`, `Cargo.toml` (`version = "0.6.0"`), `package.json`, `frontend/package.json`, `package-lock.json`, `Cargo.lock`

Содержание:
- **README**, «Маршрутизация через SOCKS5»: убрать «Правила по программе будут позже» и пункт «`apps` пока не поддерживается»; `apps` в примере `routes.toml` и в правилах проверки (Global Constraints); профиль «Dead by Daylight»; раздел «Программы»: совпадение по имени `.exe` или полному пути; TCP решается на первом SYN, UDP — на первом пакете потока; пока программа из UDP-правила запущена, через хаб идёт весь исходящий UDP на портах правила (чужие пакеты — дальше без изменений), поэтому лучше указать порты; программа, запущенная до включения, узнаётся по таблицам Windows; потоки без правила видны в «Потоках» при «все» (не больше 500 строк).
- **docs/cli.md**: `routes preset discord-voice|discord|dbd`; строка `routes flows [--all]` — «`--all` — и потоки без правила, пойманные ради правил по программе»; пункт 18 — `apps = ["Discord"]` → код 2 и `is not a program`; «Проверка вживую» 29–33:
  29. NekoBox, «Добавить из профиля» → «Dead by Daylight», включить; запустить DbD, войти в матч: в «Потоках» строки `udp` и `tcp` с программой `DeadByDaylight-Win64-Shipping.exe`, «прокси»; в журнале NekoBox — UDP к серверам игры и CONNECT к `*.bhvrdbd.com`; матч играется.
  30. DbD закрыт: «все» — нет строк `udp` без правила (UDP не перехватывается); DbD запущен: при «все» появляются чужие потоки с правилом «—» и «напрямую: нет правила»; при «только по правилам» их нет.
  31. Включить маршрутизацию при уже запущенном DbD посреди матча, затем прогнать тест стратегии: после паузы новые потоки игры снова «прокси» с программой (владелец из таблиц Windows).
  32. Правило по полному пути к `curl.exe` (`C:\Windows\System32\curl.exe`, TCP, порт 80): `curl http://example.com` — строка «прокси»; правило с другим путём — `curl` идёт напрямую, строка только при «все».
  33. Загрузка процессора хабом при запущенном DbD и закачке в браузере (`Диспетчер задач`) — записать число; выше 10 % — записать в риски спецификации.
- **CHANGELOG** `## 0.6.0`: правила по программе (этап 3): поле `apps` (имя `.exe` или полный путь), колонка «Программа», фильтр «все» показывает потоки без правила, профиль «Dead by Daylight» и `routes preset dbd`. Протокол CLI с приложением — версии 3: CLI 0.5 получит отказ, обновить вместе.
- **Спецификация** — строки «Поправка при реализации этапа 3»: 4.6 — Ruling A (TCP: только первые SYN плюс адреса записей NAT; UDP: весь на портах правила, только пока запущена программа из правила); PID — сначала события сокетов, затем `GetExtended{Tcp,Udp}Table`, затем ожидание до 50 мс, «неизвестен» запоминается до события или закрытия сокета (то же в 4.4); `apps` оканчиваются на `.exe`; строк без правила не больше 500, в итоги они не входят; 8 — «все» = потоки с правилом и пойманные ради правил по программе; 5.2 — DbD: оба протокола, без портов; 11 — новые риски: «весь UDP через хаб, пока запущена программа», «браузер в `apps` переоткрывает фильтр на каждый адрес», «первые пакеты UDP программы, чей первый сокет — UDP, до пересборки фильтра идут напрямую».

- [ ] **Шаг 1:** правки документов и версий (`npm install --package-lock-only`; `cargo check --workspace` обновит `Cargo.lock`).
- [ ] **Шаг 2:** `cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace --exclude zapret-hub`, `cargo test -p zapret-hub --lib`, `npm --prefix frontend run check`, `npm --prefix frontend test` — всё чисто.
- [ ] **Шаг 3:** коммит `docs: router stage 3 in the README, cli.md and the spec; version 0.6.0`.

---

## Самопроверка плана

- **Покрытие (этап 3):** 4.1 дескриптор сокетов для правил по программе — Task 6 (socket loop); 4.4 PID ≤ 50 мс — 3, 6; 4.6 путь exe, кэш по PID и времени старта, имя или путь без учёта регистра, TCP на SYN, UDP на первом пакете, фильтр и «не совпавшее — сразу назад» — 2, 3, 5, 6; 5.1 `apps` — 2; 5.2 DbD — 2, 7, 8; 8 колонка «Программа», фильтр «все» — 4, 8; 9 `routes preset dbd` — 7; 10 тесты, живой тест, чек-лист — каждая задача, 6, 9; версия 0.6.0 — 9; долги этапа 2 (память отказа только для 87, уведомление проверки, список причин) — 1.
- **Решения (Ruling):** A) стоимость перехвата — выше; B) `apps` обязаны оканчиваться на `.exe`: опечатка «Discord» без расширения иначе молча не совпала бы ни с чем; C) PID сначала по таблицам Windows: без этого программы, открывшие сокеты до сеанса или до паузы теста, никогда не совпадут; D) «неизвестен» запоминается: иначе захват стоит 50 мс на каждый поток такого сокета; E) потоки без правила записываются только для пойманных ради правил по программе, до 500 строк, без итогов; F) профиль DbD — `both` без портов: портов игры спецификация не даёт, цену снимает Ruling A; G) протокол CLI 3; H) удержание SYN после `admit` для обоих путей и `Nat::forget` при неоткрывшемся фильтре.
- **Согласованность имён:** `Rule.apps`, `Preset::Dbd`, `runs`, `Compiled.{apps, running}`, `Table::{matches_flow, wants_owner, with_running, idle_apps, starts}`, `Owners::pid`, `table_pid`, `process_start`, `image_path`, `Images::{lookup, get, prune, paths}`, `DirectReason::NoRule`, `FlowRow.program`, `Stats::set_program`, `MAX_UNRULED`, `refused_for_good`, `Nat::forget`, `Waiting::covers`, `programName` — одинаково в Tasks 1–9.
- **Пропорция:** тела — только схема `Live` и тексты фильтра в тестах; остальное — сигнатуры и тесты.
