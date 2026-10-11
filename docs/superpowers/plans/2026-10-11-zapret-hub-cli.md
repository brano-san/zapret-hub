# Zapret Hub — CLI для агентов: план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Цель:** `zapret-hub-cli.exe`, через который ИИ-агент без участия пользователя смотрит состояние работающего Zapret Hub, применяет стратегии, меняет списки и флаги, запускает тесты и читает рейтинг; вывод — JSON, коды выхода постоянные.

**Архитектура:** новый пакет `crates/cli` без `tauri`. Его библиотека — протокол (`Request`, `Response`, проверка запроса до выполнения) и именованный канал (сервер с ACL, клиент); его бинарь `zapret-hub-cli` превращает аргументы clap в `Request`, отправляет по каналу и печатает JSON. Приложение (`src-tauri/src/cli.rs`) открывает канал в `setup`, на каждого клиента заводит поток и выполняет запрос теми же `commands::*`, что окно (асинхронные — через `tauri::async_runtime::block_on`), поэтому очередь, снимки, `changed()` и тексты ошибок общие. Настройка `cli_enabled`, по умолчанию выключенная, проверяется на каждый запрос.

**Стек:** Rust 1.99.0 (edition 2024). Новая зависимость — clap 4.6 с `derive` (в `Cargo.lock` встаёт 4.6.7); `windows` 0.62 (0.62.2) и `serde_json` 1.0 (1.0.151) уже есть в workspace и подключаются новым потребителям. Остальное без изменений: tauri 2.12 (2.12.3), serde 1.0, Svelte 5.57.1, vitest 5.0.3, svelte-check 4.7.6.

**Спецификация:** `docs/superpowers/specs/2026-10-11-zapret-hub-cli-design.md` (далее «спецификация»); её основа — `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md` («core»). Исполнитель читает спецификацию вместе с планом.

**База:** ветка `feat/cli` начинается от `feat/core` на `fd4bbae`. Весь код плана собран и проверен на копии дерева `d5bc9fc` (раздел «Что проверено сборкой»). Между этими коммитами — только исправления финального ревью ядра. Из файлов плана они задели `src-tauri/src/jobs.rs` (`recover`, не `finish`) и `README.md` (абзац о сборке теперь с `powershell -NoProfile -ExecutionPolicy Bypass -File tools\package.ps1`; задача 5 эту форму сохраняет).

## Общие ограничения

- Только Windows 10/11 x64. `zapret-hub-cli.exe` — бинарь отдельного пакета `crates/cli`: без `tauri` и без манифеста `requireAdministrator`. Он запускается без прав и сам ничего в системе не меняет.
- Канал `\\.\pipe\zapret-hub-cli`, один на машину. DACL `D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;<SID пользователя из токена процесса>)`, метка `S:(ML;;NWNR;;;ME)`, `PIPE_REJECT_REMOTE_CLIENTS`, `FILE_FLAG_FIRST_PIPE_INSTANCE` у первого экземпляра. Имя занято — запись в журнал и уведомление «Канал CLI занят другой программой».
- Протокол v1: одно подключение — одна строка JSON с `\n` в каждую сторону. Запрос — `{"v":1,"cmd":"…","args":{…}}`, где `cmd` и `args` — имена и аргументы IPC-команд окна из `bindings.ts`; новая команда одна — `tests_status`. Ответ — `{"ok":true,"data":…}`, `{"ok":false,"error":"…"}` или `{"ok":false,"refused":true,"error":"…"}`.
- Отказ (`refused`, код 3) значит, что приложение ничего не выполняло: CLI выключен (`the CLI is off: enable it in Zapret Hub → Settings`), запрос длиннее 1 МиБ, не разбирается или содержит неизвестную команду, другая версия (`protocol v<N> is not supported: update zapret-hub-cli.exe and Zapret Hub together`).
- `cli_enabled` по умолчанию `false` и проверяется на каждый запрос. Канал открыт всегда, переключение действует без перезапуска.
- Приложение выполняет запрос теми же `commands::*`, что окно; своей логики команд у канала нет. Каждый запрос — строка журнала: `cli: <cmd>: ok`, `cli: <cmd>: <ошибка целиком>` или `cli: refused: <причина>`.
- stdout CLI — один JSON-документ с отступами (`{"error": "…"}` при ошибке), кроме `tests report` (текст как есть). Тексты ошибок не усекаются.
- Коды выхода: 0 — успех; 1 — `ok:false` или прогон завершился ошибкой; 2 — ошибка в аргументах или нет `--yes`; 3 — приложение недоступно или отказало (не запущено, CLI выключен, канал занят дольше 10 с, другая версия); 4 — `--timeout`.
- `--yes` нужен для `custom delete`, `hosts apply`, `checks fix remove-windivert|remove-conflicts`, `discord clear-cache`, `service remove`. Без него ничего не отправляется (код 2).
- Значения перечислений в CLI — kebab-case (`ipset-exclude`), в протоколе — как в `bindings.ts`. Ключи `settings set` — как в `zapret-hub.toml` (`zapret_dir`).
- Числа: опрос `tests wait` раз в 2 с, ход в stderr строками `3/33 general (ALT2)`; повтор занятого канала до 10 с; `start` ждёт канал до 30 с; предел запроса 1 МиБ.
- Код, комментарии, тексты CLI и коммиты — на английском; `docs/`, интерфейс и уведомления — на русском. Коммит — одна строка Conventional Commits (scope `cli` или `app`), без тела и без трейлеров.
- Исполнитель работает в обычном терминале. Ни один шаг не требует прав администратора, не запускает приложение и не меняет систему: никаких `sc`, `net`, `taskkill`, `netsh`, `reg`, `schtasks`. Из команд CLI вживую запускаются только `version`, `--help` и команды, которые отказывают до отправки. Проверка с приложением — вручную по `docs/cli.md`, после слияния и не исполнителем.
- Тесты приложения — `cargo test -p zapret-hub --lib`: у бинарника `zapret-hub` манифест `requireAdministrator` и `test = false`.
- Rust-код в плане уже прошёл rustfmt. Перед каждым коммитом с Rust-кодом всё равно `cargo fmt --all` и `cargo clippy --workspace --all-targets -- -D warnings` (без замечаний).
- После изменения DTO — `cargo run -p zapret-hub --bin export-bindings` и `frontend/src/lib/ipc/bindings.ts` в том же коммите.
- В коммит идут только файлы задачи: `git add` с явными путями, без `git add -A` и `git add .`.

## На что смотреть при ревью

1. **Агент прервал CLI посреди долгой команды** (предел времени на команду, Ctrl+C). Приложение доводит операцию до конца, канал отвечает следующим клиентам. Тест: `a_client_that_leaves_before_its_answer_does_not_stop_the_server` (задача 2).
2. **Агент меняет настройки при открытом окне.** Окно сразу перечитывает их, и «Сохранить» в окне не возвращает молча прежние `socks5` или `cli_enabled`. Тест: `a StateChanged reads the settings again` в `app.test.ts` (задача 3).
3. **Два клиента сразу** (`tests wait` опрашивает, второй вызов делает `apply`). Медленный клиент не задерживает быстрого; изменение во время прогона сразу получает ошибку окна (код 1), а не ждёт. Тесты: `a_slow_client_does_not_hold_up_the_others` (задача 2), `changes_need_a_folder_and_wait_out_a_test_run` в `state.rs` (уже есть).
4. **Файл стратегии из Блокнота или PowerShell 5.1** (BOM UTF-8) **и кириллица в путях** (`C:\Запрет`). Читаются как есть. Тесты: BOM в `custom_save_reads_the_strategy_from_a_file`, `C:\Запрет` в `settings_set_changes_one_key_and_sends_all_of_them` (задача 4).
5. **CLI и приложение разных версий, чужой ответ в канале, канал не пускает** (приложение запущено под другой учётной записью). Код 3 с объяснением, а не 1 и не паника. Тесты: `v2` и неизвестная команда в `the_gate_refuses_before_anything_runs` (задача 1); `<html>` и os error 5 в `answers_become_exit_codes` (задача 4).

---

## Структура файлов

```
Cargo.toml                        + cli и clap в [workspace.dependencies]; версия 0.2.0
crates/cli/
  Cargo.toml                      библиотека cli + бинарь zapret-hub-cli
  src/lib.rs                      протокол: Request, Response, encode(), gate(), TestsStatus, Live, Outcome
  src/pipe.rs                     канал: SDDL, SID пользователя, Server (bind, serve), request()
  src/main.rs                     zapret-hub-cli: clap → Request → канал → stdout, коды выхода
  tests/pipe.rs                   канал в одном процессе, без прав
src-tauri/
  Cargo.toml                      + cli, serde_json
  src/cli.rs                      сервер: поток, проверка настройки, диспетчер, tests_status
  src/settings.rs                 Settings.cli_enabled
  src/state.rs                    итог последнего прогона, end_testing(outcome)
  src/jobs.rs                     finish() передаёт итог
  src/autostart.rs                имя задачи — из cli
  src/texts.rs                    уведомления о канале
  src/lib.rs                      mod cli; cli::start() в setup
frontend/src/
  lib/ipc/bindings.ts             (генерируется) поле cli_enabled
  lib/app.svelte.ts               StateChanged перечитывает настройки
  lib/app.test.ts
  lib/strings.ts                  строки флажка
  tabs/Settings.svelte            карточка «Командная строка»
docs/cli.md                       (RU) руководство, рецепты, ручная проверка
README.md, CHANGELOG.md           раздел о CLI, запись 0.2.0
tools/package.ps1                 архив: оба exe, README, cli.md
package.json, frontend/package.json, package-lock.json   версия 0.2.0
```

Зависимости: `cli → exec, zapret, tester`; `src-tauri → cli`. В `src-tauri` есть и крейт `cli`, и модуль `cli`: в `lib.rs` `cli::start` — это модуль, в остальных модулях `cli::Outcome` — крейт, а внутри `src-tauri/src/cli.rs` крейт пишется `::cli`.

## Что проверено сборкой

Копия дерева `d5bc9fc` с изменениями всех пяти задач, своя папка `target`, обычный терминал:
- `cargo test -p cli`: 7 тестов протокола, 16 тестов бинаря, 8 тестов канала — проходят. Задачи 1 и 2 собраны и проверены и по отдельности, только со своими зависимостями.
- `cargo test -p zapret-hub --lib`: 14 тестов проходят.
- `cargo clippy --workspace --all-targets -- -D warnings` — без замечаний; `cargo fmt --all -- --check` — без изменений.
- `cargo run -p zapret-hub --bin export-bindings` добавил в `bindings.ts` ровно две строки (поле `cli_enabled`).
- `vitest run`: 10 тестов проходят, новый сначала падал; `svelte-check`: 0 ошибок, 0 предупреждений.
- Настоящий DACL созданного канала, прочитанный обратно через `GetSecurityInfo`: `D:P(A;;FA;;;SY)(A;;FA;;;BA)(A;;0x12019f;;;S-1-5-21-…)S:AI(ML;;NWNR;;;ME)` — это `GA` и `GRGW` в терминах файла.
- Бинарь без приложения: `status` → код 3 и «not running»; `version` → `{"app": null, "cli": "0.1.0", "protocol": 1}`, код 0; `service remove` → код 2; ошибки clap → JSON с кодом 2, без ANSI-последовательностей.
- `tools/package.ps1` разобран парсером PowerShell; релизную сборку проверка не запускала.
- Внешние API сверены с исходниками версий из `Cargo.lock`:
  - `windows` 0.62.2: `CreateNamedPipeW`, `ConnectNamedPipe`, `ConvertStringSecurityDescriptorToSecurityDescriptorW`, `OpenProcessToken`, `GetTokenInformation` с `TOKEN_USER`, `ConvertSidToStringSidW`, `LocalFree`, а для теста — `GetSecurityInfo` и `ConvertSecurityDescriptorToStringSecurityDescriptorW`; features — в `crates/cli/Cargo.toml` задачи 2;
  - clap 4.6.7: derive, `ValueEnum` с `rename_all`, `global`, `requires`, `try_parse`, `Error::render`;
  - tauri 2.12.3: `async_runtime::block_on`, `Manager::state`;
  - std: `OpenOptionsExt::security_qos_flags`, `File::sync_all` (это `FlushFileBuffers`), `BufRead::skip_until`.
- `DisconnectNamedPipe` не нужен: каждое подключение получает свой экземпляр канала, и его handle закрывается после ответа.

---

### Задача 1: Протокол — `Request`, `Response` и проверка запроса

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`)
- Create: `crates/cli/Cargo.toml`, `crates/cli/src/lib.rs`
- Test: `crates/cli/src/lib.rs` (модуль `tests`; читает `frontend/src/lib/ipc/bindings.ts`)

**Interfaces:**
- Consumes: типы аргументов из крейтов, все уже с `Serialize` и `Deserialize`: `tester::run::Request { suite: tester::model::Suite, strategies: Vec<String> }`, `zapret::custom::CustomStrategy { name, cloned_from, wf: String, blocks: Vec<String> }`, `zapret::game_filter::{GameFilter { mode, tcp, udp }, GameFilterMode}`, `zapret::ipset::IpsetMode`, `zapret::fakes::FakeKind`, `zapret::user_lists::UserList`, `zapret::checks::CheckAction`.
- Produces (крейт `cli`):
  - `pub const PROTOCOL: u32 = 1`, `pub const MAX_REQUEST: usize = 1024 * 1024`, `pub const AUTOSTART_TASK: &str = "Zapret Hub"`, `pub const OFF: &str` — текст отказа выключенного CLI;
  - `pub enum Request` — 37 вариантов: 36 команд окна и `TestsStatus`; `#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]`; `SaveSettings { settings: serde_json::Value }`; `pub fn name(&self) -> String` — `cmd`;
  - `pub fn encode(request: &Request) -> String` — строка запроса без `\n`;
  - `pub struct Response { pub ok: bool, pub refused: bool, pub data: serde_json::Value, pub error: Option<String> }` и конструкторы `Response::ok(Value)`, `Response::error(String)`, `Response::refused(String)`;
  - `pub fn gate(line: &[u8], enabled: bool) -> Result<Request, Response>`;
  - `pub struct TestsStatus { pub testing: bool, pub live: Option<Live>, pub last: Option<Outcome> }`, `pub struct Live { pub request: tester::run::Request, pub current: Option<u32>, pub restoring: bool }`, `pub enum Outcome { Run { run_id: String }, Failed { error: String } }` (`untagged`).

- [ ] **Шаг 1: Пакет `cli`**

В корневом `Cargo.toml`, в `[workspace.dependencies]`, после `tester = { path = "crates/tester" }`:

```toml
cli = { path = "crates/cli" }
```

`crates/cli/Cargo.toml`:

```toml
[package]
name = "cli"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "zapret-hub-cli and its protocol: agents drive the running Zapret Hub over a local pipe."

[dependencies]
tester.workspace = true
zapret.workspace = true
serde.workspace = true
serde_json.workspace = true
```

Пакет и его библиотека называются `cli`. Бинарь `zapret-hub-cli` появится в задаче 4.

- [ ] **Шаг 2: Падающие тесты**

`crates/cli/src/lib.rs` — пока doc-комментарий и тесты:

```rust
//! The protocol between `zapret-hub-cli` and the running app (CLI spec 6): one JSON line each way
//! per connection. `cmd` and `args` are the window's IPC commands, so the app runs the same code
//! for both.

#[cfg(test)]
mod tests {
    use serde_json::json;
    use tester::model::Suite;
    use zapret::game_filter::GameFilterMode;

    use super::*;

    fn strategy() -> CustomStrategy {
        CustomStrategy {
            name: "mine".into(),
            cloned_from: "general (ALT11)".into(),
            wf: "--wf-tcp=80,443".into(),
            blocks: vec!["--filter-tcp=443 --dpi-desync=fake".into()],
        }
    }

    /// One of each request.
    fn samples() -> Vec<Request> {
        vec![
            Request::AppVersion,
            Request::GetSettings,
            Request::SaveSettings {
                settings: json!({ "zapret_dir": null, "cli_enabled": true }),
            },
            Request::GetOverview,
            Request::CheckProxy {
                addr: "127.0.0.1:2080".into(),
            },
            Request::WizardCandidates,
            Request::ListStrategies,
            Request::GetStrategy {
                name: "general (ALT11)".into(),
            },
            Request::ApplyStrategy {
                name: "general (ALT11)".into(),
            },
            Request::ApplyBest,
            Request::RestartZapret,
            Request::CloneStrategy {
                source: "general (ALT11)".into(),
                name: "mine".into(),
            },
            Request::ValidateCustom {
                strategy: strategy(),
            },
            Request::SaveCustom {
                strategy: strategy(),
                create: true,
            },
            Request::DeleteCustom {
                name: "mine".into(),
            },
            Request::StartTests {
                request: TestRequest {
                    suite: Suite::Standard,
                    strategies: vec!["general".into()],
                },
            },
            Request::TestsStatus,
            Request::CancelTests,
            Request::ListRuns,
            Request::GetRun {
                id: "2026-10-10_05-03-34".into(),
            },
            Request::RunReport {
                id: "2026-10-10_05-03-34".into(),
            },
            Request::GetFlowseal,
            Request::SetGameFilter {
                filter: GameFilter {
                    mode: GameFilterMode::All,
                    tcp: "1024-65535".into(),
                    udp: "1024-65535".into(),
                },
            },
            Request::SetIpsetMode {
                mode: IpsetMode::Any,
            },
            Request::SetCheckUpdates { enabled: false },
            Request::ReplaceFake {
                kind: FakeKind::Discord,
                source: "quic_initial_www_google_com".into(),
            },
            Request::AddListEntry {
                list: UserList::General,
                entry: "example.com".into(),
            },
            Request::RemoveListEntry {
                list: UserList::IpsetExclude,
                entry: "203.0.113.0/24".into(),
            },
            Request::UpdateIpset,
            Request::HostsPlan,
            Request::ApplyHosts,
            Request::CheckFlowsealUpdate,
            Request::SystemChecks,
            Request::FixCheck {
                action: CheckAction::RemoveWinDivert,
            },
            Request::ClearDiscordCache,
            Request::RemoveService,
            Request::QuickCheck,
        ]
    }

    /// Window commands the CLI leaves out (spec 6).
    const WINDOW_ONLY: [&str; 6] = [
        "check_folder",
        "pick_folder",
        "open_url",
        "open_folder",
        "test_live",
        "take_notices",
    ];

    /// `("cmd", { a, b })` → the argument names in `bindings.ts`, sorted.
    fn ipc_arguments(bindings: &str, cmd: &str) -> Option<Vec<String>> {
        let call = format!("(\"{cmd}\"");
        let rest = &bindings[bindings.find(&call)? + call.len()..];
        let rest = &rest[..rest.find(')')?];
        let mut args: Vec<String> = rest
            .trim_start_matches(',')
            .trim()
            .trim_matches(['{', '}'])
            .split(',')
            .map(|arg| arg.trim().to_owned())
            .filter(|arg| !arg.is_empty())
            .collect();
        args.sort();
        Some(args)
    }

    #[test]
    fn every_request_is_a_window_command_with_its_arguments() {
        let bindings = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../frontend/src/lib/ipc/bindings.ts"
        ))
        .unwrap();
        let mut names = Vec::new();
        for request in samples() {
            let value = serde_json::to_value(&request).unwrap();
            let cmd = value["cmd"].as_str().unwrap().to_owned();
            assert_eq!(request.name(), cmd);
            if cmd != "tests_status" {
                let mut args: Vec<String> = value
                    .get("args")
                    .and_then(Value::as_object)
                    .map(|args| args.keys().cloned().collect())
                    .unwrap_or_default();
                args.sort();
                assert_eq!(
                    ipc_arguments(&bindings, &cmd),
                    Some(args),
                    "{cmd} in bindings.ts"
                );
            }
            names.push(cmd);
        }
        // And the other way: a new window command needs a request or a place in WINDOW_ONLY.
        // `__TAURI_INVOKE("cmd"` or `__TAURI_INVOKE<T>("cmd"`; the import line does not count.
        for chunk in bindings
            .split("__TAURI_INVOKE")
            .filter(|chunk| chunk.starts_with(['(', '<']))
        {
            let start = chunk.find("(\"").unwrap() + 2;
            let cmd = &chunk[start..start + chunk[start..].find('"').unwrap()];
            assert!(
                names.iter().any(|name| name == cmd) || WINDOW_ONLY.contains(&cmd),
                "{cmd} has no request"
            );
        }
    }

    #[test]
    fn a_request_line_carries_the_version_first() {
        assert_eq!(
            encode(&Request::ApplyStrategy {
                name: "general (ALT11)".into()
            }),
            r#"{"v":1,"cmd":"apply_strategy","args":{"name":"general (ALT11)"}}"#
        );
        assert_eq!(encode(&Request::ApplyBest), r#"{"v":1,"cmd":"apply_best"}"#);
    }

    #[test]
    fn every_request_comes_through_the_gate_as_it_left() {
        for request in samples() {
            assert_eq!(gate(encode(&request).as_bytes(), true), Ok(request));
        }
    }

    fn refusal(line: &[u8], enabled: bool) -> String {
        let response = gate(line, enabled).unwrap_err();
        assert!(!response.ok && response.refused, "{response:?}");
        response.error.unwrap()
    }

    #[test]
    fn the_gate_refuses_before_anything_runs() {
        let apply = encode(&Request::ApplyBest);
        assert_eq!(refusal(apply.as_bytes(), false), OFF);
        assert_eq!(refusal(b"not json", false), OFF, "off comes first");
        let turn_on = encode(&Request::SaveSettings {
            settings: json!({ "cli_enabled": true }),
        });
        assert_eq!(
            refusal(turn_on.as_bytes(), false),
            OFF,
            "the CLI cannot turn itself on"
        );
        assert!(refusal(b"not json", true).starts_with("the request does not parse"));
        assert!(
            refusal(br#"{"v":1,"cmd":"format_disk"}"#, true).contains("format_disk"),
            "an unknown command is named"
        );
        assert!(refusal(br#"{"cmd":"apply_best"}"#, true).contains("`v`"));
        assert_eq!(
            refusal(br#"{"v":2,"cmd":"format_disk"}"#, true),
            "protocol v2 is not supported: update zapret-hub-cli.exe and Zapret Hub together"
        );
    }

    #[test]
    fn the_gate_takes_1_mib_and_not_a_byte_more() {
        let padded = |len: usize| {
            let mut line = encode(&Request::ApplyBest).into_bytes();
            line.resize(len, b' ');
            line
        };
        assert_eq!(gate(&padded(MAX_REQUEST), true), Ok(Request::ApplyBest));
        assert_eq!(
            refusal(&padded(MAX_REQUEST + 1), true),
            "the request is longer than 1 MiB"
        );
    }

    #[test]
    fn answers_read_back_as_they_were_sent() {
        for (response, line) in [
            (
                Response::ok(json!({ "app": "0.2.0" })),
                r#"{"ok":true,"data":{"app":"0.2.0"}}"#,
            ),
            (Response::ok(Value::Null), r#"{"ok":true}"#),
            (
                Response::error("the zapret folder is not set".into()),
                r#"{"ok":false,"error":"the zapret folder is not set"}"#,
            ),
            (
                Response::refused(OFF.into()),
                r#"{"ok":false,"refused":true,"error":"the CLI is off: enable it in Zapret Hub → Settings"}"#,
            ),
        ] {
            assert_eq!(serde_json::to_string(&response).unwrap(), line);
            assert_eq!(serde_json::from_str::<Response>(line).unwrap(), response);
        }
    }

    #[test]
    fn a_run_ends_saved_or_failed() {
        let saved: Outcome = serde_json::from_str(r#"{"run_id":"2026-10-10_05-03-34"}"#).unwrap();
        assert_eq!(
            saved,
            Outcome::Run {
                run_id: "2026-10-10_05-03-34".into()
            }
        );
        let failed: Outcome =
            serde_json::from_str(r#"{"error":"curl.exe was not found"}"#).unwrap();
        assert_eq!(
            serde_json::to_string(&failed).unwrap(),
            r#"{"error":"curl.exe was not found"}"#
        );
    }
}
```

Что проверяют тесты:
- `every_request_is_a_window_command_with_its_arguments` — `cmd` и имена `args` каждого образца совпадают с вызовом в `bindings.ts`; и обратно: у каждой команды окна есть запрос или место в `WINDOW_ONLY` (спецификация 6, решение 5);
- `a_request_line_carries_the_version_first` — точная строка запроса;
- `every_request_comes_through_the_gate_as_it_left` — `encode` → `gate` возвращает тот же запрос;
- `the_gate_refuses_before_anything_runs` — выключенный CLI отказывает раньше всего, в том числе на попытку включить себя; дальше — мусор, неизвестная команда, нет `v`, чужая версия;
- `the_gate_takes_1_mib_and_not_a_byte_more` — граница 1 МиБ;
- `answers_read_back_as_they_were_sent` — форма трёх видов ответа;
- `a_run_ends_saved_or_failed` — `Outcome` без тега.

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p cli`
Expected: FAIL при компиляции: в модуле нет `Request`, `CustomStrategy`, `encode`, `gate` и остального (ошибки E0412, E0425, E0433).

- [ ] **Шаг 4: Протокол**

В `crates/cli/src/lib.rs` между doc-комментарием и `#[cfg(test)]`:

```rust
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tester::run::Request as TestRequest;
use zapret::checks::CheckAction;
use zapret::custom::CustomStrategy;
use zapret::fakes::FakeKind;
use zapret::game_filter::GameFilter;
use zapret::ipset::IpsetMode;
use zapret::user_lists::UserList;

/// Grows only with an incompatible change; a new command is not one.
pub const PROTOCOL: u32 = 1;

/// A longer request is refused (spec 5).
pub const MAX_REQUEST: usize = 1024 * 1024;

/// The Task Scheduler entry the app's autostart makes; `zapret-hub-cli start` runs it.
pub const AUTOSTART_TASK: &str = "Zapret Hub";

/// The answer to everything while «Управление из командной строки» is off (spec 5).
pub const OFF: &str = "the CLI is off: enable it in Zapret Hub → Settings";

/// One command: the variant in snake_case and its fields are an IPC command of the window and its
/// arguments (`bindings.ts`). `tests_status` is the only one the window lacks.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "cmd", content = "args", rename_all = "snake_case")]
pub enum Request {
    AppVersion,
    GetSettings,
    /// The app's `Settings` as JSON: the type lives in the app, which checks it on arrival.
    SaveSettings {
        settings: Value,
    },
    GetOverview,
    CheckProxy {
        addr: String,
    },
    WizardCandidates,
    ListStrategies,
    GetStrategy {
        name: String,
    },
    ApplyStrategy {
        name: String,
    },
    ApplyBest,
    RestartZapret,
    CloneStrategy {
        source: String,
        name: String,
    },
    ValidateCustom {
        strategy: CustomStrategy,
    },
    SaveCustom {
        strategy: CustomStrategy,
        create: bool,
    },
    DeleteCustom {
        name: String,
    },
    StartTests {
        request: TestRequest,
    },
    TestsStatus,
    CancelTests,
    ListRuns,
    GetRun {
        id: String,
    },
    RunReport {
        id: String,
    },
    GetFlowseal,
    SetGameFilter {
        filter: GameFilter,
    },
    SetIpsetMode {
        mode: IpsetMode,
    },
    SetCheckUpdates {
        enabled: bool,
    },
    ReplaceFake {
        kind: FakeKind,
        source: String,
    },
    AddListEntry {
        list: UserList,
        entry: String,
    },
    RemoveListEntry {
        list: UserList,
        entry: String,
    },
    UpdateIpset,
    HostsPlan,
    ApplyHosts,
    CheckFlowsealUpdate,
    SystemChecks,
    FixCheck {
        action: CheckAction,
    },
    ClearDiscordCache,
    RemoveService,
    QuickCheck,
}

impl Request {
    /// The `cmd` it travels as: the journal names it.
    pub fn name(&self) -> String {
        serde_json::to_value(self)
            .ok()
            .and_then(|value| value["cmd"].as_str().map(str::to_owned))
            .unwrap_or_default()
    }
}

#[derive(Serialize)]
struct Outgoing<'a> {
    v: u32,
    #[serde(flatten)]
    request: &'a Request,
}

/// The line the CLI sends, without its `\n`: `{"v":1,"cmd":"apply_strategy","args":{"name":"…"}}`.
pub fn encode(request: &Request) -> String {
    serde_json::to_string(&Outgoing {
        v: PROTOCOL,
        request,
    })
    .expect("a request is plain data")
}

/// `{"ok":true,"data":…}`, `{"ok":false,"error":"…"}`, or `{"ok":false,"refused":true,"error":"…"}`
/// when the app ran nothing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Response {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub refused: bool,
    #[serde(default, skip_serializing_if = "Value::is_null")]
    pub data: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

impl Response {
    pub fn ok(data: Value) -> Self {
        Self {
            ok: true,
            refused: false,
            data,
            error: None,
        }
    }

    /// The command ran and failed: the whole chain of causes.
    pub fn error(error: String) -> Self {
        Self {
            ok: false,
            refused: false,
            data: Value::Null,
            error: Some(error),
        }
    }

    pub fn refused(error: String) -> Self {
        Self {
            refused: true,
            ..Self::error(error)
        }
    }
}

/// What the app does with a request line before running anything (spec 5, 6): refused while the
/// CLI is off, over 1 MiB, unparsable (an unknown command included) or of another protocol version.
pub fn gate(line: &[u8], enabled: bool) -> Result<Request, Response> {
    if !enabled {
        return Err(Response::refused(OFF.to_owned()));
    }
    if line.len() > MAX_REQUEST {
        return Err(Response::refused(
            "the request is longer than 1 MiB".to_owned(),
        ));
    }
    #[derive(Deserialize)]
    struct Version {
        v: u32,
    }
    let unparsable =
        |err: serde_json::Error| Response::refused(format!("the request does not parse: {err}"));
    // The version first: a newer CLI's unknown command must read as a version mismatch.
    let Version { v } = serde_json::from_slice(line).map_err(unparsable)?;
    if v != PROTOCOL {
        return Err(Response::refused(format!(
            "protocol v{v} is not supported: update zapret-hub-cli.exe and Zapret Hub together"
        )));
    }
    serde_json::from_slice(line).map_err(unparsable)
}

/// `tests_status` (spec 7.1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TestsStatus {
    pub testing: bool,
    pub live: Option<Live>,
    pub last: Option<Outcome>,
}

/// What the CLI reads of `test_live`'s run in progress; the app sends all of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Live {
    pub request: TestRequest,
    /// The strategy under test, from 0; `None` before the first and while restoring.
    pub current: Option<u32>,
    pub restoring: bool,
}

/// How the newest run since the app started ended: saved as `run_id`, or failed before a result.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Outcome {
    Run { run_id: String },
    Failed { error: String },
}
```

Почему так:
- `SaveSettings` несёт `serde_json::Value`: тип `Settings` живёт в `src-tauri`, а `cli` от приложения не зависит. Приложение разбирает настройки на входе (задача 3).
- `v` добавляет обёртка `Outgoing` с `#[serde(flatten)]`. Смежно-тегированный enum при разборе лишний ключ `v` пропускает, это проверяет `every_request_comes_through_the_gate_as_it_left`.
- `gate` читает `v` до полного разбора: неизвестная команда от CLI новой версии выглядит как несовпадение версий, а не как мусор.

- [ ] **Шаг 5: Тесты проходят**

Run: `cargo test -p cli`
Expected: PASS, 7 тестов.

- [ ] **Шаг 6: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 7: Коммит**

```bash
git add Cargo.toml Cargo.lock crates/cli/Cargo.toml crates/cli/src/lib.rs
git commit -m "feat(cli): add the agent CLI protocol and its request gate"
```

---

### Задача 2: Канал — сервер с ACL и клиент

**Files:**
- Modify: `crates/cli/Cargo.toml` (+ `windows`), `crates/cli/src/lib.rs` (+ `pub mod pipe;`)
- Create: `crates/cli/src/pipe.rs`
- Test: `crates/cli/tests/pipe.rs`

**Interfaces:**
- Consumes: `cli::MAX_REQUEST` (задача 1).
- Produces (модуль `cli::pipe`):
  - `pub const NAME: &str = r"\\.\pipe\zapret-hub-cli"`;
  - `pub fn sddl(user: &str) -> String`, `pub fn current_user_sid() -> std::io::Result<String>`;
  - `pub struct Server`; `Server::bind(name: &str) -> io::Result<Server>` — имя занято → os error 5; `Server::serve(self, handler: impl Fn(&[u8]) -> String + Send + Sync + 'static) -> io::Error` — обработчик получает строку запроса без `\n` (не длиннее `MAX_REQUEST + 1` байт) и возвращает строку ответа без `\n`; `serve` возвращается только с ошибкой;
  - `pub fn request(name: &str, line: &[u8], busy: Duration) -> io::Result<Vec<u8>>` — отправляет строку и возвращает строку ответа без `\n`; сервера нет → os error 2; все экземпляры заняты дольше `busy` → os error 231.

- [ ] **Шаг 1: Зависимость**

В `crates/cli/Cargo.toml`, в `[dependencies]`, после `serde_json.workspace = true`:

```toml
windows = { workspace = true, features = ["Win32_Foundation", "Win32_Security", "Win32_Security_Authorization", "Win32_Storage_FileSystem", "Win32_System_IO", "Win32_System_Pipes", "Win32_System_Threading"] }
```

- [ ] **Шаг 2: Падающие тесты**

`crates/cli/tests/pipe.rs`:

```rust
//! The pipe in one process, without elevation: a pipe made by the current user changes nothing
//! in the system and goes away with the test.

use std::os::windows::io::AsRawHandle as _;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{Duration, Instant};

use cli::pipe::{self, Server};

const BUSY: Duration = Duration::from_secs(10);

/// Tests run in parallel: a name each.
fn unique() -> String {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    format!(
        r"\\.\pipe\zapret-hub-cli-test-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    )
}

fn serve(name: &str, handler: impl Fn(&[u8]) -> String + Send + Sync + 'static) {
    let server = Server::bind(name).unwrap();
    std::thread::spawn(move || server.serve(handler));
}

#[test]
fn a_client_gets_the_answer_to_its_line() {
    let name = unique();
    serve(&name, |line| String::from_utf8_lossy(line).to_uppercase());
    assert_eq!(pipe::request(&name, b"hello", BUSY).unwrap(), b"HELLO");
    assert_eq!(
        pipe::request(&name, "ещё".as_bytes(), BUSY).unwrap(),
        "ЕЩЁ".as_bytes(),
        "one connection per request"
    );
}

#[test]
fn a_second_server_cannot_take_the_name() {
    let name = unique();
    let _first = Server::bind(&name).unwrap();
    let Err(second) = Server::bind(&name) else {
        panic!("FILE_FLAG_FIRST_PIPE_INSTANCE let a second server in");
    };
    assert_eq!(second.raw_os_error(), Some(5), "{second}");
}

#[test]
fn no_server_means_file_not_found() {
    let err = pipe::request(&unique(), b"{}", BUSY).unwrap_err();
    assert_eq!(err.raw_os_error(), Some(2), "{err}");
}

#[test]
fn a_slow_client_does_not_hold_up_the_others() {
    let name = unique();
    serve(&name, |line| {
        if line == b"slow" {
            std::thread::sleep(Duration::from_secs(2));
        }
        String::from_utf8_lossy(line).into_owned()
    });
    let slow = {
        let name = name.clone();
        std::thread::spawn(move || pipe::request(&name, b"slow", BUSY).unwrap())
    };
    std::thread::sleep(Duration::from_millis(300));
    let started = Instant::now();
    assert_eq!(pipe::request(&name, b"fast", BUSY).unwrap(), b"fast");
    assert!(
        started.elapsed() < Duration::from_secs(1),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(slow.join().unwrap(), b"slow");
}

#[test]
fn a_client_that_leaves_before_its_answer_does_not_stop_the_server() {
    use std::io::Write as _;

    let name = unique();
    serve(&name, |line| {
        std::thread::sleep(Duration::from_millis(300));
        String::from_utf8_lossy(line).into_owned()
    });
    let mut gone = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&name)
        .unwrap();
    gone.write_all(b"bye\n").unwrap();
    drop(gone);
    std::thread::sleep(Duration::from_millis(500));
    assert_eq!(
        pipe::request(&name, b"still here", BUSY).unwrap(),
        b"still here"
    );
}

#[test]
fn an_oversized_line_is_cut_and_still_answered() {
    let name = unique();
    serve(&name, |line| line.len().to_string());
    let line = vec![b'a'; cli::MAX_REQUEST + 100_000];
    assert_eq!(
        pipe::request(&name, &line, BUSY).unwrap(),
        (cli::MAX_REQUEST + 1).to_string().as_bytes()
    );
}

#[test]
fn the_descriptor_names_this_user() {
    let sid = pipe::current_user_sid().unwrap();
    assert!(sid.starts_with("S-1-5-"), "{sid}");
    assert_eq!(
        pipe::sddl(&sid),
        format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{sid})S:(ML;;NWNR;;;ME)")
    );
}

#[test]
fn the_pipe_lets_in_system_administrators_and_this_user_only() {
    use windows::Win32::Foundation::{HANDLE, HLOCAL, LocalFree};
    use windows::Win32::Security::Authorization::{
        ConvertSecurityDescriptorToStringSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
        SE_KERNEL_OBJECT,
    };
    use windows::Win32::Security::{
        DACL_SECURITY_INFORMATION, LABEL_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR,
    };
    use windows::core::PWSTR;

    let name = unique();
    serve(&name, |_| String::new());
    let client = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(&name)
        .unwrap();
    let what = DACL_SECURITY_INFORMATION | LABEL_SECURITY_INFORMATION;
    // SAFETY: plain Win32 calls on an open handle; both buffers are freed.
    let sddl = unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        GetSecurityInfo(
            HANDLE(client.as_raw_handle()),
            SE_KERNEL_OBJECT,
            what,
            None,
            None,
            None,
            None,
            Some(&mut descriptor),
        )
        .ok()
        .unwrap();
        let mut text = PWSTR::null();
        ConvertSecurityDescriptorToStringSecurityDescriptorW(
            descriptor,
            SDDL_REVISION_1,
            what,
            &mut text,
            None,
        )
        .unwrap();
        let sddl = text.to_string().unwrap();
        LocalFree(Some(HLOCAL(text.0.cast())));
        LocalFree(Some(HLOCAL(descriptor.0)));
        sddl
    };
    let sid = pipe::current_user_sid().unwrap();
    assert!(sddl.contains(&format!(";;;{sid})")), "{sddl}");
    assert!(sddl.contains(";;;SY)") && sddl.contains(";;;BA)"), "{sddl}");
    assert_eq!(sddl.matches("(A;").count(), 3, "nobody else: {sddl}");
    assert!(sddl.contains("(ML;;NWNR;;;ME)"), "{sddl}");
}
```

В `crates/cli/src/lib.rs` после doc-комментария:

```rust
pub mod pipe;
```

`crates/cli/src/pipe.rs` — пока только doc-комментарий:

```rust
//! The named pipe between `zapret-hub-cli` and the app (CLI spec 4, 5): local only, one request
//! line and one answer line per connection, a thread per client.
```

Тесты создают каналы `\\.\pipe\zapret-hub-cli-test-<pid>-<n>` от имени текущего пользователя. Это не требует прав и ничего не меняет в системе: канал исчезает вместе с процессом теста.

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p cli --test pipe`
Expected: FAIL при компиляции: `unresolved import` — в `cli::pipe` нет `Server`.

- [ ] **Шаг 4: Канал**

`crates/cli/src/pipe.rs` целиком:

```rust
//! The named pipe between `zapret-hub-cli` and the app (CLI spec 4, 5): local only, one request
//! line and one answer line per connection, a thread per client.

use std::fs::{File, OpenOptions};
use std::io::{self, BufRead as _, BufReader, Read as _, Write as _};
use std::os::windows::fs::OpenOptionsExt as _;
use std::os::windows::io::{AsRawHandle as _, FromRawHandle as _, OwnedHandle};
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{
    ERROR_PIPE_BUSY, ERROR_PIPE_CONNECTED, HANDLE, HLOCAL, LocalFree,
};
use windows::Win32::Security::Authorization::{
    ConvertSidToStringSidW, ConvertStringSecurityDescriptorToSecurityDescriptorW, SDDL_REVISION_1,
};
use windows::Win32::Security::{
    GetTokenInformation, PSECURITY_DESCRIPTOR, SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER,
    TokenUser,
};
use windows::Win32::Storage::FileSystem::{
    FILE_FLAG_FIRST_PIPE_INSTANCE, PIPE_ACCESS_DUPLEX, SECURITY_IDENTIFICATION,
};
use windows::Win32::System::Pipes::{
    ConnectNamedPipe, CreateNamedPipeW, PIPE_READMODE_BYTE, PIPE_REJECT_REMOTE_CLIENTS,
    PIPE_TYPE_BYTE, PIPE_UNLIMITED_INSTANCES, PIPE_WAIT,
};
use windows::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};
use windows::core::{HSTRING, PWSTR};

use crate::MAX_REQUEST;

/// One per machine, like the zapret service (spec 12, decision 12).
pub const NAME: &str = r"\\.\pipe\zapret-hub-cli";

/// SYSTEM and Administrators may do anything, `user` may read and write, nobody else may open it;
/// Medium integrity keeps sandboxed (Low) programs from writing and reading (spec 5).
pub fn sddl(user: &str) -> String {
    format!("D:P(A;;GA;;;SY)(A;;GA;;;BA)(A;;GRGW;;;{user})S:(ML;;NWNR;;;ME)")
}

/// `S-1-5-21-…` of the user this process runs as: from its token, so an elevated app still names
/// the user who started it.
pub fn current_user_sid() -> io::Result<String> {
    // SAFETY: plain Win32 calls with buffers of the sizes they report; the token is closed and the
    // string freed before this returns.
    unsafe {
        let mut token = HANDLE::default();
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)?;
        let owned = OwnedHandle::from_raw_handle(token.0);
        let mut size = 0;
        // Fails with ERROR_INSUFFICIENT_BUFFER, having reported the size.
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut size);
        // u64 cells: TOKEN_USER starts with a pointer and must be aligned for it.
        let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
        GetTokenInformation(
            token,
            TokenUser,
            Some(buffer.as_mut_ptr().cast()),
            size,
            &mut size,
        )?;
        drop(owned);
        let user = &*buffer.as_ptr().cast::<TOKEN_USER>();
        let mut text = PWSTR::null();
        ConvertSidToStringSidW(user.User.Sid, &mut text)?;
        let sid = text.to_string();
        LocalFree(Some(HLOCAL(text.0.cast())));
        sid.map_err(io::Error::other)
    }
}

/// A new instance of the pipe. With `first` it fails with ERROR_ACCESS_DENIED when the name exists
/// already, whoever made it (spec 5).
fn instance(name: &HSTRING, sddl: &HSTRING, first: bool) -> io::Result<OwnedHandle> {
    // SAFETY: the descriptor lives until CreateNamedPipeW, which copies it, has returned.
    unsafe {
        let mut descriptor = PSECURITY_DESCRIPTOR::default();
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            sddl,
            SDDL_REVISION_1,
            &mut descriptor,
            None,
        )?;
        let attributes = SECURITY_ATTRIBUTES {
            nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
            lpSecurityDescriptor: descriptor.0,
            ..Default::default()
        };
        let mut open = PIPE_ACCESS_DUPLEX;
        if first {
            open |= FILE_FLAG_FIRST_PIPE_INSTANCE;
        }
        let pipe = CreateNamedPipeW(
            name,
            open,
            PIPE_TYPE_BYTE | PIPE_READMODE_BYTE | PIPE_WAIT | PIPE_REJECT_REMOTE_CLIENTS,
            PIPE_UNLIMITED_INSTANCES,
            64 * 1024,
            64 * 1024,
            0,
            Some(&attributes),
        );
        let created = if pipe.is_invalid() {
            Err(io::Error::last_os_error())
        } else {
            Ok(OwnedHandle::from_raw_handle(pipe.0))
        };
        LocalFree(Some(HLOCAL(descriptor.0)));
        created
    }
}

/// Waits for a client to open this instance.
fn connect(pipe: &OwnedHandle) -> io::Result<()> {
    // SAFETY: a valid pipe handle, used synchronously (no OVERLAPPED).
    match unsafe { ConnectNamedPipe(HANDLE(pipe.as_raw_handle()), None) } {
        // A client that opened the instance before the call is connected already.
        Err(err) if err.code() != ERROR_PIPE_CONNECTED.to_hresult() => Err(err.into()),
        _ => Ok(()),
    }
}

/// The app's end. The first instance is made by `bind`, so a taken name is known before serving.
pub struct Server {
    name: HSTRING,
    sddl: HSTRING,
    next: OwnedHandle,
}

impl Server {
    /// Fails with ERROR_ACCESS_DENIED (os error 5) when another program holds `name`.
    pub fn bind(name: &str) -> io::Result<Self> {
        let name = HSTRING::from(name);
        let sddl = HSTRING::from(sddl(&current_user_sid()?));
        let next = instance(&name, &sddl, true)?;
        Ok(Self { name, sddl, next })
    }

    /// Answers clients until making an instance fails, and returns that error. `handler` gets the
    /// request line without its `\n` and returns the answer line. Each client has its own thread:
    /// one that never sends its line holds only that thread (spec 11).
    pub fn serve(mut self, handler: impl Fn(&[u8]) -> String + Send + Sync + 'static) -> io::Error {
        let handler = Arc::new(handler);
        loop {
            let connected = connect(&self.next);
            // The next instance is up before this one is handed over: the name never goes free.
            let client = match instance(&self.name, &self.sddl, false) {
                Ok(fresh) => std::mem::replace(&mut self.next, fresh),
                Err(err) => return err,
            };
            // A client that left before the connection (ERROR_NO_DATA): nobody to answer.
            if connected.is_err() {
                continue;
            }
            let handler = Arc::clone(&handler);
            std::thread::spawn(move || {
                // An error here means the client went away: nobody to tell.
                let _ = answer(File::from(client), &*handler);
            });
        }
    }
}

/// Reads one line, keeping at most `MAX_REQUEST + 1` bytes of it, writes the handler's answer and
/// waits until the client has read it.
fn answer(pipe: File, handler: &impl Fn(&[u8]) -> String) -> io::Result<()> {
    let mut reader = BufReader::new(&pipe);
    let mut line = Vec::new();
    (&mut reader)
        .take(MAX_REQUEST as u64 + 1)
        .read_until(b'\n', &mut line)?;
    if line.last() == Some(&b'\n') {
        line.pop();
    } else if line.len() > MAX_REQUEST {
        // The client writes the rest before it reads: drain it, or both ends wait forever.
        reader.skip_until(b'\n')?;
    }
    let mut response = handler(&line);
    response.push('\n');
    (&pipe).write_all(response.as_bytes())?;
    // FlushFileBuffers: closing the handle before the client has read the answer would drop it.
    pipe.sync_all()
}

/// The CLI's end: sends `line` and returns the answer line. While every instance is taken the
/// pipe is busy; that is retried for up to `busy`.
pub fn request(name: &str, line: &[u8], busy: Duration) -> io::Result<Vec<u8>> {
    let since = Instant::now();
    let pipe = loop {
        // Identification only: whoever holds the name cannot act as this user.
        match OpenOptions::new()
            .read(true)
            .write(true)
            .security_qos_flags(SECURITY_IDENTIFICATION.0)
            .open(name)
        {
            Err(err)
                if err.raw_os_error() == Some(ERROR_PIPE_BUSY.0 as i32)
                    && since.elapsed() < busy =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            opened => break opened?,
        }
    };
    let mut message = line.to_vec();
    message.push(b'\n');
    (&pipe).write_all(&message)?;
    let mut answer = Vec::new();
    BufReader::new(&pipe).read_until(b'\n', &mut answer)?;
    if answer.last() == Some(&b'\n') {
        answer.pop();
    }
    Ok(answer)
}
```

Почему так:
- Каждое подключение получает свой экземпляр канала. Следующий создаётся до того, как текущий уходит потоку клиента, поэтому имя не освобождается ни на миг, а `DisconnectNamedPipe` не нужен: handle закрывается после ответа.
- `File::sync_all` на канале — это `FlushFileBuffers`: ответ не теряется, даже если клиент читает позже, чем сервер закрывает handle.
- Слишком длинную строку сервер дочитывает до `\n` (`skip_until`). Иначе клиент, который пишет всё до того, как читать, и сервер ждали бы друг друга вечно. Обработчик получает `MAX_REQUEST + 1` байт, и `gate` отказывает по размеру.
- Клиент открывает канал с `SECURITY_IDENTIFICATION`: кто бы ни держал имя, действовать от имени клиента он не может.

- [ ] **Шаг 5: Тесты проходят**

Run: `cargo test -p cli`
Expected: PASS: 7 тестов протокола и 8 тестов канала (около 2 с).

- [ ] **Шаг 6: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 7: Коммит**

```bash
git add Cargo.lock crates/cli/Cargo.toml crates/cli/src/lib.rs crates/cli/src/pipe.rs crates/cli/tests/pipe.rs
git commit -m "feat(cli): add the local named pipe with its ACL, server and client"
```

---

### Задача 3: Канал в приложении, настройка и флажок

**Files:**
- Create: `src-tauri/src/cli.rs`
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/src/settings.rs`, `src-tauri/src/state.rs`, `src-tauri/src/jobs.rs`, `src-tauri/src/autostart.rs`, `src-tauri/src/texts.rs`
- Modify: `frontend/src/lib/ipc/bindings.ts` (генерируется), `frontend/src/lib/app.svelte.ts`, `frontend/src/lib/strings.ts`, `frontend/src/tabs/Settings.svelte`
- Test: модули `tests` в `src-tauri/src/settings.rs`, `src-tauri/src/state.rs`, `src-tauri/src/cli.rs`; `frontend/src/lib/app.test.ts`

**Interfaces:**
- Consumes: `cli::{gate, Request, Response, Outcome, TestsStatus, Live, AUTOSTART_TASK}` (задача 1), `cli::pipe::{NAME, Server}` (задача 2); команды окна `commands::*` — без изменений.
- Produces:
  - канал `\\.\pipe\zapret-hub-cli`, пока приложение работает; ответы — `cli::Response`;
  - ответ на `tests_status` — `{"testing": bool, "live": LiveRun | null, "last": {"run_id": "…"} | {"error": "…"} | null}`; его читает `cli::TestsStatus` (задача 4);
  - `Settings.cli_enabled: bool` в `zapret-hub.toml`, `get_settings` и `save_settings`; по умолчанию `false`;
  - `AppState::end_testing(&self, outcome: cli::Outcome)`, `AppState::last_outcome(&self) -> Option<cli::Outcome>`.

- [ ] **Шаг 1: Зависимости**

В `src-tauri/Cargo.toml`, в `[dependencies]`: после `exec.workspace = true` добавить `cli.workspace = true`, после `serde.workspace = true` — `serde_json.workspace = true`.

- [ ] **Шаг 2: Падающие тесты приложения**

`src-tauri/src/settings.rs`, в тесте `a_missing_file_gives_the_defaults` после `assert!(Settings::default().start_minimized);`:

```rust
        assert!(
            !Settings::default().cli_enabled,
            "off until the user turns it on"
        );
```

там же, в тесте `settings_survive_a_save`, в литерал `Settings { … }` после `wizard_done: true,`:

```rust
            cli_enabled: true,
```

`src-tauri/src/state.rs`, в тесте `changes_need_a_folder_and_wait_out_a_test_run` строку `state.end_testing();` заменить на:

```rust
        state.end_testing(cli::Outcome::Failed {
            error: "canceled".into(),
        });
```

и после этого теста, в конце модуля `tests`, добавить:

```rust
    #[test]
    fn a_run_ends_with_its_outcome_in_place() {
        use tester::model::Suite;
        use tester::run::Request;

        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        assert!(state.begin_testing());
        state.set_live(Some(LiveRun {
            request: Request {
                suite: Suite::Standard,
                strategies: vec!["general".into()],
            },
            current: Some(0),
            restoring: false,
            results: Vec::new(),
        }));
        let saved = cli::Outcome::Run {
            run_id: "2026-10-11_10-00-00".into(),
        };
        state.end_testing(saved.clone());
        assert!(!state.testing());
        assert_eq!(state.live(), None);
        assert_eq!(state.last_outcome(), Some(saved));
    }
```

`src-tauri/src/cli.rs` — пока doc-комментарий и тест:

```rust
//! The app's end of `zapret-hub-cli` (CLI spec 4–6, 8): a pipe served from its own thread, each
//! request run by the same `commands::*` the window calls, so the queue, the snapshots, `changed()`
//! and the error texts are the window's.

#[cfg(test)]
mod tests {
    use tester::model::Suite;
    use tester::run::Request as TestRequest;

    use super::*;
    use crate::dto::LiveRun;
    use crate::settings::Settings;
    use crate::state::Paths;

    #[test]
    fn tests_status_is_what_the_cli_reads() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::new(
            Paths {
                exe_dir: dir.path().to_path_buf(),
            },
            Settings::default(),
        );
        let read = |state: &AppState| -> ::cli::TestsStatus {
            serde_json::from_value(tests_status(state)).unwrap()
        };
        assert_eq!(
            read(&state),
            ::cli::TestsStatus {
                testing: false,
                live: None,
                last: None
            }
        );
        let request = TestRequest {
            suite: Suite::Standard,
            strategies: vec!["general".into(), "general (ALT2)".into()],
        };
        assert!(state.begin_testing());
        state.set_live(Some(LiveRun {
            request: request.clone(),
            current: Some(1),
            restoring: false,
            results: Vec::new(),
        }));
        let going = read(&state);
        assert!(going.testing);
        assert_eq!(
            going.live,
            Some(::cli::Live {
                request,
                current: Some(1),
                restoring: false
            })
        );
        let failed = ::cli::Outcome::Failed {
            error: "curl.exe was not found".into(),
        };
        state.end_testing(failed.clone());
        assert_eq!(
            read(&state),
            ::cli::TestsStatus {
                testing: false,
                live: None,
                last: Some(failed)
            }
        );
    }
}
```

В `src-tauri/src/lib.rs` между `mod autostart;` и `mod commands;`:

```rust
mod cli;
```

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p zapret-hub --lib`
Expected: FAIL при компиляции: нет `tests_status` и `AppState` в `cli.rs`, поля `Settings::cli_enabled`, метода `last_outcome`, аргумента у `end_testing` (rustc может показать не все ошибки сразу).

- [ ] **Шаг 4: Настройка и итог прогона**

`src-tauri/src/settings.rs`, в `pub struct Settings` после `pub wizard_done: bool,`:

```rust
    /// The pipe of `zapret-hub-cli` runs commands (CLI spec 5).
    pub cli_enabled: bool,
```

в `impl Default for Settings` после `wizard_done: false,`:

```rust
            cli_enabled: false,
```

`src-tauri/src/state.rs`, в `pub struct AppState` после `notices: Mutex<Vec<Notice>>,`:

```rust
    /// How the newest run since the start ended: `tests_status` (CLI spec 7.1).
    last_outcome: Mutex<Option<cli::Outcome>>,
```

в `AppState::new` после `notices: Mutex::new(Vec::new()),`:

```rust
            last_outcome: Mutex::new(None),
```

и функцию `end_testing` заменить на:

```rust
    /// The outcome is in place before `testing` turns false: whoever polls `tests_status` never
    /// sees a finished run with the previous run's outcome (CLI spec 7.1).
    pub fn end_testing(&self, outcome: cli::Outcome) {
        *lock(&self.last_outcome) = Some(outcome);
        self.set_live(None);
        self.testing.store(false, Ordering::SeqCst);
    }

    pub fn last_outcome(&self) -> Option<cli::Outcome> {
        lock(&self.last_outcome).clone()
    }
```

`src-tauri/src/jobs.rs`, функцию `finish` заменить целиком:

```rust
fn finish(app: &AppHandle, outcome: anyhow::Result<TestRun>) {
    let state = state(app);
    let (run_id, last) = match outcome {
        Ok(run) => {
            if let Some(err) = &run.restore_error {
                actions::notice(
                    app,
                    NoticeLevel::Error,
                    texts::RESTORE_AFTER_TESTS,
                    err.clone(),
                );
            }
            let title = if run.canceled {
                texts::TESTS_CANCELED
            } else {
                texts::TESTS_DONE
            };
            notify::show(app, title, &texts::tests_done(&run));
            let id = run.id.clone();
            if run.suite == Suite::Standard && !run.canceled {
                state.set_ranking(Some(run));
            }
            (Some(id.clone()), cli::Outcome::Run { run_id: id })
        }
        Err(err) => {
            let text = format!("{err:#}");
            notify::show(app, texts::TESTS_FAILED, &text);
            actions::notice(app, NoticeLevel::Error, texts::TESTS_FAILED, text.clone());
            (None, cli::Outcome::Failed { error: text })
        }
    };
    state.end_testing(last);
    if let Err(err) = (TestsFinished { run_id }).emit(app) {
        log::warn!("cannot report the end of a test run: {err}");
    }
    actions::changed(app);
}
```

`set_live(None)` из `finish` ушёл в `end_testing`: итог ставится раньше, чем `testing` становится `false`, поэтому тот, кто опрашивает `tests_status`, не увидит конец прогона с итогом предыдущего.

`src-tauri/src/autostart.rs`, строку `pub const TASK: &str = "Zapret Hub";` заменить на:

```rust
/// `zapret-hub-cli start` runs it by this name.
pub const TASK: &str = cli::AUTOSTART_TASK;
```

`src-tauri/src/texts.rs`, перед `pub const TESTS_DONE`:

```rust
pub const CLI_PIPE_BUSY: &str = "Канал CLI занят другой программой";
pub const CLI_PIPE_FAILED: &str = "Канал CLI не открыт";

```

- [ ] **Шаг 5: Сервер в приложении**

`src-tauri/src/cli.rs`, между doc-комментарием и `#[cfg(test)]`:

```rust
use ::cli::pipe::{NAME, Server};
use ::cli::{Request, Response};
use serde_json::Value;
use tauri::async_runtime::block_on;
use tauri::{AppHandle, Manager as _};

use crate::actions;
use crate::commands;
use crate::dto::NoticeLevel;
use crate::state::AppState;
use crate::texts;

/// ERROR_ACCESS_DENIED from the first instance: another program holds the name.
const NAME_TAKEN: i32 = 5;

/// Called once from `setup`. The pipe is open even while the CLI is off: every request is checked
/// against the setting, so the switch works without a restart (spec 12, decision 3).
pub fn start(app: &AppHandle) {
    let server = match Server::bind(NAME) {
        Ok(server) => server,
        Err(err) => {
            log::error!("cli: {NAME} is not open: {err}");
            let title = if err.raw_os_error() == Some(NAME_TAKEN) {
                texts::CLI_PIPE_BUSY
            } else {
                texts::CLI_PIPE_FAILED
            };
            actions::notice(app, NoticeLevel::Error, title, format!("{NAME}: {err}"));
            return;
        }
    };
    log::info!("cli: listening on {NAME}");
    let app = app.clone();
    std::thread::spawn(move || {
        let err = server.serve(move |line| {
            serde_json::to_string(&handle(&app, line)).expect("an answer is plain data")
        });
        log::error!("cli: {NAME} stopped: {err}");
    });
}

/// One request line → one answer, and one journal line (spec 5).
fn handle(app: &AppHandle, line: &[u8]) -> Response {
    let enabled = app.state::<AppState>().settings().cli_enabled;
    let request = match ::cli::gate(line, enabled) {
        Ok(request) => request,
        Err(refusal) => {
            log::warn!(
                "cli: refused: {}",
                refusal.error.as_deref().unwrap_or_default()
            );
            return refusal;
        }
    };
    let cmd = request.name();
    match dispatch(app, request) {
        Ok(data) => {
            log::info!("cli: {cmd}: ok");
            Response::ok(data)
        }
        Err(err) => {
            log::warn!("cli: {cmd}: {err}");
            Response::error(err)
        }
    }
}

/// The window's command for each request: one arm each, so a new request does not compile until it
/// runs something.
fn dispatch(app: &AppHandle, request: Request) -> Result<Value, String> {
    let handle = app.clone();
    let state = app.state::<AppState>();
    match request {
        Request::AppVersion => json(commands::app_version()),
        Request::GetSettings => json(commands::get_settings(state)),
        Request::SaveSettings { settings } => {
            let settings = serde_json::from_value(settings)
                .map_err(|err| format!("the settings do not parse: {err}"))?;
            json(block_on(commands::save_settings(handle, settings))?)
        }
        Request::GetOverview => json(block_on(commands::get_overview(handle))?),
        Request::CheckProxy { addr } => json(block_on(commands::check_proxy(addr))?),
        Request::WizardCandidates => json(block_on(commands::wizard_candidates())?),
        Request::ListStrategies => json(block_on(commands::list_strategies(handle))?),
        Request::GetStrategy { name } => json(block_on(commands::get_strategy(handle, name))?),
        Request::ApplyStrategy { name } => json(block_on(commands::apply_strategy(handle, name))?),
        Request::ApplyBest => json(block_on(commands::apply_best(handle))?),
        Request::RestartZapret => json(block_on(commands::restart_zapret(handle))?),
        Request::CloneStrategy { source, name } => {
            json(block_on(commands::clone_strategy(handle, source, name))?)
        }
        Request::ValidateCustom { strategy } => {
            json(block_on(commands::validate_custom(handle, strategy))?)
        }
        Request::SaveCustom { strategy, create } => {
            json(block_on(commands::save_custom(handle, strategy, create))?)
        }
        Request::DeleteCustom { name } => json(block_on(commands::delete_custom(handle, name))?),
        Request::StartTests { request } => json(block_on(commands::start_tests(handle, request))?),
        Request::TestsStatus => Ok(tests_status(&state)),
        Request::CancelTests => {
            commands::cancel_tests(state);
            Ok(Value::Null)
        }
        Request::ListRuns => json(block_on(commands::list_runs(handle))?),
        Request::GetRun { id } => json(block_on(commands::get_run(handle, id))?),
        Request::RunReport { id } => json(block_on(commands::run_report(handle, id))?),
        Request::GetFlowseal => json(block_on(commands::get_flowseal(handle))?),
        Request::SetGameFilter { filter } => {
            json(block_on(commands::set_game_filter(handle, filter))?)
        }
        Request::SetIpsetMode { mode } => json(block_on(commands::set_ipset_mode(handle, mode))?),
        Request::SetCheckUpdates { enabled } => {
            json(block_on(commands::set_check_updates(handle, enabled))?)
        }
        Request::ReplaceFake { kind, source } => {
            json(block_on(commands::replace_fake(handle, kind, source))?)
        }
        Request::AddListEntry { list, entry } => {
            json(block_on(commands::add_list_entry(handle, list, entry))?)
        }
        Request::RemoveListEntry { list, entry } => {
            json(block_on(commands::remove_list_entry(handle, list, entry))?)
        }
        Request::UpdateIpset => json(block_on(commands::update_ipset(handle))?),
        Request::HostsPlan => json(block_on(commands::hosts_plan(handle))?),
        Request::ApplyHosts => json(block_on(commands::apply_hosts(handle))?),
        Request::CheckFlowsealUpdate => json(block_on(commands::check_flowseal_update(handle))?),
        Request::SystemChecks => json(block_on(commands::system_checks(handle))?),
        Request::FixCheck { action } => json(block_on(commands::fix_check(handle, action))?),
        Request::ClearDiscordCache => json(block_on(commands::clear_discord_cache())?),
        Request::RemoveService => json(block_on(commands::remove_service(handle))?),
        Request::QuickCheck => json(block_on(commands::quick_check(handle))?),
    }
}

fn json(value: impl serde::Serialize) -> Result<Value, String> {
    serde_json::to_value(value).map_err(|err| format!("the answer does not serialize: {err}"))
}

/// `tests_status` (spec 7.1): `test_live` and how the newest run ended.
fn tests_status(state: &AppState) -> Value {
    serde_json::json!({
        "testing": state.testing(),
        "live": state.live(),
        "last": state.last_outcome(),
    })
}
```

`src-tauri/src/lib.rs`, в `.setup(…)` после `jobs::start(app.handle());`:

```rust
            cli::start(app.handle());
```

Почему так:
- `block_on` вызывается в потоке клиента, вне рантайма Tauri, и так можно. Сами команды уходят в `blocking()` и очередь, как из окна.
- `dispatch` — исчерпывающий `match` без `_`: новый вариант `Request` не соберётся, пока не получит команду (решение 5).
- `cancel_tests` синхронная и ничего не возвращает: ответ — `null`.
- `handle` читает `cli_enabled` на каждый запрос (решение 3); канал открывается и при выключенном CLI.
- Имя занято → `Server::bind` возвращает os error 5 → уведомление `CLI_PIPE_BUSY`; приложение работает дальше без канала.
- Журнал: цель `zapret_hub_lib::cli` начинается с `zapret`, поэтому строки `cli: …` пишутся на уровне info.

- [ ] **Шаг 6: Тесты приложения проходят**

Run: `cargo test -p zapret-hub --lib`
Expected: PASS, 14 тестов.

- [ ] **Шаг 7: Привязки**

Run: `cargo run -p zapret-hub --bin export-bindings`
Expected: код 0.

Run: `git diff --stat frontend/src/lib/ipc/bindings.ts`
Expected: `1 file changed, 2 insertions(+)` — в `Settings` после `wizard_done?: boolean,`:

```ts
	/**  The pipe of `zapret-hub-cli` runs commands (CLI spec 5). */
	cli_enabled?: boolean,
```

- [ ] **Шаг 8: Падающий тест окна**

Окно читает настройки один раз при старте и после своего «Сохранить». Настройки, сохранённые агентом, оно бы не увидело, а следующее «Сохранить» в окне вернуло бы прежние. `StateChanged` приходит после любого изменения, в том числе после `save_settings` из CLI: по нему окно и будет перечитывать настройки.

`frontend/src/lib/app.test.ts` целиком (мок `stateChanged.listen` запоминает обработчик, `getSettings` отдаёт текущее `ipc.settings`, последний тест новый):

```ts
import { beforeEach, expect, test, vi } from "vitest";
import { notices } from "./notices.svelte";
import { S } from "./strings";
import { app, start } from "./app.svelte";

// `app.svelte` asks for `matchMedia` when it is imported and `applyTheme` writes to `document`: Node has neither.
const ipc = vi.hoisted(() => {
  Object.assign(globalThis, {
    matchMedia: () => ({ matches: false, addEventListener: () => {} }),
    document: { documentElement: { dataset: {} } },
  });
  return { log: [] as string[], failing: "", settings: {} as object, stateChanged: () => {} };
});

vi.mock("./ipc/bindings", () => {
  const listen = async () => {
    ipc.log.push("listen");
    return () => {};
  };
  const read = (name: string, value: () => unknown) => async () => {
    ipc.log.push(name);
    if (name === ipc.failing) throw new Error(`${name} is down`);
    return value();
  };
  return {
    events: {
      stateChanged: {
        listen: async (handler: () => void) => {
          ipc.stateChanged = handler;
          return listen();
        },
      },
      noticesChanged: { listen },
      testProgress: { listen },
      testsFinished: { listen },
    },
    commands: {
      getSettings: read("getSettings", () => ipc.settings),
      takeNotices: read("takeNotices", () => []),
      testLive: read("testLive", () => null),
      getOverview: read("getOverview", () => ({ status: "ok", data: {} })),
    },
  };
});

beforeEach(() => {
  ipc.log.length = 0;
  ipc.failing = "";
  ipc.settings = {};
  notices.items = [];
});

test("start listens before it reads and stays quiet when nothing fails", async () => {
  await start();
  expect(ipc.log.slice(0, 5)).toEqual(["listen", "listen", "listen", "listen", "getSettings"]);
  expect(notices.items).toEqual([]);
});

test.each(["getSettings", "takeNotices", "testLive"])("a rejected %s becomes an Error notice with the error as is", async (name) => {
  ipc.failing = name;
  await start();
  expect(notices.items).toEqual([{ level: "Error", title: S.failed.start, text: `Error: ${name} is down` }]);
});

test("a StateChanged reads the settings again: zapret-hub-cli saves them too", async () => {
  await start();
  ipc.settings = { theme: "dark", cli_enabled: false };
  ipc.log.length = 0;
  ipc.stateChanged();
  expect(ipc.log).toEqual(["getSettings", "getOverview"]);
  await vi.waitFor(() => expect(app.settings).toEqual({ theme: "dark", cli_enabled: false }));
});
```

- [ ] **Шаг 9: Убедиться, что тест падает**

Run: `npm test`
Expected: FAIL: `1 failed | 9 passed`; в журнале вызовов нового теста нет `"getSettings"`.

- [ ] **Шаг 10: Окно**

`frontend/src/lib/app.svelte.ts`, в `start()` обработчик `StateChanged` становится:

```ts
    await events.stateChanged.listen(() => {
      app.revision++;
      // zapret-hub-cli saves settings too: the window reads them again (CLI spec 8).
      void loadSettings();
      void refreshOverview();
    });
```

`frontend/src/lib/strings.ts`, в `settings` после строки `themes: { system: … } satisfies Record<Theme, string>,`:

```ts
    cli: "Командная строка",
    cliEnabled: "Управление из командной строки (zapret-hub-cli)",
    cliHint: "Любая программа, запущенная от вашего имени, сможет менять zapret через приложение.",
```

`frontend/src/tabs/Settings.svelte`: строку `<!-- One button for the three cards above: they are saved together. -->` (сразу после карточки «Запуск») заменить на карточку и исправленный комментарий:

```svelte
  <section class="card">
    <h2>{S.settings.cli}</h2>
    <label class="inline"><input type="checkbox" bind:checked={draft.cli_enabled} /> {S.settings.cliEnabled}</label>
    <p class="muted">{S.settings.cliHint}</p>
  </section>

  <!-- One button for the four cards above: they are saved together. -->
```

Флажок сохраняется общей кнопкой «Сохранить» вместе с остальными настройками; `.muted` уже есть в `app.css`.

- [ ] **Шаг 11: Проверки фронтенда**

Run: `npm test`
Expected: PASS, 10 тестов в 4 файлах.

Run: `npm run check`
Expected: `svelte-check found 0 errors and 0 warnings`.

- [ ] **Шаг 12: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 13: Коммит**

```bash
git add Cargo.lock src-tauri/Cargo.toml src-tauri/src/cli.rs src-tauri/src/lib.rs src-tauri/src/settings.rs src-tauri/src/state.rs src-tauri/src/jobs.rs src-tauri/src/autostart.rs src-tauri/src/texts.rs frontend/src/lib/ipc/bindings.ts frontend/src/lib/app.svelte.ts frontend/src/lib/app.test.ts frontend/src/lib/strings.ts frontend/src/tabs/Settings.svelte
git commit -m "feat(app): serve zapret-hub-cli over the pipe behind a setting"
```

---

### Задача 4: Бинарь `zapret-hub-cli`

**Files:**
- Modify: `Cargo.toml` (+ `clap`), `crates/cli/Cargo.toml` (+ `[[bin]]`, `exec`, `anyhow`, `clap`, `tempfile`)
- Create: `crates/cli/src/main.rs`
- Test: `crates/cli/src/main.rs` (модуль `tests`)

**Interfaces:**
- Consumes: `cli::{Request, Response, encode, PROTOCOL, TestsStatus, Live, Outcome, AUTOSTART_TASK}` (задача 1); `cli::pipe::{request, NAME}` (задача 2); ответ `tests_status` (задача 3); `exec::run(cmd: &mut Command, timeout: Duration, cancel: Option<&AtomicBool>) -> io::Result<exec::Output>` и `exec::Output::check(self) -> anyhow::Result<Self>`; `tester::catalog::StrategyRow { name, error, … }`, `tester::model::{TestRun, StrategyResult, Score, Suite}`.
- Produces: `zapret-hub-cli.exe` с командами таблицы спецификации 6: stdout — JSON, коды выхода 0–4. Задача 5 кладёт его в архив.

- [ ] **Шаг 1: Зависимости**

В корневом `Cargo.toml`, в `[workspace.dependencies]`, после `anyhow = "1.0"`:

```toml
clap = { version = "4.6", features = ["derive"] }
```

`crates/cli/Cargo.toml` целиком:

```toml
[package]
name = "cli"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "zapret-hub-cli and its protocol: agents drive the running Zapret Hub over a local pipe."

[[bin]]
name = "zapret-hub-cli"
path = "src/main.rs"

[dependencies]
exec.workspace = true
tester.workspace = true
zapret.workspace = true
anyhow.workspace = true
clap.workspace = true
serde.workspace = true
serde_json.workspace = true
windows = { workspace = true, features = ["Win32_Foundation", "Win32_Security", "Win32_Security_Authorization", "Win32_Storage_FileSystem", "Win32_System_IO", "Win32_System_Pipes", "Win32_System_Threading"] }

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Шаг 2: Падающие тесты**

`crates/cli/src/main.rs` — пока doc-комментарий и тесты:

```rust
//! `zapret-hub-cli`: one command to the running Zapret Hub, one JSON document on stdout, fixed
//! exit codes (CLI spec 6, 7).

#[cfg(test)]
mod tests {
    use clap::CommandFactory as _;
    use tester::model::{Score, StrategyResult};

    use super::*;

    /// Runs `args` against a fake app that gives `answers` in turn (then `null`); returns the
    /// outcome and what the app was asked.
    fn drive(
        args: &[&str],
        answers: Vec<Result<Value, Fail>>,
    ) -> (Result<Done, Fail>, Vec<Request>) {
        let args =
            Cli::try_parse_from(std::iter::once("zapret-hub-cli").chain(args.iter().copied()))
                .unwrap();
        let mut asked = Vec::new();
        let mut answers = answers.into_iter();
        let outcome = run(args, &mut |request| {
            asked.push(request);
            answers.next().unwrap_or(Ok(Value::Null))
        });
        (outcome, asked)
    }

    fn code(outcome: Result<Done, Fail>) -> u8 {
        outcome.map_or_else(|fail| fail.code, |_| 0)
    }

    #[test]
    fn the_arguments_are_well_formed() {
        Cli::command().debug_assert();
    }

    #[test]
    fn every_command_sends_its_window_command() {
        let general = || "general (ALT11)".to_owned();
        let table: Vec<(&[&str], Request)> = vec![
            (&["status"], Request::GetOverview),
            (&["settings", "get"], Request::GetSettings),
            (
                &["proxy", "check", "127.0.0.1:2080"],
                Request::CheckProxy {
                    addr: "127.0.0.1:2080".into(),
                },
            ),
            (&["candidates"], Request::WizardCandidates),
            (&["strategies", "list"], Request::ListStrategies),
            (
                &["strategies", "show", "general (ALT11)"],
                Request::GetStrategy { name: general() },
            ),
            (
                &["apply", "general (ALT11)"],
                Request::ApplyStrategy { name: general() },
            ),
            (&["apply-best"], Request::ApplyBest),
            (&["restart"], Request::RestartZapret),
            (&["quick-check"], Request::QuickCheck),
            (
                &["custom", "clone", "general (ALT11)", "mine"],
                Request::CloneStrategy {
                    source: general(),
                    name: "mine".into(),
                },
            ),
            (
                &["custom", "delete", "mine", "--yes"],
                Request::DeleteCustom {
                    name: "mine".into(),
                },
            ),
            (&["tests", "status"], Request::TestsStatus),
            (&["tests", "cancel"], Request::CancelTests),
            (&["tests", "list"], Request::ListRuns),
            (
                &["tests", "show", "2026-10-10_05-03-34", "--full"],
                Request::GetRun {
                    id: "2026-10-10_05-03-34".into(),
                },
            ),
            (
                &["tests", "report", "2026-10-10_05-03-34"],
                Request::RunReport {
                    id: "2026-10-10_05-03-34".into(),
                },
            ),
            (&["flowseal"], Request::GetFlowseal),
            (&["flowseal", "check-update"], Request::CheckFlowsealUpdate),
            (
                &[
                    "game-filter",
                    "tcp",
                    "--tcp",
                    "80,443",
                    "--udp",
                    "1024-65535",
                ],
                Request::SetGameFilter {
                    filter: GameFilter {
                        mode: GameFilterMode::Tcp,
                        tcp: "80,443".into(),
                        udp: "1024-65535".into(),
                    },
                },
            ),
            (
                &["ipset", "mode", "none"],
                Request::SetIpsetMode {
                    mode: IpsetMode::None,
                },
            ),
            (&["ipset", "update"], Request::UpdateIpset),
            (
                &["check-updates", "off"],
                Request::SetCheckUpdates { enabled: false },
            ),
            (
                &["fake", "game", "quic_initial_www_google_com"],
                Request::ReplaceFake {
                    kind: FakeKind::Game,
                    source: "quic_initial_www_google_com".into(),
                },
            ),
            (
                &["lists", "add", "ipset-exclude", "10.0.0.0/8"],
                Request::AddListEntry {
                    list: UserList::IpsetExclude,
                    entry: "10.0.0.0/8".into(),
                },
            ),
            (
                &["lists", "remove", "general", "example.com"],
                Request::RemoveListEntry {
                    list: UserList::General,
                    entry: "example.com".into(),
                },
            ),
            (&["hosts", "plan"], Request::HostsPlan),
            (&["hosts", "apply", "--yes"], Request::ApplyHosts),
            (&["checks"], Request::SystemChecks),
            (
                &["checks", "fix", "enable-tcp-timestamps"],
                Request::FixCheck {
                    action: CheckAction::EnableTcpTimestamps,
                },
            ),
            (
                &["--yes", "checks", "fix", "remove-windivert"],
                Request::FixCheck {
                    action: CheckAction::RemoveWinDivert,
                },
            ),
            (
                &["checks", "fix", "remove-conflicts", "--yes"],
                Request::FixCheck {
                    action: CheckAction::RemoveConflicts,
                },
            ),
            (
                &["discord", "clear-cache", "--yes"],
                Request::ClearDiscordCache,
            ),
            (&["service", "remove", "--yes"], Request::RemoveService),
        ];
        for (args, expected) in table {
            let (_, asked) = drive(args, Vec::new());
            assert_eq!(asked, [expected], "{args:?}");
        }
    }

    #[test]
    fn what_the_window_confirms_needs_yes() {
        for args in [
            &["custom", "delete", "mine"][..],
            &["hosts", "apply"],
            &["checks", "fix", "remove-windivert"],
            &["checks", "fix", "remove-conflicts"],
            &["discord", "clear-cache"],
            &["service", "remove"],
        ] {
            let (outcome, asked) = drive(args, Vec::new());
            assert_eq!(code(outcome), USAGE, "{args:?}");
            assert!(asked.is_empty(), "{args:?} reached the app");
        }
    }

    #[test]
    fn answers_become_exit_codes() {
        let line = |text: &str| Ok(text.as_bytes().to_vec());
        assert_eq!(
            answer(line(r#"{"ok":true,"data":{"app":"0.2.0"}}"#)),
            Ok(json!({ "app": "0.2.0" }))
        );
        assert_eq!(answer(line(r#"{"ok":true}"#)), Ok(Value::Null));
        assert_eq!(
            answer(line(r#"{"ok":false,"error":"no test results yet"}"#)),
            Err(Fail::new(FAILED, "no test results yet"))
        );
        assert_eq!(
            answer(line(
                r#"{"ok":false,"refused":true,"error":"the CLI is off"}"#
            )),
            Err(Fail::new(UNAVAILABLE, "the CLI is off"))
        );
        assert_eq!(code(answer(line("<html>")).map(Done::Json)), UNAVAILABLE);
        let absent = answer(Err(io::Error::from_raw_os_error(ERROR_FILE_NOT_FOUND))).unwrap_err();
        assert_eq!(absent.code, UNAVAILABLE);
        assert!(absent.error.contains("not running"), "{}", absent.error);
        let busy = answer(Err(io::Error::from_raw_os_error(ERROR_PIPE_BUSY))).unwrap_err();
        assert!(busy.error.contains("busy"), "{}", busy.error);
        let denied = answer(Err(io::Error::from_raw_os_error(ERROR_ACCESS_DENIED))).unwrap_err();
        assert_eq!(denied.code, UNAVAILABLE);
        assert!(denied.error.contains("Windows user"), "{}", denied.error);
    }

    #[test]
    fn bad_arguments_are_refused_by_clap() {
        for args in [
            &["settings", "set", "colour", "red"][..],
            &["lists", "add", "everything", "x"],
            &["tests", "run", "--timeout", "5"],
            &["frobnicate"],
        ] {
            let parsed =
                Cli::try_parse_from(std::iter::once("zapret-hub-cli").chain(args.iter().copied()));
            assert!(parsed.is_err(), "{args:?}");
        }
        let err = Cli::try_parse_from(["zapret-hub-cli", "settings", "set", "colour", "red"])
            .unwrap_err()
            .render()
            .to_string();
        assert!(
            err.contains("zapret_dir") && err.contains("cli_enabled") && !err.contains('\u{1b}'),
            "{err}"
        );
    }

    fn settings() -> Value {
        json!({
            "zapret_dir": "E:\\Zapret",
            "socks5": "127.0.0.1:2080",
            "autostart": false,
            "start_minimized": true,
            "theme": "system",
            "wizard_done": true,
            "cli_enabled": true,
        })
    }

    #[test]
    fn settings_set_changes_one_key_and_sends_all_of_them() {
        for (key, value, expected) in [
            ("socks5", "", Value::Null),
            ("zapret_dir", r"C:\Запрет", json!(r"C:\Запрет")),
            ("theme", "dark", json!("dark")),
            ("cli_enabled", "false", json!(false)),
            ("start_minimized", "false", json!(false)),
        ] {
            let (outcome, asked) = drive(&["settings", "set", key, value], vec![Ok(settings())]);
            assert_eq!(outcome, Ok(Done::Json(Value::Null)), "{key}");
            let mut changed = settings();
            changed[key] = expected;
            assert_eq!(
                asked,
                [
                    Request::GetSettings,
                    Request::SaveSettings { settings: changed }
                ]
            );
        }
    }

    #[test]
    fn settings_set_checks_the_value_before_asking() {
        for (key, value) in [
            ("theme", "blue"),
            ("autostart", "yes"),
            ("cli_enabled", "1"),
        ] {
            let (outcome, asked) = drive(&["settings", "set", key, value], vec![Ok(settings())]);
            assert_eq!(code(outcome), USAGE, "{key} {value}");
            assert!(asked.is_empty());
        }
    }

    #[test]
    fn proxy_check_without_an_address_takes_the_setting() {
        let (_, asked) = drive(&["proxy", "check"], vec![Ok(settings())]);
        assert_eq!(
            asked,
            [
                Request::GetSettings,
                Request::CheckProxy {
                    addr: "127.0.0.1:2080".into()
                }
            ]
        );
        let (outcome, asked) = drive(&["proxy", "check"], vec![Ok(json!({ "socks5": null }))]);
        assert_eq!(code(outcome), USAGE);
        assert_eq!(asked, [Request::GetSettings]);
    }

    #[test]
    fn game_filter_keeps_the_ports_not_given() {
        let flowseal = json!({
            "game_filter": { "mode": "all", "tcp": "1024-65535", "udp": "50000-50100" },
            "ipset": "loaded",
        });
        let (_, asked) = drive(&["game-filter", "tcp", "--tcp", "80"], vec![Ok(flowseal)]);
        assert_eq!(
            asked,
            [
                Request::GetFlowseal,
                Request::SetGameFilter {
                    filter: GameFilter {
                        mode: GameFilterMode::Tcp,
                        tcp: "80".into(),
                        udp: "50000-50100".into(),
                    }
                }
            ]
        );
    }

    #[test]
    fn custom_save_reads_the_strategy_from_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mine.json");
        let strategy = CustomStrategy {
            name: "mine".into(),
            cloned_from: "general (ALT11)".into(),
            wf: "--wf-tcp=80,443".into(),
            blocks: vec!["--filter-tcp=443 --dpi-desync=fake".into()],
        };
        let json = serde_json::to_string(&strategy).unwrap();
        std::fs::write(&file, format!("\u{feff}{json}")).unwrap();
        let path = file.to_str().unwrap();
        let (_, asked) = drive(&["custom", "save", path, "--create"], Vec::new());
        assert_eq!(
            asked,
            [Request::SaveCustom {
                strategy,
                create: true
            }]
        );
        std::fs::write(&file, "{\"name\": 1}").unwrap();
        let (outcome, asked) = drive(&["custom", "validate", path], Vec::new());
        assert_eq!(code(outcome), USAGE);
        assert!(asked.is_empty());
    }

    fn rows() -> Value {
        json!([
            { "name": "general", "custom": false, "active": true, "error": null, "last": null },
            { "name": "general (broken)", "custom": false, "active": false, "error": "no winws line", "last": null },
            { "name": "general (ALT11)", "custom": false, "active": false, "error": null, "last": null },
        ])
    }

    #[test]
    fn tests_run_without_names_takes_every_strategy_that_parses() {
        let (outcome, asked) = drive(&["tests", "run"], vec![Ok(rows())]);
        assert_eq!(
            outcome,
            Ok(Done::Json(
                json!({ "started": true, "suite": "standard", "strategies": 2 })
            ))
        );
        assert_eq!(
            asked,
            [
                Request::ListStrategies,
                Request::StartTests {
                    request: TestRequest {
                        suite: Suite::Standard,
                        strategies: vec!["general".into(), "general (ALT11)".into()],
                    }
                }
            ]
        );
    }

    fn result(name: &str, launch_error: Option<&str>) -> StrategyResult {
        StrategyResult {
            name: name.into(),
            args_sha256: String::new(),
            launch_error: launch_error.map(str::to_owned),
            targets: Vec::new(),
            dpi: Vec::new(),
            score: Score {
                ok: 35,
                total: 36,
                ..Score::default()
            },
        }
    }

    fn test_run() -> TestRun {
        TestRun {
            id: "2026-10-11_10-00-00".into(),
            suite: Suite::Dpi,
            flowseal_version: Some("1.10.3".into()),
            canceled: false,
            restore_error: None,
            strategies: vec![
                result("general (ALT11)", None),
                result("general", Some("exit 1")),
            ],
        }
    }

    #[test]
    fn tests_run_wait_ends_with_the_ranking() {
        let finished =
            json!({ "testing": false, "live": null, "last": { "run_id": "2026-10-11_10-00-00" } });
        let (outcome, asked) = drive(
            &[
                "tests",
                "run",
                "--dpi",
                "--wait",
                "general (ALT11)",
                "general",
            ],
            vec![
                Ok(Value::Null),
                Ok(finished),
                Ok(serde_json::to_value(test_run()).unwrap()),
            ],
        );
        assert_eq!(
            asked,
            [
                Request::StartTests {
                    request: TestRequest {
                        suite: Suite::Dpi,
                        strategies: vec!["general (ALT11)".into(), "general".into()],
                    }
                },
                Request::TestsStatus,
                Request::GetRun {
                    id: "2026-10-11_10-00-00".into()
                },
            ]
        );
        assert_eq!(outcome, Ok(Done::Json(compact(&test_run()))));
    }

    #[test]
    fn the_compact_ranking_keeps_places_scores_and_launch_errors() {
        let compact = compact(&test_run());
        assert_eq!(compact["suite"], "dpi");
        assert_eq!(compact["ranking"][0]["place"], 1);
        assert_eq!(compact["ranking"][0]["name"], "general (ALT11)");
        assert_eq!(compact["ranking"][0]["score"]["ok"], 35);
        assert_eq!(compact["ranking"][1]["launch_error"], "exit 1");
        assert!(compact["ranking"][0].get("targets").is_none());
    }

    #[test]
    fn tests_wait_reports_how_the_run_ended() {
        let idle = |last: Value| json!({ "testing": false, "live": null, "last": last });
        let (outcome, _) = drive(
            &["tests", "wait"],
            vec![Ok(idle(json!({ "error": "curl.exe was not found" })))],
        );
        assert_eq!(outcome, Err(Fail::new(FAILED, "curl.exe was not found")));
        let (outcome, _) = drive(&["tests", "wait"], vec![Ok(idle(Value::Null))]);
        assert_eq!(outcome, Ok(Done::Json(json!({ "last": null }))));
        let going = json!({ "testing": true, "live": null, "last": null });
        let (outcome, asked) = drive(&["tests", "wait", "--timeout", "0"], vec![Ok(going)]);
        assert_eq!(code(outcome), TIMED_OUT);
        assert_eq!(asked, [Request::TestsStatus]);
    }

    #[test]
    fn progress_names_each_strategy_once() {
        let live = Live {
            request: TestRequest {
                suite: Suite::Standard,
                strategies: vec!["general".into(), "general (ALT2)".into()],
            },
            current: Some(1),
            restoring: false,
        };
        let mut shown = None;
        assert_eq!(
            progress(&live, &mut shown).as_deref(),
            Some("2/2 general (ALT2)")
        );
        assert_eq!(progress(&live, &mut shown), None);
    }

    #[test]
    fn version_answers_without_the_app() {
        let (outcome, _) = drive(
            &["version"],
            vec![Err(Fail::new(UNAVAILABLE, "Zapret Hub is not running"))],
        );
        assert_eq!(
            outcome,
            Ok(Done::Json(json!({
                "cli": env!("CARGO_PKG_VERSION"),
                "protocol": 1,
                "app": null,
            })))
        );
    }
}
```

Что проверяют тесты (`run` получает фальшивое приложение: замыкание, которое запоминает запросы и отдаёт заготовленные ответы):
- `the_arguments_are_well_formed` — `debug_assert` clap;
- `every_command_sends_its_window_command` — таблица спецификации 6: аргументы → ровно один запрос;
- `what_the_window_confirms_needs_yes` — без `--yes` код 2 и ни одного запроса;
- `answers_become_exit_codes` — `ok`, `ok:false` → 1, `refused` → 3, чужой ответ → 3, нет канала, занят, отказ доступа → 3 с объяснением;
- `bad_arguments_are_refused_by_clap` — неизвестный ключ со списком известных, неизвестный список, `--timeout` без `--wait`, неизвестная команда; без ANSI;
- `settings_set_changes_one_key_and_sends_all_of_them`, `settings_set_checks_the_value_before_asking` — слияние настроек (спецификация 7.2);
- `proxy_check_without_an_address_takes_the_setting`, `game_filter_keeps_the_ports_not_given` — уточнения спецификации 6;
- `custom_save_reads_the_strategy_from_a_file` — файл с BOM, неверная форма → код 2;
- `tests_run_without_names_takes_every_strategy_that_parses`, `tests_run_wait_ends_with_the_ranking`, `the_compact_ranking_keeps_places_scores_and_launch_errors`, `tests_wait_reports_how_the_run_ended`, `progress_names_each_strategy_once` — спецификация 7.1;
- `version_answers_without_the_app`.

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p cli --bin zapret-hub-cli`
Expected: FAIL при компиляции: нет `Cli`, `run`, `answer`, `main` и остального.

- [ ] **Шаг 4: Бинарь**

`crates/cli/src/main.rs`, между doc-комментарием и `#[cfg(test)]`:

```rust
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
use std::process::{Command as Process, ExitCode};
use std::time::{Duration, Instant};

use clap::{Parser, Subcommand, ValueEnum};
use cli::{Live, Outcome, PROTOCOL, Request, Response, TestsStatus};
use serde::de::DeserializeOwned;
use serde_json::{Value, json};
use tester::catalog::StrategyRow;
use tester::model::{Suite, TestRun};
use tester::run::Request as TestRequest;
use zapret::checks::CheckAction;
use zapret::custom::CustomStrategy;
use zapret::fakes::FakeKind;
use zapret::game_filter::{GameFilter, GameFilterMode};
use zapret::ipset::IpsetMode;
use zapret::user_lists::UserList;

/// The app ran the command and it failed, or the test run failed.
const FAILED: u8 = 1;
/// Bad arguments, or `--yes` missing.
const USAGE: u8 = 2;
/// The app is not running, refused (the CLI is off, another protocol) or cannot be read: fix the
/// setup, not the command.
const UNAVAILABLE: u8 = 3;
/// `tests wait` or `tests run --wait` reached `--timeout`.
const TIMED_OUT: u8 = 4;

const ERROR_FILE_NOT_FOUND: i32 = 2;
const ERROR_ACCESS_DENIED: i32 = 5;
const ERROR_PIPE_BUSY: i32 = 231;

/// How long a busy pipe is tried again (spec 7).
const BUSY: Duration = Duration::from_secs(10);
/// How long `start` waits for the pipe after the task ran (spec 4).
const START_WAIT: Duration = Duration::from_secs(30);
/// How often `tests wait` asks (spec 7.1).
const POLL: Duration = Duration::from_secs(2);

#[derive(Debug, Parser)]
#[command(
    name = "zapret-hub-cli",
    about = "Controls the running Zapret Hub. Prints one JSON document.",
    after_help = "Exit codes: 0 done; 1 the command failed; 2 bad arguments or no --yes; \
                  3 Zapret Hub not running, the CLI off in its Settings, or another protocol; \
                  4 --timeout reached."
)]
struct Cli {
    /// Confirm what the window asks to confirm: custom delete, hosts apply, checks fix
    /// remove-windivert|remove-conflicts, discord clear-cache, service remove.
    #[arg(long, global = true)]
    yes: bool,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Service, strategy, last quick check and test run.
    Status,
    /// Versions of this CLI, of its protocol and of the running app.
    Version,
    /// Start Zapret Hub through its autostart task and wait up to 30 s for it.
    Start,
    /// zapret-hub.toml.
    #[command(subcommand)]
    Settings(SettingsCommand),
    /// SOCKS5 proxy.
    #[command(subcommand)]
    Proxy(ProxyCommand),
    /// Zapret folders and SOCKS5 proxies found on this computer.
    Candidates,
    /// The strategies of the zapret folder.
    #[command(subcommand)]
    Strategies(StrategiesCommand),
    /// Install a strategy as the zapret service, then run a quick check.
    Apply { name: String },
    /// Apply the best strategy of the newest standard test run.
    ApplyBest,
    /// Restart the zapret service, then run a quick check.
    Restart,
    /// Check that zapret works now.
    QuickCheck,
    /// Custom strategies.
    #[command(subcommand)]
    Custom(CustomCommand),
    /// Strategy test runs.
    #[command(subcommand)]
    Tests(TestsCommand),
    /// Flowseal's settings, lists and fakes.
    Flowseal {
        #[command(subcommand)]
        command: Option<FlowsealCommand>,
    },
    /// Set the game filter; ports not given stay as they are.
    GameFilter {
        mode: FilterMode,
        /// TCP ports, e.g. 1024-65535.
        #[arg(long)]
        tcp: Option<String>,
        /// UDP ports, e.g. 1024-65535.
        #[arg(long)]
        udp: Option<String>,
    },
    /// The ipset-all list.
    #[command(subcommand)]
    Ipset(IpsetCommand),
    /// Flowseal's own update check.
    CheckUpdates { state: Switch },
    /// Make a file of bin\ (its name without .bin) the active fake.
    Fake { kind: Fake, name: String },
    /// The user lists.
    #[command(subcommand)]
    Lists(ListsCommand),
    /// The zapret-hub block in hosts.
    #[command(subcommand)]
    Hosts(HostsCommand),
    /// service.bat's diagnostics.
    Checks {
        #[command(subcommand)]
        command: Option<ChecksCommand>,
    },
    /// Discord.
    #[command(subcommand)]
    Discord(DiscordCommand),
    /// The zapret service.
    #[command(subcommand)]
    Service(ServiceCommand),
}

#[derive(Debug, Subcommand)]
enum SettingsCommand {
    /// All settings.
    Get,
    /// Change one setting; an empty value clears zapret_dir and socks5.
    Set { key: Key, value: String },
}

#[derive(Debug, Subcommand)]
enum ProxyCommand {
    /// Check an address, or the socks5 setting when none is given.
    Check { addr: Option<String> },
}

#[derive(Debug, Subcommand)]
enum StrategiesCommand {
    /// Every strategy in service.bat's order with its last test result.
    List,
    /// Arguments, last result and, for a custom strategy, its record.
    Show { name: String },
}

#[derive(Debug, Subcommand)]
enum CustomCommand {
    /// Copy a strategy as "general (HUB <name>)" and print its record.
    Clone { source: String, name: String },
    /// Check a CustomStrategy JSON (a file, or - for stdin) without saving it.
    Validate { file: PathBuf },
    /// Save a CustomStrategy JSON (a file, or - for stdin).
    Save {
        file: PathBuf,
        /// A new strategy rather than a change to one.
        #[arg(long)]
        create: bool,
    },
    /// Delete a custom strategy by its own name: mine for "general (HUB mine)" (needs --yes).
    Delete { name: String },
}

#[derive(Debug, Subcommand)]
enum TestsCommand {
    /// Start a run of the named strategies, or of every one that parses.
    Run {
        /// The DPI suite instead of the standard one.
        #[arg(long)]
        dpi: bool,
        /// Wait for the end and print the ranking.
        #[arg(long)]
        wait: bool,
        /// Stop waiting after this many seconds (exit code 4); the run goes on.
        #[arg(long, requires = "wait")]
        timeout: Option<u64>,
        /// Strategy names in the order to run them; none: every strategy that parses.
        names: Vec<String>,
    },
    /// Whether a run is going, its progress and how the last one ended.
    Status,
    /// Wait for the run to end and print its ranking.
    Wait {
        /// Stop waiting after this many seconds (exit code 4); the run goes on.
        #[arg(long)]
        timeout: Option<u64>,
    },
    /// Cancel the run.
    Cancel,
    /// The saved runs.
    List,
    /// A run's ranking; --full prints every check.
    Show {
        id: String,
        #[arg(long)]
        full: bool,
    },
    /// A run as test zapret.ps1 writes it (text).
    Report { id: String },
}

#[derive(Debug, Subcommand)]
enum FlowsealCommand {
    /// Look for a newer Flowseal version.
    CheckUpdate,
}

#[derive(Debug, Subcommand)]
enum IpsetCommand {
    /// Switch the mode as service.bat does.
    Mode { mode: Ipset },
    /// Download ipset-all.txt again.
    Update,
}

#[derive(Debug, Subcommand)]
enum ListsCommand {
    Add { list: List, entry: String },
    Remove { list: List, entry: String },
}

#[derive(Debug, Subcommand)]
enum HostsCommand {
    /// What applying would change; nothing is written.
    Plan,
    /// Write the zapret-hub block into hosts (needs --yes).
    Apply,
}

#[derive(Debug, Subcommand)]
enum ChecksCommand {
    /// Apply a fix; remove-windivert and remove-conflicts need --yes.
    Fix { action: Fix },
}

#[derive(Debug, Subcommand)]
enum DiscordCommand {
    /// Close Discord and clear its caches (needs --yes).
    ClearCache,
}

#[derive(Debug, Subcommand)]
enum ServiceCommand {
    /// Remove the zapret service and WinDivert (needs --yes).
    Remove,
}

/// The keys of zapret-hub.toml the CLI may change (spec 7.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
#[value(rename_all = "snake_case")]
enum Key {
    ZapretDir,
    Socks5,
    Autostart,
    StartMinimized,
    Theme,
    CliEnabled,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum FilterMode {
    Disabled,
    All,
    Tcp,
    Udp,
}

impl From<FilterMode> for GameFilterMode {
    fn from(mode: FilterMode) -> Self {
        match mode {
            FilterMode::Disabled => Self::Disabled,
            FilterMode::All => Self::All,
            FilterMode::Tcp => Self::Tcp,
            FilterMode::Udp => Self::Udp,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Ipset {
    Loaded,
    None,
    Any,
}

impl From<Ipset> for IpsetMode {
    fn from(mode: Ipset) -> Self {
        match mode {
            Ipset::Loaded => Self::Loaded,
            Ipset::None => Self::None,
            Ipset::Any => Self::Any,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Switch {
    On,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Fake {
    Discord,
    Game,
}

impl From<Fake> for FakeKind {
    fn from(kind: Fake) -> Self {
        match kind {
            Fake::Discord => Self::Discord,
            Fake::Game => Self::Game,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum List {
    General,
    Exclude,
    IpsetExclude,
}

impl From<List> for UserList {
    fn from(list: List) -> Self {
        match list {
            List::General => Self::General,
            List::Exclude => Self::Exclude,
            List::IpsetExclude => Self::IpsetExclude,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Fix {
    EnableTcpTimestamps,
    RemoveWindivert,
    RemoveConflicts,
}

impl From<Fix> for CheckAction {
    fn from(fix: Fix) -> Self {
        match fix {
            Fix::EnableTcpTimestamps => Self::EnableTcpTimestamps,
            Fix::RemoveWindivert => Self::RemoveWinDivert,
            Fix::RemoveConflicts => Self::RemoveConflicts,
        }
    }
}

/// What goes to stdout on success.
#[derive(Debug, PartialEq)]
enum Done {
    Json(Value),
    /// `tests report`: the text as the app sent it.
    Text(String),
}

/// `{"error": …}` on stdout, and the exit code.
#[derive(Debug, PartialEq)]
struct Fail {
    code: u8,
    error: String,
}

impl Fail {
    fn new(code: u8, error: impl Into<String>) -> Self {
        Self {
            code,
            error: error.into(),
        }
    }
}

/// One request to the app and its answer's `data`.
type Call<'a> = &'a mut dyn FnMut(Request) -> Result<Value, Fail>;

fn main() -> ExitCode {
    let args = match Cli::try_parse() {
        Ok(args) => args,
        // --help, as clap prints it.
        Err(err) if !err.use_stderr() => err.exit(),
        Err(err) => return finish(Err(Fail::new(USAGE, err.render().to_string()))),
    };
    finish(run(args, &mut |request| answer(send(&request))))
}

fn send(request: &Request) -> io::Result<Vec<u8>> {
    cli::pipe::request(cli::pipe::NAME, cli::encode(request).as_bytes(), BUSY)
}

/// Prints the outcome and turns it into the exit code (spec 7).
fn finish(outcome: Result<Done, Fail>) -> ExitCode {
    let (text, code) = match outcome {
        Ok(Done::Json(value)) => (pretty(&value), 0),
        Ok(Done::Text(text)) => (text, 0),
        Err(fail) => (pretty(&json!({ "error": fail.error })), fail.code),
    };
    // A closed stdout is the caller's business, not a panic.
    let _ = io::stdout().lock().write_all(text.as_bytes());
    ExitCode::from(code)
}

fn pretty(value: &Value) -> String {
    serde_json::to_string_pretty(value).expect("a JSON value prints") + "\n"
}

/// The app's answer line → its `data`, or the exit code it means (spec 6, 7).
fn answer(line: io::Result<Vec<u8>>) -> Result<Value, Fail> {
    let line = line.map_err(|err| Fail::new(UNAVAILABLE, unavailable(&err)))?;
    let response: Response = serde_json::from_slice(&line).map_err(foreign)?;
    match response {
        Response { ok: true, data, .. } => Ok(data),
        Response { refused, error, .. } => Err(Fail::new(
            if refused { UNAVAILABLE } else { FAILED },
            error.unwrap_or_default(),
        )),
    }
}

fn unavailable(err: &io::Error) -> String {
    match err.raw_os_error() {
        Some(ERROR_FILE_NOT_FOUND) => {
            "Zapret Hub is not running: start it with `zapret-hub-cli start` or run zapret-hub.exe"
                .to_owned()
        }
        Some(ERROR_ACCESS_DENIED) => "Zapret Hub's pipe did not let this program in: it answers \
                                       only the Windows user who started Zapret Hub, outside \
                                       sandboxes"
            .to_owned(),
        Some(ERROR_PIPE_BUSY) => "Zapret Hub's pipe stayed busy for 10 s: try again".to_owned(),
        _ => format!("cannot reach Zapret Hub: {err}"),
    }
}

/// An answer of another shape: the CLI and the app do not match.
fn foreign(err: serde_json::Error) -> Fail {
    Fail::new(
        UNAVAILABLE,
        format!("Zapret Hub sent an answer this CLI does not read: {err}"),
    )
}

fn read<T: DeserializeOwned>(value: Value) -> Result<T, Fail> {
    serde_json::from_value(value).map_err(foreign)
}

/// What the window confirms before it runs it (spec 6).
fn needs_yes(command: &Command) -> bool {
    matches!(
        command,
        Command::Custom(CustomCommand::Delete { .. })
            | Command::Hosts(HostsCommand::Apply)
            | Command::Checks {
                command: Some(ChecksCommand::Fix {
                    action: Fix::RemoveWindivert | Fix::RemoveConflicts
                })
            }
            | Command::Discord(DiscordCommand::ClearCache)
            | Command::Service(ServiceCommand::Remove)
    )
}

fn run(args: Cli, call: Call) -> Result<Done, Fail> {
    if needs_yes(&args.command) && !args.yes {
        return Err(Fail::new(
            USAGE,
            "the window asks to confirm this: add --yes",
        ));
    }
    let request = match args.command {
        Command::Version => {
            return Ok(Done::Json(json!({
                "cli": env!("CARGO_PKG_VERSION"),
                "protocol": PROTOCOL,
                "app": call(Request::AppVersion).ok(),
            })));
        }
        Command::Start => return start(),
        Command::Settings(SettingsCommand::Set { key, value }) => {
            let value = setting(key, &value)?;
            let mut settings = call(Request::GetSettings)?;
            settings
                .as_object_mut()
                .ok_or_else(|| {
                    Fail::new(
                        UNAVAILABLE,
                        "Zapret Hub sent settings that are not an object",
                    )
                })?
                .insert(key_name(key), value);
            Request::SaveSettings { settings }
        }
        Command::Proxy(ProxyCommand::Check { addr: None }) => {
            let addr = call(Request::GetSettings)?["socks5"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| {
                    Fail::new(USAGE, "no address given and no socks5 in the settings")
                })?;
            Request::CheckProxy { addr }
        }
        Command::GameFilter { mode, tcp, udp } => {
            let (tcp, udp) = match (tcp, udp) {
                (Some(tcp), Some(udp)) => (tcp, udp),
                (tcp, udp) => {
                    let now: GameFilter = read(call(Request::GetFlowseal)?["game_filter"].clone())?;
                    (tcp.unwrap_or(now.tcp), udp.unwrap_or(now.udp))
                }
            };
            Request::SetGameFilter {
                filter: GameFilter {
                    mode: mode.into(),
                    tcp,
                    udp,
                },
            }
        }
        Command::Custom(CustomCommand::Validate { file }) => Request::ValidateCustom {
            strategy: read_strategy(&file)?,
        },
        Command::Custom(CustomCommand::Save { file, create }) => Request::SaveCustom {
            strategy: read_strategy(&file)?,
            create,
        },
        Command::Tests(TestsCommand::Run {
            dpi,
            wait,
            timeout,
            names,
        }) => {
            let strategies = if names.is_empty() {
                runnable(call(Request::ListStrategies)?)?
            } else {
                names
            };
            let suite = if dpi { Suite::Dpi } else { Suite::Standard };
            let count = strategies.len();
            call(Request::StartTests {
                request: TestRequest { suite, strategies },
            })?;
            if wait {
                return wait_for_run(call, timeout);
            }
            return Ok(Done::Json(json!({
                "started": true,
                "suite": suite_name(suite),
                "strategies": count,
            })));
        }
        Command::Tests(TestsCommand::Wait { timeout }) => return wait_for_run(call, timeout),
        Command::Tests(TestsCommand::Show { id, full }) => return show(call, id, full),
        Command::Tests(TestsCommand::Report { id }) => {
            return Ok(Done::Text(read(call(Request::RunReport { id })?)?));
        }
        // One command, one request: the table of spec 6.
        Command::Status => Request::GetOverview,
        Command::Settings(SettingsCommand::Get) => Request::GetSettings,
        Command::Proxy(ProxyCommand::Check { addr: Some(addr) }) => Request::CheckProxy { addr },
        Command::Candidates => Request::WizardCandidates,
        Command::Strategies(StrategiesCommand::List) => Request::ListStrategies,
        Command::Strategies(StrategiesCommand::Show { name }) => Request::GetStrategy { name },
        Command::Apply { name } => Request::ApplyStrategy { name },
        Command::ApplyBest => Request::ApplyBest,
        Command::Restart => Request::RestartZapret,
        Command::QuickCheck => Request::QuickCheck,
        Command::Custom(CustomCommand::Clone { source, name }) => {
            Request::CloneStrategy { source, name }
        }
        Command::Custom(CustomCommand::Delete { name }) => Request::DeleteCustom { name },
        Command::Tests(TestsCommand::Status) => Request::TestsStatus,
        Command::Tests(TestsCommand::Cancel) => Request::CancelTests,
        Command::Tests(TestsCommand::List) => Request::ListRuns,
        Command::Flowseal { command: None } => Request::GetFlowseal,
        Command::Flowseal {
            command: Some(FlowsealCommand::CheckUpdate),
        } => Request::CheckFlowsealUpdate,
        Command::Ipset(IpsetCommand::Mode { mode }) => Request::SetIpsetMode { mode: mode.into() },
        Command::Ipset(IpsetCommand::Update) => Request::UpdateIpset,
        Command::CheckUpdates { state } => Request::SetCheckUpdates {
            enabled: state == Switch::On,
        },
        Command::Fake { kind, name } => Request::ReplaceFake {
            kind: kind.into(),
            source: name,
        },
        Command::Lists(ListsCommand::Add { list, entry }) => Request::AddListEntry {
            list: list.into(),
            entry,
        },
        Command::Lists(ListsCommand::Remove { list, entry }) => Request::RemoveListEntry {
            list: list.into(),
            entry,
        },
        Command::Hosts(HostsCommand::Plan) => Request::HostsPlan,
        Command::Hosts(HostsCommand::Apply) => Request::ApplyHosts,
        Command::Checks { command: None } => Request::SystemChecks,
        Command::Checks {
            command: Some(ChecksCommand::Fix { action }),
        } => Request::FixCheck {
            action: action.into(),
        },
        Command::Discord(DiscordCommand::ClearCache) => Request::ClearDiscordCache,
        Command::Service(ServiceCommand::Remove) => Request::RemoveService,
    };
    call(request).map(Done::Json)
}

fn key_name(key: Key) -> String {
    key.to_possible_value()
        .expect("no key is skipped")
        .get_name()
        .to_owned()
}

/// A value for `settings set`, checked before anything is sent (spec 7.2).
fn setting(key: Key, value: &str) -> Result<Value, Fail> {
    let bad = |what: &str| {
        Fail::new(
            USAGE,
            format!("{} takes {what}, not {value:?}", key_name(key)),
        )
    };
    match key {
        Key::ZapretDir | Key::Socks5 if value.trim().is_empty() => Ok(Value::Null),
        Key::ZapretDir | Key::Socks5 => Ok(value.into()),
        Key::Autostart | Key::StartMinimized | Key::CliEnabled => value
            .parse::<bool>()
            .map(Value::Bool)
            .map_err(|_| bad("true or false")),
        Key::Theme if ["system", "light", "dark"].contains(&value) => Ok(value.into()),
        Key::Theme => Err(bad("system, light or dark")),
    }
}

/// A `CustomStrategy` JSON from a file, or from stdin for `-` (spec 7.3).
fn read_strategy(file: &Path) -> Result<CustomStrategy, Fail> {
    let text = if file == Path::new("-") {
        io::read_to_string(io::stdin())
    } else {
        std::fs::read_to_string(file)
    }
    .map_err(|err| Fail::new(USAGE, format!("cannot read {}: {err}", file.display())))?;
    // PowerShell 5.1 (`Set-Content -Encoding UTF8`) and Notepad start the file with a BOM.
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    serde_json::from_str(text).map_err(|err| {
        Fail::new(
            USAGE,
            format!(
                "{} is not a CustomStrategy (name, cloned_from, wf, blocks): {err}",
                file.display()
            ),
        )
    })
}

/// `tests run` without names: every strategy that parses, in service.bat's order (spec 7.1).
fn runnable(rows: Value) -> Result<Vec<String>, Fail> {
    let rows: Vec<StrategyRow> = read(rows)?;
    Ok(rows
        .into_iter()
        .filter(|row| row.error.is_none())
        .map(|row| row.name)
        .collect())
}

fn suite_name(suite: Suite) -> &'static str {
    match suite {
        Suite::Standard => "standard",
        Suite::Dpi => "dpi",
    }
}

/// Asks `tests_status` every 2 s until the run is over; the progress goes to stderr (spec 7.1).
fn wait_for_run(call: Call, timeout: Option<u64>) -> Result<Done, Fail> {
    let deadline = timeout.map(|secs| Instant::now() + Duration::from_secs(secs));
    let mut shown = None;
    loop {
        let status: TestsStatus = read(call(Request::TestsStatus)?)?;
        if !status.testing {
            return match status.last {
                None => Ok(Done::Json(json!({ "last": null }))),
                Some(Outcome::Failed { error }) => Err(Fail::new(FAILED, error)),
                Some(Outcome::Run { run_id }) => show(call, run_id, false),
            };
        }
        if let Some(line) = status
            .live
            .as_ref()
            .and_then(|live| progress(live, &mut shown))
        {
            let _ = writeln!(io::stderr(), "{line}");
        }
        let left = deadline.map_or(POLL, |deadline| {
            deadline.saturating_duration_since(Instant::now())
        });
        if left.is_zero() {
            return Err(Fail::new(
                TIMED_OUT,
                format!(
                    "the test run is still going after {} s: wait again with `zapret-hub-cli tests wait`",
                    timeout.unwrap_or_default()
                ),
            ));
        }
        std::thread::sleep(left.min(POLL));
    }
}

/// `3/33 general (ALT2)`, once per strategy.
fn progress(live: &Live, shown: &mut Option<u32>) -> Option<String> {
    let index = live.current.filter(|index| *shown != Some(*index))?;
    *shown = Some(index);
    let names = &live.request.strategies;
    Some(format!(
        "{}/{} {}",
        index + 1,
        names.len(),
        names.get(index as usize)?
    ))
}

fn show(call: Call, id: String, full: bool) -> Result<Done, Fail> {
    let run = call(Request::GetRun { id })?;
    if full {
        return Ok(Done::Json(run));
    }
    Ok(Done::Json(compact(&read(run)?)))
}

/// The ranking without the checks (spec 7.1); a saved run is ranked already.
fn compact(run: &TestRun) -> Value {
    let ranking: Vec<Value> = run
        .strategies
        .iter()
        .enumerate()
        .map(|(i, strategy)| {
            json!({
                "place": i + 1,
                "name": strategy.name,
                "score": strategy.score,
                "launch_error": strategy.launch_error,
            })
        })
        .collect();
    json!({
        "id": run.id,
        "suite": suite_name(run.suite),
        "canceled": run.canceled,
        "restore_error": run.restore_error,
        "ranking": ranking,
    })
}

/// `{"app": <version>}` once the app answers; when it is not running, its autostart task starts
/// it first (spec 4).
fn start() -> Result<Done, Fail> {
    let started = |version| Done::Json(json!({ "app": version }));
    if let Some(version) = running()? {
        return Ok(started(version));
    }
    let ran = exec::run(
        Process::new("schtasks.exe").args(["/Run", "/TN", cli::AUTOSTART_TASK]),
        START_WAIT,
        None,
    );
    if let Err(err) = ran
        .map_err(anyhow::Error::from)
        .and_then(exec::Output::check)
    {
        return Err(Fail::new(
            UNAVAILABLE,
            format!(
                "the autostart task «{}» did not run: start zapret-hub.exe yourself or turn \
                 autostart on in its Settings\n\n{err:#}",
                cli::AUTOSTART_TASK
            ),
        ));
    }
    let deadline = Instant::now() + START_WAIT;
    while Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(500));
        if let Some(version) = running()? {
            return Ok(started(version));
        }
    }
    Err(Fail::new(
        UNAVAILABLE,
        "Zapret Hub did not answer within 30 s of its task starting",
    ))
}

/// The app's version; `None` while there is no pipe.
fn running() -> Result<Option<Value>, Fail> {
    match send(&Request::AppVersion) {
        Err(err) if err.raw_os_error() == Some(ERROR_FILE_NOT_FOUND) => Ok(None),
        exchanged => answer(exchanged).map(Some),
    }
}
```

Почему так:
- `run` получает вызов приложения как `&mut dyn FnMut(Request) -> Result<Value, Fail>`: тесты подставляют фальшивое приложение, канал им не нужен.
- `needs_yes` и проверка значения `settings set` срабатывают до первого обращения к приложению: код 2 значит «ничего не отправлено».
- Ошибки clap печатаются через `render().to_string()`: без цветов, в JSON, с кодом 2. `--help` печатается как обычно, код 0.
- `start` запускает `schtasks.exe` через `exec::run`, как любой внешний процесс проекта: с таймаутом и без окна. Логгера у CLI нет, поэтому журнала `exec` здесь нет.
- `tests wait` повторяет ход только при смене `current`: строка на стратегию, а не на опрос.
- Компактный рейтинг берёт порядок прогона: сохранённый прогон уже отсортирован (core 7.4).

- [ ] **Шаг 5: Тесты проходят**

Run: `cargo test -p cli`
Expected: PASS: 7 тестов протокола, 16 тестов бинаря, 8 тестов канала.

- [ ] **Шаг 6: Бинарь без приложения**

Только эти команды: они не меняют систему ни с приложением, ни без него. Остальные команды CLI не запускать — при работающем Zapret Hub с включённым CLI они меняют zapret.

Run: `cargo run -q -p cli --bin zapret-hub-cli -- version; echo "exit $?"`
Expected: `{"app": null, "cli": "0.1.0", "protocol": 1}` с отступами (если на машине работает Zapret Hub с включённым CLI — вместо `null` его версия), `exit 0`.

Run: `cargo run -q -p cli --bin zapret-hub-cli -- service remove; echo "exit $?"`
Expected: `{"error": "the window asks to confirm this: add --yes"}`, `exit 2`.

Run: `cargo run -q -p cli --bin zapret-hub-cli -- tests run --timeout 5; echo "exit $?"`
Expected: `{"error": "error: the following required arguments were not provided:\n  --wait\n\nUsage: …"}`, `exit 2`.

Run: `cargo run -q -p cli --bin zapret-hub-cli -- --help; echo "exit $?"`
Expected: первая строка `Controls the running Zapret Hub. Prints one JSON document.`, дальше список команд и в конце `Exit codes: 0 done; …`; `exit 0`.

- [ ] **Шаг 7: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 8: Коммит**

```bash
git add Cargo.toml Cargo.lock crates/cli/Cargo.toml crates/cli/src/main.rs
git commit -m "feat(cli): add zapret-hub-cli with JSON output and fixed exit codes"
```

---

### Задача 5: Руководство и выпуск 0.2.0

**Files:**
- Create: `docs/cli.md`
- Modify: `README.md`, `CHANGELOG.md`, `tools/package.ps1`
- Modify: `Cargo.toml` (версия), `Cargo.lock`, `package.json`, `frontend/package.json`, `package-lock.json`

**Interfaces:**
- Consumes: команды, выводы и коды выхода `zapret-hub-cli` (задача 4); флажок и тексты окна (задача 3); `tools/package.ps1` и README из core (задача 21 core).
- Produces: `dist\zapret-hub-0.2.0.zip` с `zapret-hub.exe`, `zapret-hub-cli.exe`, `README.md` и `cli.md`; `/dist` уже в `.gitignore`.

- [ ] **Шаг 1: Руководство**

`docs/cli.md`:

````markdown
# zapret-hub-cli

`zapret-hub-cli.exe` — командная строка для работающего Zapret Hub. Она сделана для ИИ-агентов (Claude Code
и другие) и скриптов: каждая команда печатает один JSON-документ, коды выхода постоянные.

## Как это устроено

CLI сам ничего в системе не меняет и прав администратора не просит. Он передаёт команду работающему
приложению через локальный канал `\\.\pipe\zapret-hub-cli`. Приложение уже запущено с правами и выполняет
команду тем же кодом, что кнопки окна: через ту же очередь операций, со снимком для отката. Поэтому окно и трей
сразу показывают, что сделал агент, а тексты ошибок у CLI и окна одни и те же.

Каждая команда попадает в журнал приложения `data\logs\ГГГГ-ММ-ДД.log`: `cli: apply_strategy: ok` или
`cli: apply_strategy: <текст ошибки>`. Команды, которые приложение не выполняло, — строкой
`cli: refused: <причина>`.

Если агент прервёт CLI посреди долгой команды (применение стратегии, исправление, загрузка), приложение
доведёт её до конца. Итог покажет следующий `zapret-hub-cli status`.

## Как включить

По умолчанию CLI выключен. Включите его один раз: «Настройки» → «Командная строка» → флажок «Управление из
командной строки (zapret-hub-cli)» → «Сохранить». Переключение действует сразу, без перезапуска.

Пока флажок выключен, приложение отвечает на любую команду отказом
`the CLI is off: enable it in Zapret Hub → Settings` (код 3). Выключить CLI можно и из него самого
(`settings set cli_enabled false`), включить — только в окне.

Канал принимает команды от пользователя Windows, который запустил Zapret Hub, а также от SYSTEM и
администраторов. Программы других пользователей, программы из песочниц (низкий уровень целостности) и
подключения по сети в канал не пускаются.

Включённый CLI даёт любой программе, запущенной от вашего имени, те же действия, что окно, включая удаление
службы и правку `hosts`. Произвольный код через канал не запустить: список команд фиксирован.

## Вывод и коды выхода

stdout — один JSON-документ с отступами, в UTF-8. При успехе это данные команды, при ошибке —
`{"error": "…"}` с полным текстом ошибки. Исключение — `tests report`: он печатает текст отчёта.
Порядок ключей в объектах не значим.

| Код | Значение |
|---|---|
| 0 | успех |
| 1 | приложение выполнило команду с ошибкой, или прогон тестов завершился ошибкой |
| 2 | ошибка в аргументах или нет `--yes`; в приложение ничего не отправлено |
| 3 | приложение недоступно или отказало: не запущено, CLI выключен, канал занят дольше 10 с, версии CLI и приложения не совпадают |
| 4 | `tests wait` или `tests run --wait` упёрлись в `--timeout` |

Код 3 значит «чинить окружение, а не команду»: запустить приложение, включить CLI, обновить `zapret-hub.exe`
и `zapret-hub-cli.exe` вместе.

`zapret-hub-cli --help` и `zapret-hub-cli <команда> --help` перечисляют команды и аргументы.

PowerShell читает вывод программ в кодировке `[Console]::OutputEncoding`. Чтобы кириллица в JSON не
испортилась, выполните перед вызовами `[Console]::OutputEncoding = [Text.Encoding]::UTF8`.

## Команды

Значения перечислений пишутся через дефис (`ipset-exclude`, `remove-windivert`), ключи настроек — как в
`zapret-hub.toml` (`zapret_dir`). Имя стратегии — имя её файла без `.bat`, например `"general (ALT11)"`.

| Команда | Что делает | Команда приложения |
|---|---|---|
| `status` | служба, стратегия, последняя быстрая проверка, последний прогон | `get_overview` |
| `version` | версии CLI, протокола и приложения; без приложения `"app": null` | `app_version` |
| `start` | запускает приложение через задачу автозапуска и ждёт его до 30 с | — |
| `settings get` | все настройки | `get_settings` |
| `settings set <key> <value>` | меняет одну настройку (раздел «Настройки») | `get_settings`, `save_settings` |
| `proxy check [<addr>]` | проверяет прокси SOCKS5; без адреса — из настройки `socks5` | `check_proxy` |
| `candidates` | папки zapret и прокси SOCKS5, найденные на компьютере | `wizard_candidates` |
| `strategies list` | стратегии в порядке меню `service.bat` с последним результатом; `error` — файл не стратегия | `list_strategies` |
| `strategies show <name>` | аргументы, последний результат, у своей стратегии — её запись `record` | `get_strategy` |
| `apply <name>` | ставит стратегию службой, делает быструю проверку и печатает её итог | `apply_strategy` |
| `apply-best` | то же для лучшей стратегии последнего стандартного прогона | `apply_best` |
| `restart` | перезапускает службу и делает быструю проверку | `restart_zapret` |
| `quick-check` | быстрая проверка: работает ли zapret сейчас | `quick_check` |
| `custom clone <source> <name>` | копирует стратегию в `general (HUB <name>)` и печатает её запись | `clone_strategy` |
| `custom validate <file\|->` | проверяет запись (`winws --dry-run`), не сохраняя | `validate_custom` |
| `custom save <file\|-> [--create]` | сохраняет запись; `--create` — как новую стратегию | `save_custom` |
| `custom delete <name> --yes` | удаляет свою стратегию; `<name>` — её собственное имя: `mine` для `general (HUB mine)` | `delete_custom` |
| `tests run [--dpi] [--wait] [--timeout <s>] [<name>…]` | запускает прогон (раздел «Тесты») | `start_tests` |
| `tests status` | идёт ли прогон, его ход, итог последнего | `tests_status` |
| `tests wait [--timeout <s>]` | ждёт конца прогона и печатает рейтинг | `tests_status` |
| `tests cancel` | отменяет прогон | `cancel_tests` |
| `tests list` | сохранённые прогоны | `list_runs` |
| `tests show <id> [--full]` | рейтинг прогона; `--full` — прогон целиком, со всеми проверками | `get_run` |
| `tests report <id>` | прогон текстом в формате `test zapret.ps1` | `run_report` |
| `flowseal` | игровой фильтр, ipset, фейки, свои списки, флаг обновлений | `get_flowseal` |
| `flowseal check-update` | новая версия Flowseal или `null` | `check_flowseal_update` |
| `game-filter <disabled\|all\|tcp\|udp> [--tcp <ports>] [--udp <ports>]` | игровой фильтр; не указанные порты остаются прежними | `set_game_filter` |
| `ipset mode <loaded\|none\|any>` | режим ipset, как в `service.bat` | `set_ipset_mode` |
| `ipset update` | скачивает `ipset-all.txt` заново | `update_ipset` |
| `check-updates <on\|off>` | собственная проверка обновлений Flowseal | `set_check_updates` |
| `fake <discord\|game> <name>` | делает `bin\<name>.bin` активным фейком; имена — в выводе `flowseal`, поле `fakes.files` | `replace_fake` |
| `lists add <list> <entry>`, `lists remove <list> <entry>` | свои списки: `general`, `exclude`, `ipset-exclude` | `add_list_entry`, `remove_list_entry` |
| `hosts plan` | что изменит запись блока в `hosts`; ничего не пишет | `hosts_plan` |
| `hosts apply --yes` | пишет блок zapret-hub в `hosts` | `apply_hosts` |
| `checks` | диагностика `service.bat` | `system_checks` |
| `checks fix <action>` | исправление: `enable-tcp-timestamps`; `remove-windivert` и `remove-conflicts` — с `--yes` | `fix_check` |
| `discord clear-cache --yes` | закрывает Discord и чистит его кэш | `clear_discord_cache` |
| `service remove --yes` | удаляет службу zapret и WinDivert | `remove_service` |

`--yes` повторяет подтверждения окна и ставится в любом месте команды. Без него эти команды не отправляются:
код 2.

Команд окна, которых нет в CLI:
- `pick_folder`, `open_url`, `open_folder` — открывают окна и проводник;
- `check_folder` — открыл бы путь, переданный клиентом; `settings set zapret_dir` проверяет папку сам;
- `take_notices` — забрал бы уведомления у окна;
- `test_live` — его заменяет `tests status`.

## Настройки

`settings set <key> <value>` читает все настройки, меняет одну и сохраняет их, как кнопка «Сохранить» в окне:
приложение проверяет папку zapret и адрес прокси, включает или выключает автозапуск. Окно сразу показывает
новые значения.

| Ключ | Значение |
|---|---|
| `zapret_dir` | путь к папке Flowseal; `""` — не задана |
| `socks5` | `адрес:порт`, например `127.0.0.1:2080`; `""` — без прокси |
| `autostart` | `true` или `false` |
| `start_minimized` | `true` или `false`: при автозапуске оставаться в трее |
| `theme` | `system`, `light` или `dark` |
| `cli_enabled` | `false` выключает CLI; включить его из CLI нельзя |

Неизвестный ключ или неверное значение — код 2 со списком допустимых; в приложение ничего не отправляется.

```
zapret-hub-cli settings set socks5 127.0.0.1:2080
zapret-hub-cli settings set socks5 ""
```

## Тесты

`tests run` без имён запускает все стратегии из `strategies list` в их порядке, кроме строк с `error`; с именами
— только названные, в указанном порядке. `--dpi` — набор DPI вместо стандартного. Без `--wait` команда
возвращается сразу:

```json
{
  "started": true,
  "strategies": 22,
  "suite": "standard"
}
```

`tests wait` и `tests run --wait` раз в 2 с спрашивают приложение о прогоне и пишут ход в stderr, по строке на
стратегию:

```
3/22 general (ALT2)
```

Когда прогон закончится, stdout получает компактный рейтинг. Порядок — как в прогоне: он уже отсортирован,
лучшая стратегия первая. `score` — счёт стратегии целиком (`ok` из `total` проверок прошли, `median_ms` —
медианная задержка и другие поля, как в `tests show --full`).

```
{
  "canceled": false,
  "id": "2026-10-11_10-00-00",
  "ranking": [
    {
      "launch_error": null,
      "name": "general (ALT11)",
      "place": 1,
      "score": { "ok": 36, "total": 36, "median_ms": 184, … }
    },
    …
  ],
  "restore_error": null,
  "suite": "standard"
}
```

- `launch_error` не `null` — стратегия не запустилась, она в конце рейтинга. `restore_error` — после прогона
  что-то не вернулось на место, подробности в журнале. `canceled` — прогон отменён.
- Прогон завершился ошибкой (например, папка zapret не найдена) — `{"error": "…"}`, код 1.
- Прогона сейчас нет — печатается итог последнего с запуска приложения: его рейтинг или ошибка; если прогонов
  не было — `{"last": null}`.
- `--timeout <s>` ограничивает ожидание: по истечении — код 4, а прогон продолжается; ждите дальше `tests wait`.
- Во время прогона приложение не меняет zapret: `apply`, `restart` и другие команды, которые его меняют,
  отвечают ошибкой (код 1). `tests status` и `tests cancel` работают.

`tests status` печатает `{"testing": …, "live": …, "last": …}`: `live` — ход прогона, как его видит окно,
`last` — `{"run_id": "…"}` или `{"error": "…"}`.

## Рецепт: подобрать и применить лучшую стратегию

```
zapret-hub-cli start
zapret-hub-cli tests run --wait --timeout 540
zapret-hub-cli tests wait --timeout 540
zapret-hub-cli apply-best
zapret-hub-cli quick-check
```

1. `start` поднимает приложение, если оно не запущено (раздел «Если приложение не запущено»).
2. `tests run --wait` запускает стандартный набор по всем стратегиям и ждёт рейтинг. Около 15 с на стратегию:
   22 стратегии Flowseal 1.10.3 — около 6 минут. У агента с пределом времени на команду (у Claude Code — 10
   минут) `--timeout 540` оставляет запас. Код 4 — прогон ещё идёт: повторяйте `tests wait --timeout 540`, пока
   не будет 0 или 1.
3. `apply-best` ставит первую стратегию последнего завершённого стандартного прогона (не отменённого).
4. `quick-check` подтверждает, что zapret работает: `"ok": true`.

## Свои стратегии

```
zapret-hub-cli custom clone "general (ALT11)" mine
```

Команда печатает запись новой стратегии `general (HUB mine)`: `name`, `cloned_from`, `wf`, `blocks`. Позже ту же
запись даёт `strategies show "general (HUB mine)"`, поле `record`. Сохраните её в файл, поправьте `wf` или
`blocks` и проверьте:

```
zapret-hub-cli custom validate mine.json
zapret-hub-cli custom save mine.json
```

- `-` вместо файла читает запись из stdin.
- `--create` сохраняет запись как новую стратегию; без него меняется существующая с тем же `name`.
- Файл — UTF-8; BOM, который ставят Блокнот и `Set-Content -Encoding UTF8` в Windows PowerShell 5.1,
  допускается.
- JSON не той формы — код 2, в приложение ничего не отправлено. Аргументы не приняты `winws --dry-run` — код 1
  с текстом ошибки.

Удалить: `zapret-hub-cli custom delete mine --yes`.

## Если приложение не запущено

Команды, которым нужно приложение, отвечают кодом 3:
``Zapret Hub is not running: start it with `zapret-hub-cli start` or run zapret-hub.exe``.

`zapret-hub-cli start` запускает задачу Планировщика «Zapret Hub». Её создаёт автозапуск приложения, и она
запускает приложение с правами администратора без запроса UAC. Потом `start` до 30 с ждёт, пока приложение
ответит, и печатает `{"app": "0.2.0"}`; если приложение уже работает — сразу.

Если автозапуск выключен, задачи нет: `start` выходит с кодом 3 и советует запустить `zapret-hub.exe` вручную
или включить автозапуск в настройках. Запуск exe вручную требует подтверждения UAC: его делает человек.

## Ограничения

- Канал один на компьютер. Если его имя уже занято другой программой (или Zapret Hub, запущенным другим
  пользователем), приложение показывает уведомление «Канал CLI занят другой программой» и работает без CLI.
- Канал пускает пользователя, от имени которого запущено приложение. Если в окне UAC вводили пароль другой
  учётной записи, приложение работает от неё, и CLI обычного пользователя получает отказ (код 3).
- Итог последнего прогона (`last`) хранится в памяти приложения: после перезапуска он `null`.
- `version` печатает `"app": null` и тогда, когда приложение отказало (например, CLI выключен).

## Проверка вживую

Автотесты приложение не запускают: ему нужны права администратора. Перед выпуском пройдите список вручную.
Приложение — из повышенного терминала (`npm run tauri dev`) или готовым exe; CLI — из обычного терминала:
`cargo run -q -p cli --bin zapret-hub-cli -- <команда>` или `zapret-hub-cli.exe <команда>`. Код выхода —
`echo $?` в Git Bash или `$LASTEXITCODE` в PowerShell. Каждый пункт сверяется с журналом `data\logs`.

1. CLI выключен: `status` → код 3, `{"error": "the CLI is off: enable it in Zapret Hub → Settings"}`; в журнале
   `cli: refused: the CLI is off…`.
2. Флажок «Управление из командной строки» включён и сохранён: `status` → код 0, обзор; в журнале
   `cli: get_overview: ok`.
3. `version` → `{"app": "0.2.0", "cli": "0.2.0", "protocol": 1}`, код 0.
4. `apply "general (ALT2)"` → код 0; окно и иконка трея сами показывают новую стратегию.
5. `service remove` → код 2, служба на месте. `service remove --yes` → код 0, `sc query zapret` службу не
   находит. `apply "general (ALT2)"` возвращает её.
6. `tests run --wait general "general (ALT2)"` → ход в stderr, рейтинг из двух строк, код 0.
   `tests run --wait --timeout 5` → код 4; затем `tests wait` → рейтинг, код 0.
7. Во время прогона `apply general` → код 1 с ошибкой про прогон; `tests status` отвечает сразу.
8. `custom clone general mine` → запись; `general (HUB mine)` есть в `strategies list`. Запись в файл →
   `custom validate` → код 0; файл `{"name": 1}` → код 2; `custom delete mine --yes` → код 0.
9. `settings set theme dark` → окно сразу тёмное. `settings set cli_enabled false` → код 0; флажок на вкладке
   «Настройки» снят (если вкладка была открыта — после перехода на неё заново); следующий `status` → код 3.
10. Выход из приложения через трей: `status` → код 3, `Zapret Hub is not running…`. С включённым автозапуском
    `start` → `{"app": "0.2.0"}` не позже чем через 30 с, приложение в трее; с выключенным — код 3 и совет.
11. Занятое имя: выйти из приложения, в обычном PowerShell выполнить
    `$p = [System.IO.Pipes.NamedPipeServerStream]::new('zapret-hub-cli')`, запустить приложение → уведомление
    «Канал CLI занят другой программой», в журнале `cli: \\.\pipe\zapret-hub-cli is not open: …`. Затем
    `$p.Dispose()`, перезапуск приложения → `status` снова отвечает.
````

- [ ] **Шаг 2: README**

В `README.md` четыре правки.

1. В «Что умеет» после пункта «**Трей:** …» (две строки):

```markdown
- **Командная строка:** `zapret-hub-cli.exe` для ИИ-агентов и скриптов — те же действия, что в окне, вывод в JSON.
  Включается в настройках.
```

2. В «Что приложение меняет в системе» после строки про `HKCU\Software\Classes\AppUserModelId\ZapretHub`:

```markdown
- Именованный канал `\\.\pipe\zapret-hub-cli`, пока приложение работает. Команды через него выполняются, только
  если в настройках включено «Управление из командной строки».
```

3. Между разделами «Тесты стратегий» и «Антивирусы» новый раздел:

````markdown
## Командная строка для агентов

`zapret-hub-cli.exe` лежит рядом с `zapret-hub.exe` и передаёт команды работающему приложению: ИИ-агент
(Claude Code и другие) или скрипт смотрит состояние, применяет стратегии, меняет списки и флаги, запускает тесты
и читает рейтинг. Вывод — JSON, коды выхода постоянные, прав администратора CLI не просит.

По умолчанию CLI выключен: включите «Управление из командной строки» в настройках. После этого любая
программа, запущенная от вашего имени, сможет менять zapret через приложение. Проверка: `zapret-hub-cli status`.

Подобрать и применить лучшую стратегию:

```
zapret-hub-cli tests run --wait
zapret-hub-cli apply-best
zapret-hub-cli quick-check
```

Все команды, коды выхода и рецепты — в `cli.md` рядом с exe (в исходниках — `docs/cli.md`) и в
`zapret-hub-cli --help`.
````

4. В «Сборка из исходников» последний абзац (от «Готовый exe —» до конца файла) заменить на:

```markdown
Готовый exe — `target\release\zapret-hub.exe`. CLI — отдельный пакет: `cargo build --release -p cli` собирает
`target\release\zapret-hub-cli.exe`. `tools\package.ps1` собирает оба и кладёт в `dist\zapret-hub-<версия>.zip`
вместе с README и `cli.md`. `npm run tauri dev` запускается из терминала с правами администратора; сборка,
тесты, `npm run check` и `zapret-hub-cli` — из обычного.
```

- [ ] **Шаг 3: CHANGELOG**

В `CHANGELOG.md` между строкой `Версии по [semver](https://semver.org/lang/ru/).` и `## 0.1.0`:

```markdown
## 0.2.0

- `zapret-hub-cli.exe` — командная строка для ИИ-агентов и скриптов: все действия окна, кроме чисто оконных,
  ожидание прогона тестов с компактным рейтингом, запуск приложения через задачу автозапуска. Вывод — JSON,
  коды выхода постоянные. Включается флажком «Управление из командной строки» в настройках, по умолчанию
  выключено. Руководство — `cli.md` в архиве.

```

- [ ] **Шаг 4: Версия 0.2.0**

Заменить `0.1.0` на `0.2.0`:
- `Cargo.toml`, `[workspace.package]`: `version = "0.1.0"` → `version = "0.2.0"`;
- `package.json` и `frontend/package.json`: `"version": "0.1.0",` → `"version": "0.2.0",`;
- `package-lock.json`: три строки `"version": "0.1.0",` — у корня файла, у `packages[""]` и у `packages["frontend"]` (строки 3, 9 и 23).

Run: `grep -n '0\.1\.0' Cargo.toml package.json frontend/package.json package-lock.json`
Expected: пусто.

Run: `cargo check --workspace`
Expected: код 0; `Cargo.lock` обновился сам.

Run: `git diff Cargo.lock`
Expected: только версии шести пакетов workspace: `cli`, `exec`, `probe`, `tester`, `zapret`, `zapret-hub`.

- [ ] **Шаг 5: Архив**

`tools/package.ps1` целиком:

```powershell
# Builds the release app and CLI and packs them with the README and the CLI guide into
# dist\zapret-hub-<version>.zip.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    # The first `version = "..."` line of the root Cargo.toml is [workspace.package]'s.
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"$' | Select-Object -First 1).Matches[0].Groups[1].Value
    npm run tauri build
    if ($LASTEXITCODE -ne 0) { throw "npm run tauri build exited with $LASTEXITCODE" }
    # tauri build makes the app's package only; the CLI is a package of its own.
    cargo build --release -p cli
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release -p cli exited with $LASTEXITCODE" }

    $stage = Join-Path $root "dist\zapret-hub-$version"
    $zip = "$stage.zip"
    Remove-Item -Recurse -Force $stage, $zip -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item target\release\zapret-hub.exe, target\release\zapret-hub-cli.exe, README.md, docs\cli.md -Destination $stage
    Compress-Archive -Path "$stage\*" -DestinationPath $zip
    Write-Output "packed $zip"
} finally {
    Pop-Location
}
```

`npm run tauri build` собирает только пакет приложения, поэтому CLI собирается отдельной командой. `docs\cli.md` ложится в архив как `cli.md` рядом с exe — на него ссылается README.

- [ ] **Шаг 6: Автоматические проверки целиком**

В обычном терминале, на ненагруженной машине, по очереди:

Run: `cargo fmt --all -- --check`
Expected: код 0, вывода нет.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

Run: `cargo test --workspace`
Expected: все тесты проходят, среди них `cli` (7 + 16 + 8) и `zapret_hub_lib` (14); тесты, которым нужны права администратора, служба или `E:\Zapret`, перечислены как `ignored`.

Run: `npm run check`
Expected: `svelte-check found 0 errors and 0 warnings`.

Run: `npm test`
Expected: PASS, 10 тестов.

- [ ] **Шаг 7: Сборка архива**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File tools/package.ps1`
Expected: последняя строка `packed …\dist\zapret-hub-0.2.0.zip`. Релизная сборка с LTO идёт несколько минут.

Run (PowerShell): `(Get-ChildItem dist\zapret-hub-0.2.0).Name`
Expected: `cli.md`, `README.md`, `zapret-hub-cli.exe`, `zapret-hub.exe`.

Run: `dist/zapret-hub-0.2.0/zapret-hub-cli.exe version; echo "exit $?"`
Expected: `"cli": "0.2.0"`, `"protocol": 1`, `exit 0`.

Run (PowerShell): `Select-String -Path target\release\zapret-hub-cli.exe -Pattern "requireAdministrator" -SimpleMatch -Quiet`
Expected: `False`: CLI запускается без прав. Та же команда для `target\release\zapret-hub.exe` — `True`.

- [ ] **Шаг 8: Коммит**

```bash
git add docs/cli.md README.md CHANGELOG.md tools/package.ps1 Cargo.toml Cargo.lock package.json frontend/package.json package-lock.json
git commit -m "build(app): release 0.2.0 with zapret-hub-cli and its guide"
```

Ручная проверка с приложением — раздел «Проверка вживую» в `docs/cli.md`. Ей нужны права администратора, поэтому её проводит пользователь после слияния, а не исполнитель.

---

## Расхождения со спецификацией

Решений спецификации, которые не работают как написано, нет. Ниже — уточнения там, где спецификация молчит; все они уже в коде плана.

1. `save_settings` в протоколе несёт `settings` как JSON (`serde_json::Value`): тип `Settings` живёт в `src-tauri`, а `cli` от приложения не зависит. Приложение разбирает настройки на входе; ошибка — `ok:false`, код 1.
2. Зависимости `cli` шире раздела 3 (`zapret`, `tester`): ещё `exec` (`start` запускает `schtasks` через `exec::run`, как любой внешний процесс), `anyhow` (тип ошибки `exec`), `clap`, `windows`, `serde`, `serde_json`.
3. Порядок проверок запроса: выключен → размер → версия → разбор. Версия читается раньше разбора, чтобы неизвестная команда от CLI новой версии выглядела как несовпадение версий.
4. `tests run --timeout` без `--wait` — ошибка аргументов, код 2: ждать нечего.
5. `tests wait` без идущего прогона печатает рейтинг последнего прогона с запуска приложения, его ошибку (код 1) или `{"last": null}`.
6. `suite` в собственном выводе CLI (`tests run`, компактный рейтинг) — `standard` или `dpi`, как в примере спецификации 7.1. `data` команд приложения (`tests show --full`, `tests status`) печатается как есть, там `"Standard"`.
7. `AppState::end_testing(outcome)` ставит итог и гасит `live` раньше, чем `testing` становится `false` (задача 3).
8. Окно перечитывает настройки на каждое `StateChanged`. Без этого настройки, сохранённые агентом, окно не видит (спецификация 8 обещает обратное), а «Сохранить» в окне вернуло бы прежние.
9. Файл `CustomStrategy` может начинаться с BOM (Блокнот, `Set-Content -Encoding UTF8` в Windows PowerShell 5.1): он пропускается.
10. os error 5 при подключении — код 3 с объяснением, кого пускает канал.
11. `version` печатает `"app": null` и тогда, когда приложение отказало (CLI выключен).
12. `docs/cli.md` кладётся в архив рядом с exe как `cli.md`: на него ссылается README.

Ограничения, которые следуют из спецификации:
- Приложение, повышенное чужой учётной записью (в окне UAC вводили пароль администратора), пускает в канал только эту запись: SID берётся из токена процесса (раздел 5). CLI обычного пользователя получает отказ, код 3. Записано в `docs/cli.md`.
- `GRGW` для канала включает `FILE_CREATE_PIPE_INSTANCE`: программа того же пользователя может создать свой экземпляр канала и отвечать CLI вместо приложения. Это в пределах остаточного риска разделов 5 и 11: выполнить что-либо от имени приложения такая программа не может, а CLI открывает канал с `SECURITY_IDENTIFICATION`.
