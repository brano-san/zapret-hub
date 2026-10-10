# Zapret Hub — каркас, менеджер zapret и тесты стратегий: план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Цель:** портативное приложение в трее (Tauri v2 + Svelte 5), которое управляет папкой Flowseal-zapret вместо `service.bat`, гоняет тесты стратегий как `test zapret.ps1`, ранжирует их и ставит лучшую в один клик.

**Архитектура:** Cargo workspace из четырёх крейтов с линейными зависимостями `exec ← probe ← zapret ← tester`, и `src-tauri` поверх них. `exec` — запуск внешних программ с таймаутом, отменой и журналом. `probe` — сетевые проверки. `zapret` — модель папки Flowseal, служба, файлы настроек, обновления, системные проверки, свои стратегии, снимок состояния. `tester` — прогон, рейтинг, история. `src-tauri` — IPC, трей, окно, автозапуск, уведомления, фоновые задачи; тело команды ~10 строк. Фронтенд — Svelte 5 (руны) без UI-библиотек.

**Стек:** Rust 1.99.0 (edition 2024), Tauri 2.12, tauri-specta/specta `=2.0.0-rc.25`, specta-typescript 0.0.12, windows 0.62, windows-registry 0.6, sysinfo 0.39, toml 1.1, sha2 0.10, tauri-plugin-dialog 2.8, tauri-plugin-single-instance 2.5; Svelte 5.57.1, Vite 8.3.2, @sveltejs/vite-plugin-svelte 7.3.1, TypeScript ~6.0.3, svelte-check 4.7.6, vitest 5.0.3, @tauri-apps/api и @tauri-apps/cli 2.12.1.

**Спецификация:** `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md`. Исполнитель читает её вместе с планом.

## Общие ограничения

- Только Windows 10/11 x64. Портативный exe без установщика, манифест `requireAdministrator`.
- Крейты не зависят от `tauri`: только `serde` и `specta`-derive для DTO. Логика в крейтах, в `src-tauri` — маршрутизация IPC, трей, окно, автозапуск.
- Данные рядом с exe: `zapret-hub.toml`, `data/strategies/*.toml`, `data/tests/*.json`, `data/cache/dpi-suite.json`, `data/state/pending.json`, `data/logs/YYYY-MM-DD.log`.
- Флаги, списки, служба и значение реестра `zapret-discord-youtube` — в тех же местах и форматах, что у `service.bat`. Оригинальные `service.bat` и `test zapret.ps1` продолжают работать.
- Каждый внешний процесс запускается через `exec::run`: таймаут, без окна, в журнал пишутся команда, код выхода, полный stdout и stderr. Ничего не обрезается. Исключение — долгоживущий winws: тестовый (им владеет `procs::Winws`, он же его останавливает и собирает вывод) и ручной winws, возвращаемый из снимка (`procs::spawn_detached`). Они запускаются напрямую, без окна, а командная строка и pid пишутся в журнал (задача 6).
- Изменяющие операции идут строго по одной. Смена стратегии, тесты, ipset, игровой фильтр и удаление службы делают снимок до первого изменения и откатываются по нему при ошибке.
- Рейтинг стандартного набора: (запуск не удался) → число не-OK проверок по возрастанию → медиана `time_total` успешных проверок → порядок меню `service.bat` (естественная сортировка имён). ICMP-пинг показывается отдельно и в сортировке не участвует. Отличие от спецификации 7.4 обосновано в задаче 11.
- Интерфейс на русском, все строки фронтенда в `frontend/src/lib/strings.ts`, строки трея и уведомлений — в `src-tauri/src/texts.rs`. Тема светлая/тёмная по системе, в настройках можно зафиксировать.
- DTO, которые идут через IPC, не используют `u64`/`i64`/`usize`: specta-typescript запрещает BigInt. Используются `u32`, `f64`, `String`.
- Поля DTO в snake_case, как в Rust (без `rename_all`). Те же имена в TOML и JSON на диске.
- Код, комментарии и коммиты на английском, документы в `docs/` на русском. Коммит — одна строка Conventional Commits, scope = крейт (`exec`, `probe`, `zapret`, `tester`, `app`, `ui`, `docs`), без тела и без трейлеров.
- Тесты, которым нужны права администратора, служба, WinDivert или папка `E:\Zapret`, помечены `#[ignore]` и запускаются вручную.
- `npm run tauri dev` и готовый exe запускаются только из повышенного терминала: exe помечен `requireAdministrator`. Всё остальное — сборка, тесты, `cargo run -p zapret-hub --bin export-bindings`, `npm run check` — работает в обычном терминале: манифест с повышением получает только `zapret-hub.exe` (задача 13).
- После каждого изменения команд или DTO: `cargo run -p zapret-hub --bin export-bindings` и коммит `frontend/src/lib/ipc/bindings.ts` вместе с изменением.
- Код в плане не отформатирован rustfmt. Перед каждым коммитом с Rust-кодом — `cargo fmt --all` (настройки по умолчанию, без `rustfmt.toml`); `cargo clippy --workspace --all-targets -- -D warnings` тоже чистый на каждом коммите. Переформатирование — не отклонение от плана.

## На что смотреть при ревью

1. **Приложение убито посреди прогона** (диспетчер задач, сбой питания). Следующий запуск находит `pending.json`, возвращает ipset, службу и ручные winws и сообщает об этом. Тест: `restore_files` возвращает ipset после `enter_test_any` (задача 10), разбор `pending.json` (задача 10).
2. **Папку zapret перенесли или удалили после настройки.** Каждая команда отвечает понятной ошибкой «папка не найдена: нет …», без паники. Тест: `Folder::open` перечисляет недостающее (задача 3).
3. **winws стратегии сразу завершается** (плохие аргументы, WinDivert занят). Это ошибка запуска: стратегия уходит в конец рейтинга, прогон продолжается. Тест: сортировка ставит `launch_error` последней (задача 11).
4. **Сеть полностью лежит во время тестов.** Все проверки `ERROR`, ничего не висит дольше таймаутов, порядок детерминирован: как в меню `service.bat`. Тесты: таймаут `exec::run` (задача 1), равные счёты идут в порядке меню (задача 11).
5. **Кириллица в пути zapret или в выводе консольных программ.** Парсер сохраняет путь как есть, журнал читает OEM-кодировку. Тесты: `parse` с корнем `C:\Запрет\` (задача 4), `decode` для cp866 и UTF-16 (задача 1).

---

## Структура файлов

```
Cargo.toml                       workspace, общие версии зависимостей
rust-toolchain.toml              1.99.0
package.json                     npm workspace + @tauri-apps/cli
.gitignore
README.md                        (RU) установка, запуск, где данные, антивирусы
CHANGELOG.md
tools/
  gen-bat-fixtures.cmd           эталоны парсера: копия разбора из service.bat
  make-icon.ps1                  рисует src-tauri/icons/icon.ico
  package.ps1                    сборка + zip
crates/
  exec/src/lib.rs                run(), Output, decode(), CREATE_NO_WINDOW
  exec/src/time.rs               LocalTime (GetLocalTime)
  exec/tests/run.rs
  probe/src/lib.rs
  probe/src/curl.rs              проверки test zapret.ps1, DPI-запрос, загрузки
  probe/src/ping.rs              IcmpSendEcho
  probe/src/socks.rs             рукопожатие SOCKS5, проверка через curl, порты-кандидаты
  probe/tests/{curl,socks,ping}.rs
  zapret/src/lib.rs              read_text, sha256_hex, модули
  zapret/src/folder.rs           Folder, проверка, версия, кандидаты
  zapret/src/strategies.rs       список *.bat в порядке service.bat
  zapret/src/targets.rs          utils\targets.txt
  zapret/src/batparse.rs         порт разбора :service_install
  zapret/src/game_filter.rs      utils\game_filter.enabled
  zapret/src/ipset.rs            режимы ipset-all.txt, тестовое переключение, установка загрузки
  zapret/src/user_lists.rs       *-user.txt
  zapret/src/fakes.rs            ACTIVE_*.bin
  zapret/src/flags.rs            utils\check_updates.enabled
  zapret/src/procs.rs            процессы (sysinfo), winws для тестов
  zapret/src/service.rs          SCM, sc/net, установка, быстрая проверка, TCP timestamps
  zapret/src/hosts.rs            блок zapret-hub в hosts
  zapret/src/updates.rs          версия, ipset-all, hosts из репозитория Flowseal
  zapret/src/checks.rs           проверки :service_diagnostics
  zapret/src/discord_cache.rs    :clear_discord_cache
  zapret/src/custom.rs           свои стратегии: TOML, .bat, --dry-run
  zapret/src/snapshot.rs         снимок и откат
  zapret/tests/common/mod.rs     фальшивая папка Flowseal
  zapret/tests/*.rs
  zapret/tests/fixtures/         *.bat + *.args + ROOT.txt (эталоны)
  tester/src/lib.rs
  tester/src/model.rs            Suite, результаты, Score, TestRun
  tester/src/rank.rs             счёт, профили, сортировка
  tester/src/history.rs          data/tests/*.json
  tester/src/report.rs           текст в формате test zapret.ps1
  tester/src/dpi.rs              набор hyperion-cs + кэш
  tester/src/run.rs              процедура 7.3, par_map
  tester/src/catalog.rs          строки и подробности для вкладки «Стратегии»
  tester/tests/*.rs
src-tauri/
  Cargo.toml, tauri.conf.json, capabilities/default.json, icons/icon.ico
  build.rs                       манифесты через компоновщик: Common-Controls v6 всем, requireAdministrator только exe
  common-controls.manifest
  src/main.rs
  src/bin/export-bindings.rs     привязки IPC без запуска приложения и без повышения
  src/lib.rs                     specta_builder(), export_bindings(), run(), fatal(), проверка WebView2
  src/settings.rs                zapret-hub.toml
  src/state.rs                   Paths, AppState: очередь операций, флаг тестов, уведомления
  src/journal.rs                 log::Log → data/logs
  src/dto.rs                     Overview, FlowsealState, LiveRun, Notice, события IPC
  src/commands.rs                все команды IPC и blocking()
  src/actions.rs                 общие действия команд, трея и фоновых задач
  src/jobs.rs                    прогон тестов в фоне, старт, быстрая проверка раз в 30 мин, версия раз в сутки
  src/tray.rs                    иконка (рисуется в коде), меню
  src/window.rs                  создание/показ окна
  src/notify.rs                  тосты WinRT + регистрация AUMID
  src/autostart.rs               задача Планировщика из XML
  src/texts.rs                   русские строки трея и уведомлений
frontend/
  package.json, vite.config.ts, svelte.config.js, tsconfig.json, index.html
  src/main.ts, src/App.svelte, src/app.css
  src/lib/ipc/bindings.ts        генерируется (tauri-specta)
  src/lib/api.ts                 call(): ошибка → очередь уведомлений
  src/lib/notices.svelte.ts      очередь уведомлений (+ тест)
  src/lib/confirm.svelte.ts      модальное подтверждение
  src/lib/app.svelte.ts          общее состояние: настройки, обзор, вкладка, живой прогон
  src/lib/strings.ts             все строки интерфейса
  src/lib/sort.ts                сортировка таблиц (+ тест)
  src/lib/format.ts              мс, даты прогонов, сводка блока (+ тест)
  src/components/Notices.svelte, ConfirmDialog.svelte, TargetChecks.svelte, ResultsTable.svelte, ListEditor.svelte
  src/tabs/Overview.svelte, Strategies.svelte, StrategyEditor.svelte, Tests.svelte, Zapret.svelte, Settings.svelte
  src/Wizard.svelte
```

Отличия от раздела 4.1 спецификации, которые план вносит в спецификацию в задаче 13:
- Крейты-заглушки `diag` и `router` и скрытые вкладки не создаются: добавить крейт позже ничего не ломает, а пустой код ничего не проверяет.
- Появился крейт `exec`: запуск процессов нужен и `probe`, и `zapret`.
- Зависимость `zapret → probe`: быстрая проверка после установки и загрузки обновлений нужны модулю `zapret`.

---

### Задача 1: Workspace и крейт `exec`

**Files:**
- Create: `Cargo.toml`, `rust-toolchain.toml`, `.gitignore`
- Create: `crates/exec/Cargo.toml`, `crates/exec/src/lib.rs`, `crates/exec/src/time.rs`
- Test: `crates/exec/tests/run.rs`

**Interfaces:**
- Consumes: —
- Produces:
  - `exec::run(cmd: &mut std::process::Command, timeout: Duration, cancel: Option<&AtomicBool>) -> std::io::Result<exec::Output>`
  - `exec::Output { command: String, code: Option<i32>, stdout: String, stderr: String, timed_out: bool, canceled: bool, elapsed: Duration }` с методами `success() -> bool`, `check(self) -> anyhow::Result<Output>`, `describe(&self) -> String`
  - `exec::decode(&[u8]) -> String`
  - `exec::CREATE_NO_WINDOW: u32`
  - `exec::LocalTime::now()`, `.date() -> "2026-10-10"`, `.stamp() -> "2026-10-10_05-03-34"`, `.clock() -> "05:03:34"`
  - Журнал: каждый `run` пишет `log::info!(target: "exec", …)` с `describe()`.

- [ ] **Шаг 1: Workspace**

`Cargo.toml`:

```toml
[workspace]
resolver = "3"
members = ["crates/*"]

[workspace.package]
version = "0.1.0"
edition = "2024"
rust-version = "1.99"
license = "MIT"
publish = false

[workspace.dependencies]
exec = { path = "crates/exec" }
probe = { path = "crates/probe" }
zapret = { path = "crates/zapret" }
tester = { path = "crates/tester" }

anyhow = "1.0"
log = "0.4"
serde = { version = "1.0", features = ["derive"] }
serde_json = "1.0"
sha2 = "0.10"
specta = { version = "=2.0.0-rc.25", features = ["derive", "function"] }
specta-typescript = "0.0.12"
sysinfo = { version = "0.39", default-features = false, features = ["system"] }
tempfile = "3.27"
toml = "1.1"
windows = "0.62"
windows-registry = "0.6"

tauri = { version = "2.12", features = ["specta", "tray-icon"] }
tauri-build = "2.7"
tauri-specta = { version = "=2.0.0-rc.25", features = ["derive", "typescript"] }
tauri-plugin-dialog = "2.8"
tauri-plugin-single-instance = "2.5"

[profile.release]
lto = true
codegen-units = 1
strip = true
```

`rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.99.0"
components = ["clippy", "rustfmt"]
profile = "minimal"
```

`.gitignore`:

```
/target
/node_modules
/frontend/node_modules
/frontend/dist
/src-tauri/gen
/dist
/zapret-hub.toml
/data
```

`crates/exec/Cargo.toml`:

```toml
[package]
name = "exec"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "Runs external programs for Zapret Hub: no console window, timeout, cancel, journal."

[dependencies]
anyhow.workspace = true
log.workspace = true
windows = { workspace = true, features = ["Win32_Foundation", "Win32_Globalization", "Win32_System_SystemInformation"] }
```

- [ ] **Шаг 2: Падающие тесты**

`crates/exec/tests/run.rs`:

```rust
use std::process::Command;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[test]
fn captures_stdout_and_the_exit_code() {
    let out = exec::run(Command::new("cmd").args(["/c", "echo hello& exit 3"]), Duration::from_secs(10), None).unwrap();
    assert_eq!(out.code, Some(3));
    assert_eq!(out.stdout.trim(), "hello");
    assert!(!out.timed_out && !out.canceled);
}

#[test]
fn kills_a_program_that_outlives_its_timeout() {
    let out = exec::run(Command::new("ping").args(["-n", "30", "127.0.0.1"]), Duration::from_millis(300), None).unwrap();
    assert!(out.timed_out);
    assert_eq!(out.code, None);
    assert!(out.elapsed < Duration::from_secs(5));
}

#[test]
fn stops_when_canceled() {
    let cancel = AtomicBool::new(true);
    let out = exec::run(Command::new("ping").args(["-n", "30", "127.0.0.1"]), Duration::from_secs(60), Some(&cancel)).unwrap();
    assert!(out.canceled);
    assert!(out.elapsed < Duration::from_secs(5));
}

#[test]
fn a_failed_check_carries_the_whole_output() {
    let out = exec::run(Command::new("cmd").args(["/c", "echo out& echo err 1>&2& exit 1"]), Duration::from_secs(10), None).unwrap();
    let err = out.check().unwrap_err().to_string();
    assert!(err.contains("exit code 1"), "{err}");
    assert!(err.contains("--- stdout ---\nout") && err.contains("--- stderr ---\nerr"), "{err}");
}

#[test]
fn decode_reads_utf8_utf16_and_oem() {
    assert_eq!(exec::decode("ok ✓".as_bytes()), "ok ✓");
    assert_eq!(exec::decode(&[b'a', 0, b'b', 0, b'c', 0, b'd', 0]), "abcd");
    assert_eq!(exec::decode(&[0xFF, 0xFE, b'x', 0]), "x");
    // "Привет" in cp866, as `sc` prints it on Russian Windows: one char per byte in any single-byte
    // OEM page. With Windows' "UTF-8 for worldwide language support" the OEM page is 65001 and
    // these bytes are not text there.
    let text = exec::decode(&[0x8F, 0xE0, 0xA8, 0xA2, 0xA5, 0xE2]);
    if unsafe { windows::Win32::Globalization::GetOEMCP() } == 65001 {
        assert!(!text.is_empty());
    } else {
        assert_eq!(text.chars().count(), 6);
    }
}

#[test]
fn stamp_has_the_shape_of_test_zapret_ps1_results() {
    let stamp = exec::LocalTime::now().stamp();
    assert_eq!(stamp.len(), "2026-10-10_05-03-34".len());
    assert_eq!(&stamp[4..5], "-");
    assert_eq!(&stamp[10..11], "_");
    assert_eq!(&stamp[13..14], "-");
}
```

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p exec`
Expected: FAIL при компиляции, `run`, `decode` и `LocalTime` не определены (создайте пустой `crates/exec/src/lib.rs`, чтобы cargo нашёл крейт).

- [ ] **Шаг 4: Реализация**

`crates/exec/src/lib.rs`:

```rust
//! Every external program Zapret Hub starts goes through [`run`]: no console window, a hard
//! timeout, an optional cancel flag, and one journal entry with the whole output (spec 10).

mod time;

pub use time::LocalTime;

use std::io::Read;
use std::os::windows::process::CommandExt as _;
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

/// `CREATE_NO_WINDOW`: console programs (sc, netsh, curl, winws) must not flash a window.
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// What a finished, or killed, program left behind.
#[derive(Debug, Clone)]
pub struct Output {
    /// Program and arguments, as the journal shows them.
    pub command: String,
    /// `None` when the process was killed on timeout or cancel.
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub canceled: bool,
    pub elapsed: Duration,
}

impl Output {
    pub fn success(&self) -> bool {
        self.code == Some(0)
    }

    /// `self` when the exit code is 0, otherwise an error that carries all of it.
    pub fn check(self) -> anyhow::Result<Self> {
        if self.success() {
            Ok(self)
        } else {
            Err(anyhow::anyhow!(self.describe()))
        }
    }

    /// Command, outcome and both streams, never shortened.
    pub fn describe(&self) -> String {
        let outcome = if self.canceled {
            "canceled".to_owned()
        } else if self.timed_out {
            format!("killed after {} ms (timeout)", self.elapsed.as_millis())
        } else {
            match self.code {
                Some(code) => format!("exit code {code}"),
                None => "no exit code".to_owned(),
            }
        };
        format!(
            "{} -> {outcome}\n--- stdout ---\n{}\n--- stderr ---\n{}",
            self.command, self.stdout, self.stderr
        )
    }
}

/// Runs `cmd` to the end, or kills it after `timeout` or once `cancel` turns true.
pub fn run(cmd: &mut Command, timeout: Duration, cancel: Option<&AtomicBool>) -> std::io::Result<Output> {
    let command = command_line(cmd);
    let started = Instant::now();
    let spawned = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
    let mut child = match spawned {
        Ok(child) => child,
        Err(err) => {
            log::error!(target: "exec", "{command} -> failed to start: {err}");
            return Err(err);
        }
    };
    let stdout = drain(child.stdout.take());
    let stderr = drain(child.stderr.take());
    let (mut timed_out, mut canceled) = (false, false);
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break Some(status);
        }
        canceled = cancel.is_some_and(|flag| flag.load(Ordering::Relaxed));
        timed_out = started.elapsed() >= timeout;
        if canceled || timed_out {
            let _ = child.kill();
            let _ = child.wait();
            break None;
        }
        std::thread::sleep(Duration::from_millis(20));
    };
    let output = Output {
        command,
        code: status.and_then(|status| status.code()),
        stdout: decode(&stdout.join().unwrap_or_default()),
        stderr: decode(&stderr.join().unwrap_or_default()),
        timed_out,
        canceled,
        elapsed: started.elapsed(),
    };
    log::info!(target: "exec", "{}", output.describe());
    Ok(output)
}

fn command_line(cmd: &Command) -> String {
    let mut line = cmd.get_program().to_string_lossy().into_owned();
    for arg in cmd.get_args() {
        line.push(' ');
        line.push_str(&arg.to_string_lossy());
    }
    line
}

fn drain<R: Read + Send + 'static>(pipe: Option<R>) -> JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut bytes = Vec::new();
        if let Some(mut pipe) = pipe {
            let _ = pipe.read_to_end(&mut bytes);
        }
        bytes
    })
}

/// Console programs write UTF-8, UTF-16 (some system tools when redirected) or the OEM code
/// page (cp866 on Russian Windows); this reads all three.
pub fn decode(bytes: &[u8]) -> String {
    if let Some(rest) = bytes.strip_prefix(&[0xFF, 0xFE]) {
        return utf16le(rest);
    }
    let odd_zeros = bytes.iter().skip(1).step_by(2).filter(|b| **b == 0).count();
    if bytes.len() >= 4 && bytes.len().is_multiple_of(2) && odd_zeros * 4 > bytes.len() {
        return utf16le(bytes);
    }
    match std::str::from_utf8(bytes) {
        Ok(text) => text.to_owned(),
        Err(_) => oem(bytes),
    }
}

fn utf16le(bytes: &[u8]) -> String {
    let units: Vec<u16> = bytes.as_chunks::<2>().0.iter().map(|pair| u16::from_le_bytes(*pair)).collect();
    String::from_utf16_lossy(&units)
}

fn oem(bytes: &[u8]) -> String {
    use windows::Win32::Globalization::{CP_OEMCP, MULTI_BYTE_TO_WIDE_CHAR_FLAGS, MultiByteToWideChar};
    let flags = MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0);
    // SAFETY: valid slices; the first call only measures.
    let len = unsafe { MultiByteToWideChar(CP_OEMCP, flags, bytes, None) };
    if len <= 0 {
        return String::from_utf8_lossy(bytes).into_owned();
    }
    let mut wide = vec![0u16; len as usize];
    // SAFETY: `wide` is exactly as long as the first call asked for.
    unsafe { MultiByteToWideChar(CP_OEMCP, flags, bytes, Some(&mut wide)) };
    String::from_utf16_lossy(&wide)
}
```

`crates/exec/src/time.rs`:

```rust
/// Local wall-clock time: log file names, test run ids.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LocalTime {
    pub year: u16,
    pub month: u16,
    pub day: u16,
    pub hour: u16,
    pub minute: u16,
    pub second: u16,
}

impl LocalTime {
    pub fn now() -> Self {
        // SAFETY: GetLocalTime has no preconditions.
        let t = unsafe { windows::Win32::System::SystemInformation::GetLocalTime() };
        Self { year: t.wYear, month: t.wMonth, day: t.wDay, hour: t.wHour, minute: t.wMinute, second: t.wSecond }
    }

    /// `2026-10-10`
    pub fn date(&self) -> String {
        format!("{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }

    /// `2026-10-10_05-03-34`: how `test zapret.ps1` names its results.
    pub fn stamp(&self) -> String {
        format!("{}_{:02}-{:02}-{:02}", self.date(), self.hour, self.minute, self.second)
    }

    /// `05:03:34`
    pub fn clock(&self) -> String {
        format!("{:02}:{:02}:{:02}", self.hour, self.minute, self.second)
    }
}
```

- [ ] **Шаг 5: Тесты проходят**

Run: `cargo test -p exec`
Expected: PASS, 6 тестов.

- [ ] **Шаг 6: Коммит**

```bash
git add Cargo.toml rust-toolchain.toml .gitignore crates/exec Cargo.lock
git commit -m "feat(exec): run external programs with timeout, cancel and journal"
```

---

### Задача 2: Крейт `probe` — curl, ICMP, SOCKS5

**Files:**
- Create: `crates/probe/Cargo.toml`, `crates/probe/src/lib.rs`, `crates/probe/src/curl.rs`, `crates/probe/src/ping.rs`, `crates/probe/src/socks.rs`
- Test: `crates/probe/tests/curl.rs`, `crates/probe/tests/socks.rs`, `crates/probe/tests/ping.rs`

**Interfaces:**
- Consumes: `exec::run`, `exec::Output`
- Produces:
  - `probe::curl::CheckKind { Http, Tls12, Tls13 }`, `CheckKind::ALL`, `.label() -> "HTTP" | "TLS1.2" | "TLS1.3"`
  - `probe::curl::CheckStatus { Ok, Ssl, Unsup, Error }`
  - `probe::curl::CheckResult { kind, status, http_code: String, time_ms: Option<u32>, stderr: String }`
  - `probe::curl::check(url: &str, kind: CheckKind, cancel: Option<&AtomicBool>) -> CheckResult`
  - `probe::curl::classify(code: Option<i32>, stderr: &str) -> CheckStatus`
  - `probe::curl::parse_code_and_time(stdout: &str) -> (String, Option<u32>)`
  - `probe::curl::DpiStatus { Ok, Fail, Unsupported, LikelyBlocked }`, `DpiResult { kind, status, code: String, up_bytes: u32, down_bytes: u32, time_s: f64 }`
  - `probe::curl::dpi_check(host: &str, kind, payload: &Path, cancel) -> DpiResult`, `dpi_classify(kind, exit: Option<i32>, text: &str, timeout_s: u32) -> DpiResult`, `write_payload(path: &Path) -> io::Result<()>`, `DPI_TIMEOUT_S`
  - `probe::curl::available() -> bool`, `fetch_text(url, socks5: Option<&str>) -> anyhow::Result<String>`, `download(url, dest: &Path, socks5: Option<&str>) -> anyhow::Result<()>`
  - `probe::ping::ping(host: &str, timeout_ms: u32) -> Option<u32>`
  - `probe::socks::handshake(addr: &str, timeout: Duration) -> bool`, `ProxyStatus { ok, latency_ms: Option<u32>, error: Option<String> }`, `check(addr) -> ProxyStatus`, `KNOWN_PORTS`, `nekobox_port(exe_dir: &Path) -> Option<u16>`, `listening(ports: &[u16]) -> Vec<String>`

- [ ] **Шаг 1: Манифест крейта**

`crates/probe/Cargo.toml`:

```toml
[package]
name = "probe"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "Network checks for Zapret Hub: curl.exe requests, ICMP ping, SOCKS5."

[dependencies]
exec.workspace = true
anyhow.workspace = true
log.workspace = true
serde.workspace = true
serde_json.workspace = true
specta.workspace = true
windows = { workspace = true, features = ["Win32_Foundation", "Win32_NetworkManagement_IpHelper"] }

[dev-dependencies]
tempfile.workspace = true
```

Список `members = ["crates/*"]` подхватит крейт сам.

- [ ] **Шаг 2: Падающие тесты**

`crates/probe/tests/curl.rs`:

```rust
use probe::curl::{CheckKind, CheckStatus, DpiStatus, classify, dpi_classify, parse_code_and_time};

#[test]
fn classifies_like_test_zapret_ps1() {
    assert_eq!(classify(Some(0), ""), CheckStatus::Ok);
    assert_eq!(classify(Some(28), "curl: (28) Connection timed out after 2001 milliseconds"), CheckStatus::Error);
    assert_eq!(classify(Some(6), "curl: (6) Could not resolve host: discord.com"), CheckStatus::Ssl);
    assert_eq!(classify(Some(60), "curl: (60) SSL certificate problem: self-signed certificate"), CheckStatus::Ssl);
    assert_eq!(classify(Some(35), "curl: (35) Recv failure: Connection was reset"), CheckStatus::Unsup);
    assert_eq!(classify(Some(35), ""), CheckStatus::Unsup);
    assert_eq!(classify(Some(4), "curl: option --tlsv1.3: the installed libcurl version does not support this"), CheckStatus::Unsup);
    assert_eq!(classify(None, ""), CheckStatus::Error);
}

#[test]
fn reads_code_and_time_from_w() {
    assert_eq!(parse_code_and_time("200 0.153000"), ("200".to_owned(), Some(153)));
    assert_eq!(parse_code_and_time("000 2.004"), ("000".to_owned(), Some(2004)));
    assert_eq!(parse_code_and_time(""), ("000".to_owned(), None));
}

#[test]
fn dpi_verdicts_follow_invoke_dpi_suite() {
    let blocked = dpi_classify(CheckKind::Tls12, Some(28), "000 65536 0 5.004", 5);
    assert_eq!(blocked.status, DpiStatus::LikelyBlocked);
    assert_eq!((blocked.up_bytes, blocked.down_bytes), (65536, 0));

    assert_eq!(dpi_classify(CheckKind::Http, Some(0), "200 65536 65536 0.512", 5).status, DpiStatus::Ok);
    assert_eq!(dpi_classify(CheckKind::Http, Some(28), "000 0 0 3.001", 5).status, DpiStatus::Fail);
    let unsup = dpi_classify(CheckKind::Tls13, Some(35), "", 5);
    assert_eq!((unsup.status, unsup.code.as_str()), (DpiStatus::Unsupported, "UNSUP"));
    let err = dpi_classify(CheckKind::Http, Some(7), "garbage", 5);
    assert_eq!((err.status, err.code.as_str()), (DpiStatus::Fail, "ERR"));
}

#[test]
fn payload_is_64_kib() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("p.bin");
    probe::curl::write_payload(&path).unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), 65536);
}

#[test]
#[ignore = "needs the network"]
fn live_check_reaches_a_site() {
    let result = probe::curl::check("https://www.gstatic.com/generate_204", CheckKind::Tls12, None);
    assert_eq!(result.status, CheckStatus::Ok, "{result:?}");
}
```

`crates/probe/tests/socks.rs`:

```rust
use std::io::{Read, Write};
use std::net::TcpListener;
use std::time::Duration;

#[test]
fn handshake_talks_socks5() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let server = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut hello = [0u8; 3];
        stream.read_exact(&mut hello).unwrap();
        assert_eq!(hello, [5, 1, 0]);
        stream.write_all(&[5, 0]).unwrap();
    });
    assert!(probe::socks::handshake(&addr, Duration::from_secs(2)));
    server.join().unwrap();
}

#[test]
fn a_closed_port_is_not_a_proxy() {
    let port = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    assert!(!probe::socks::handshake(&format!("127.0.0.1:{port}"), Duration::from_millis(300)));
    assert!(!probe::socks::handshake("not an address", Duration::from_millis(300)));
}

#[test]
fn reads_the_nekobox_port() {
    let dir = tempfile::tempdir().unwrap();
    let groups = dir.path().join("config").join("groups");
    std::fs::create_dir_all(&groups).unwrap();
    std::fs::write(groups.join("nekobox.json"), r#"{"inbound_socks_port": 18082, "spmode2": ["system_proxy"]}"#).unwrap();
    assert_eq!(probe::socks::nekobox_port(dir.path()), Some(18082));
    assert_eq!(probe::socks::nekobox_port(&dir.path().join("missing")), None);
}
```

`crates/probe/tests/ping.rs`:

```rust
#[test]
fn pings_loopback() {
    assert!(probe::ping::ping("127.0.0.1", 1000).is_some());
}

#[test]
fn an_unresolvable_name_is_a_timeout() {
    assert_eq!(probe::ping::ping("no-such-host.invalid", 300), None);
}
```

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p probe`
Expected: FAIL при компиляции, модулей `curl`, `ping`, `socks` нет.

- [ ] **Шаг 4: Реализация**

`crates/probe/src/lib.rs`:

```rust
//! Network checks: `curl.exe` with the arguments `test zapret.ps1` uses, ICMP ping, SOCKS5.

pub mod curl;
pub mod ping;
pub mod socks;
```

`crates/probe/src/curl.rs`:

```rust
//! `curl.exe` requests exactly as `test zapret.ps1` makes them, the DPI 16-20 request, downloads.

use std::path::Path;
use std::process::Command;
use std::sync::OnceLock;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckKind {
    Http,
    Tls12,
    Tls13,
}

impl CheckKind {
    pub const ALL: [CheckKind; 3] = [Self::Http, Self::Tls12, Self::Tls13];

    /// The label `test zapret.ps1` prints.
    pub fn label(self) -> &'static str {
        match self {
            Self::Http => "HTTP",
            Self::Tls12 => "TLS1.2",
            Self::Tls13 => "TLS1.3",
        }
    }

    fn args(self) -> &'static [&'static str] {
        match self {
            Self::Http => &["--http1.1"],
            Self::Tls12 => &["--tlsv1.2", "--tls-max", "1.2"],
            Self::Tls13 => &["--tlsv1.3", "--tls-max", "1.3"],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckStatus {
    Ok,
    Ssl,
    Unsup,
    Error,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CheckResult {
    pub kind: CheckKind,
    pub status: CheckStatus,
    /// `%{http_code}`; `000` when no response came.
    pub http_code: String,
    /// `%{time_total}` in ms, when curl printed one.
    pub time_ms: Option<u32>,
    pub stderr: String,
}

/// `-I -s -m 4 --connect-timeout 2 -o NUL -w "%{http_code} %{time_total}" --show-error`, then the
/// kind's arguments and the URL. Only `%{time_total}` is added to the script's request.
pub fn check(url: &str, kind: CheckKind, cancel: Option<&AtomicBool>) -> CheckResult {
    let mut cmd = Command::new("curl.exe");
    cmd.args(["-I", "-s", "-m", "4", "--connect-timeout", "2", "-o", "NUL", "-w", "%{http_code} %{time_total}", "--show-error"])
        .args(kind.args())
        .arg(url);
    match exec::run(&mut cmd, Duration::from_secs(15), cancel) {
        Ok(out) => {
            let (http_code, time_ms) = parse_code_and_time(&out.stdout);
            CheckResult { kind, status: classify(out.code, &out.stderr), http_code, time_ms, stderr: out.stderr }
        }
        Err(err) => CheckResult { kind, status: CheckStatus::Error, http_code: "000".into(), time_ms: None, stderr: err.to_string() },
    }
}

/// The order of `test zapret.ps1`: certificate or DNS trouble, then "unsupported", then the exit code.
pub fn classify(code: Option<i32>, stderr: &str) -> CheckStatus {
    const SSL: [&str; 5] = ["could not resolve host", "certificate", "self-signed", "self signed", "selfsigned"];
    const UNSUP: [&str; 8] = [
        "does not support",
        "not supported",
        "unsupported protocol",
        "unrecognized option",
        "unknown option",
        "unsupported option",
        "unsupported feature",
        "schannel",
    ];
    let err = stderr.to_ascii_lowercase();
    if SSL.iter().any(|p| err.contains(p)) {
        CheckStatus::Ssl
    } else if code == Some(35) || UNSUP.iter().any(|p| err.contains(p)) {
        CheckStatus::Unsup
    } else if code == Some(0) {
        CheckStatus::Ok
    } else {
        CheckStatus::Error
    }
}

/// `"200 0.153000"` → (`"200"`, `Some(153)`).
pub fn parse_code_and_time(stdout: &str) -> (String, Option<u32>) {
    let mut parts = stdout.split_whitespace();
    let code = parts.next().unwrap_or("000").to_owned();
    let time = parts.next().and_then(|t| t.parse::<f64>().ok()).map(|s| (s * 1000.0).round() as u32);
    (code, time)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum DpiStatus {
    Ok,
    Fail,
    Unsupported,
    LikelyBlocked,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DpiResult {
    pub kind: CheckKind,
    pub status: DpiStatus,
    /// HTTP code, `UNSUP`, `ERR` or `NA`.
    pub code: String,
    pub up_bytes: u32,
    pub down_bytes: u32,
    /// `-1` when curl printed no time.
    pub time_s: f64,
}

pub const DPI_TIMEOUT_S: u32 = 5;
const DPI_RANGE_BYTES: usize = 65536;
const DPI_UNSUP: [&str; 9] = [
    "not supported",
    "does not support",
    "unsupported protocol",
    "unrecognized option",
    "unknown option",
    "unsupported option",
    "unsupported feature",
    "schannel",
    "ssl",
];

/// One request of `Invoke-DpiSuite`: POST 64 KiB, ask for 64 KiB back.
pub fn dpi_check(host: &str, kind: CheckKind, payload: &Path, cancel: Option<&AtomicBool>) -> DpiResult {
    let mut cmd = Command::new("curl.exe");
    cmd.args(["--range", "0-65535", "-m", "5", "--connect-timeout", "3"])
        .args(["-w", "%{http_code} %{size_upload} %{size_download} %{time_total}", "-o", "NUL", "-X", "POST"])
        .arg("--data-binary")
        .arg(format!("@{}", payload.display()))
        .arg("-s")
        .args(kind.args())
        .arg(format!("https://{host}"));
    let (code, text) = match exec::run(&mut cmd, Duration::from_secs(20), cancel) {
        Ok(out) => (out.code, format!("{}{}", out.stdout, out.stderr)),
        Err(err) => (None, err.to_string()),
    };
    dpi_classify(kind, code, text.trim(), DPI_TIMEOUT_S)
}

/// The verdict of `Invoke-DpiSuite`: something went up, nothing came down, the time ran out and
/// curl failed — the 16-20 KB freeze.
pub fn dpi_classify(kind: CheckKind, exit: Option<i32>, text: &str, timeout_s: u32) -> DpiResult {
    let mut result = DpiResult { kind, status: DpiStatus::Ok, code: "NA".into(), up_bytes: 0, down_bytes: 0, time_s: -1.0 };
    let lower = text.to_ascii_lowercase();
    if let Some((code, up, down, time)) = parse_w4(text) {
        (result.code, result.up_bytes, result.down_bytes, result.time_s) = (code, up, down, time);
    } else if exit == Some(35) || DPI_UNSUP.iter().any(|p| lower.contains(p)) {
        result.code = "UNSUP".into();
    } else if !text.is_empty() {
        result.code = "ERR".into();
    }
    result.status = if result.code == "UNSUP" {
        DpiStatus::Unsupported
    } else if exit != Some(0) || result.code == "ERR" || result.code == "NA" {
        DpiStatus::Fail
    } else {
        DpiStatus::Ok
    };
    if result.up_bytes > 0 && result.down_bytes == 0 && result.time_s >= f64::from(timeout_s) && exit != Some(0) {
        result.status = DpiStatus::LikelyBlocked;
    }
    result
}

/// `^(\d{3})\s+(\d+)\s+(\d+)\s+([\d.]+)$`
fn parse_w4(text: &str) -> Option<(String, u32, u32, f64)> {
    let parts: Vec<&str> = text.split_whitespace().collect();
    let [code, up, down, time] = parts[..] else { return None };
    if code.len() != 3 || !code.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if !time.bytes().all(|b| b.is_ascii_digit() || b == b'.') {
        return None;
    }
    Some((code.to_owned(), up.parse().ok()?, down.parse().ok()?, time.parse().ok()?))
}

/// 64 KiB of random bytes for the DPI POST body.
pub fn write_payload(path: &Path) -> std::io::Result<()> {
    use std::hash::{BuildHasher, RandomState};
    let state = RandomState::new();
    let bytes: Vec<u8> = (0..DPI_RANGE_BYTES / 8).flat_map(|i| state.hash_one(i).to_le_bytes()).collect();
    std::fs::write(path, bytes)
}

/// Windows 10 1803+ ships `curl.exe`; tests and updates need it.
pub fn available() -> bool {
    exec::run(Command::new("curl.exe").arg("--version"), Duration::from_secs(10), None).is_ok_and(|out| out.success())
}

/// `--ssl-no-revoke` for an old libcurl 7, `--ssl-revoke-best-effort` otherwise, as `:ipset_update` picks.
fn revoke_flag() -> &'static str {
    static FLAG: OnceLock<&'static str> = OnceLock::new();
    FLAG.get_or_init(|| {
        let version = exec::run(Command::new("curl.exe").arg("--version"), Duration::from_secs(10), None)
            .map(|out| out.stdout)
            .unwrap_or_default();
        if version.contains("libcurl/7") { "--ssl-no-revoke" } else { "--ssl-revoke-best-effort" }
    })
}

fn get(url: &str, socks5: Option<&str>) -> Command {
    let mut cmd = Command::new("curl.exe");
    cmd.args(["-L", "-f", "-s", "-S", "--connect-timeout", "10", revoke_flag(), "-H", "Cache-Control: no-cache"]);
    if let Some(proxy) = socks5 {
        cmd.args(["--socks5-hostname", proxy]);
    }
    cmd.arg(url);
    cmd
}

/// Tries directly, then once more through the proxy when there is one.
fn with_fallback<T>(socks5: Option<&str>, attempt: impl Fn(Option<&str>) -> anyhow::Result<T>) -> anyhow::Result<T> {
    attempt(None).or_else(|direct| match socks5 {
        Some(proxy) => attempt(Some(proxy)).map_err(|via| anyhow::anyhow!("direct: {direct:#}\n\nthrough {proxy}: {via:#}")),
        None => Err(direct),
    })
}

/// A small text resource (version file, hosts, DPI suite).
pub fn fetch_text(url: &str, socks5: Option<&str>) -> anyhow::Result<String> {
    with_fallback(socks5, |proxy| {
        let mut cmd = get(url, proxy);
        cmd.args(["-m", "30"]);
        Ok(exec::run(&mut cmd, Duration::from_secs(40), None)?.check()?.stdout)
    })
}

/// A file (ipset list) straight to `dest`.
pub fn download(url: &str, dest: &Path, socks5: Option<&str>) -> anyhow::Result<()> {
    with_fallback(socks5, |proxy| {
        let mut cmd = get(url, proxy);
        cmd.args(["-m", "120", "-o"]).arg(dest);
        exec::run(&mut cmd, Duration::from_secs(130), None)?.check()?;
        Ok(())
    })
}
```

`crates/probe/src/ping.rs`:

```rust
//! ICMP echo, like `System.Net.NetworkInformation.Ping.Send(host, 1000)` in `test zapret.ps1`.

use std::net::{SocketAddr, ToSocketAddrs};

use windows::Win32::NetworkManagement::IpHelper::{ICMP_ECHO_REPLY, IcmpCloseHandle, IcmpCreateFile, IcmpSendEcho};

/// Round-trip time in ms; `None` on timeout, error, or a name that does not resolve.
pub fn ping(host: &str, timeout_ms: u32) -> Option<u32> {
    let ip = (host, 0).to_socket_addrs().ok()?.find_map(|addr| match addr {
        SocketAddr::V4(v4) => Some(*v4.ip()),
        SocketAddr::V6(_) => None,
    })?;
    let data = [0x61u8; 32];
    let mut reply = vec![0u8; size_of::<ICMP_ECHO_REPLY>() + data.len() + 8];
    // SAFETY: the handle is closed below; both buffers outlive the call and their sizes are passed.
    unsafe {
        let handle = IcmpCreateFile().ok()?;
        let count = IcmpSendEcho(
            handle,
            u32::from_ne_bytes(ip.octets()),
            data.as_ptr().cast(),
            data.len() as u16,
            None,
            reply.as_mut_ptr().cast(),
            reply.len() as u32,
            timeout_ms,
        );
        let _ = IcmpCloseHandle(handle);
        if count == 0 {
            return None;
        }
        let echo = std::ptr::read_unaligned(reply.as_ptr().cast::<ICMP_ECHO_REPLY>());
        (echo.Status == 0).then_some(echo.RoundTripTime)
    }
}
```

`crates/probe/src/socks.rs`:

```rust
//! The local SOCKS5 proxy: does it answer, how fast, which ports to suggest.

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::Path;
use std::process::Command;
use std::time::Duration;

/// Greeting `05 01 00` (no auth), answered with `05 00`.
pub fn handshake(addr: &str, timeout: Duration) -> bool {
    let Ok(addr) = addr.parse::<SocketAddr>() else { return false };
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, timeout) else { return false };
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let mut reply = [0u8; 2];
    stream.write_all(&[5, 1, 0]).is_ok() && stream.read_exact(&mut reply).is_ok() && reply == [5, 0]
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ProxyStatus {
    pub ok: bool,
    pub latency_ms: Option<u32>,
    pub error: Option<String>,
}

/// `curl.exe --socks5-hostname <addr> -I https://www.gstatic.com/generate_204` (spec 8).
pub fn check(addr: &str) -> ProxyStatus {
    let mut cmd = Command::new("curl.exe");
    cmd.args(["-I", "-s", "-S", "-m", "8", "-o", "NUL", "-w", "%{http_code} %{time_total}", "--socks5-hostname", addr])
        .arg("https://www.gstatic.com/generate_204");
    match exec::run(&mut cmd, Duration::from_secs(15), None) {
        Ok(out) if out.success() => {
            ProxyStatus { ok: true, latency_ms: crate::curl::parse_code_and_time(&out.stdout).1, error: None }
        }
        Ok(out) => ProxyStatus { ok: false, latency_ms: None, error: Some(out.stderr.trim().to_owned()) },
        Err(err) => ProxyStatus { ok: false, latency_ms: None, error: Some(err.to_string()) },
    }
}

/// Default SOCKS ports of known clients: NekoBox 2080, v2rayN 10808, Clash 7890.
pub const KNOWN_PORTS: [u16; 3] = [2080, 10808, 7890];

/// `inbound_socks_port` from `config\groups\nekobox.json` next to NekoBox's exe.
pub fn nekobox_port(exe_dir: &Path) -> Option<u16> {
    let text = std::fs::read_to_string(exe_dir.join("config").join("groups").join("nekobox.json")).ok()?;
    let json: serde_json::Value = serde_json::from_str(&text).ok()?;
    u16::try_from(json.get("inbound_socks_port")?.as_u64()?).ok()
}

/// `127.0.0.1:<port>` of every port that answers a SOCKS5 greeting, in the given order.
pub fn listening(ports: &[u16]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for port in ports {
        let addr = format!("127.0.0.1:{port}");
        if !out.contains(&addr) && handshake(&addr, Duration::from_millis(300)) {
            out.push(addr);
        }
    }
    out
}
```

- [ ] **Шаг 5: Тесты проходят**

Run: `cargo test -p probe`
Expected: PASS (живой тест `live_check_reaches_a_site` пропущен как `ignored`).

Run: `cargo test -p probe -- --ignored`
Expected: PASS при работающей сети.

- [ ] **Шаг 6: Коммит**

```bash
git add crates/probe Cargo.lock
git commit -m "feat(probe): add curl checks, DPI request, ICMP ping and SOCKS5 probe"
```

---

### Задача 3: `zapret` — папка Flowseal, список стратегий, `targets.txt`

**Files:**
- Create: `crates/zapret/Cargo.toml`, `crates/zapret/src/lib.rs`, `crates/zapret/src/folder.rs`, `crates/zapret/src/strategies.rs`, `crates/zapret/src/targets.rs`
- Test: `crates/zapret/tests/common/mod.rs`, `crates/zapret/tests/folder.rs`

**Interfaces:**
- Consumes: —
- Produces:
  - `zapret::read_text(path: &Path) -> io::Result<String>` (UTF-8 с заменой битых байтов), `zapret::sha256_hex(bytes: &[u8]) -> String`
  - `zapret::folder::Folder::open(root: impl Into<PathBuf>) -> anyhow::Result<Folder>`; `.root() -> &Path`, `.bin()`, `.lists()`, `.utils()`, `.winws() -> PathBuf`, `.root_dp0() -> String` (корень с `\` на конце), `.version() -> Option<String>`
  - `zapret::folder::{missing_parts(&Path) -> Vec<&'static str>, local_version(&str) -> Option<String>, root_from_image_path(&str) -> Option<PathBuf>}`
  - `zapret::strategies::StrategyFile { name: String /* stem */, path: PathBuf }`, `list(&Folder) -> io::Result<Vec<StrategyFile>>`, `find(&Folder, name) -> anyhow::Result<StrategyFile>`, `natural_key(&str) -> String`
  - `zapret::targets::Target { name: String, url: Option<String>, ping: String }`, `load(&Folder) -> Vec<Target>`, `parse(&str) -> Vec<Target>`, `defaults() -> Vec<Target>`, `url_host(&str) -> &str`
  - Тестовый помощник `tests/common/mod.rs`: `fake_zapret() -> tempfile::TempDir` (нужен задачам 4–10)

- [ ] **Шаг 1: Манифест крейта**

`crates/zapret/Cargo.toml` (все зависимости крейта сразу, следующие задачи их используют):

```toml
[package]
name = "zapret"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "Flowseal zapret folder model: strategies, service, flags, lists, updates, system checks."

[dependencies]
exec.workspace = true
probe.workspace = true
anyhow.workspace = true
log.workspace = true
serde.workspace = true
serde_json.workspace = true
sha2.workspace = true
specta.workspace = true
sysinfo.workspace = true
toml.workspace = true
windows = { workspace = true, features = ["Win32_Foundation", "Win32_System_Services"] }
windows-registry.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Шаг 2: Падающие тесты**

`crates/zapret/tests/common/mod.rs`:

```rust
#![allow(dead_code)]

use std::fs;

/// A minimal Flowseal folder: what `Folder::open` checks, plus `utils\`.
pub fn fake_zapret() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    fs::write(root.join("service.bat"), "@echo off\r\nset \"LOCAL_VERSION=1.10.3\"\r\n").unwrap();
    fs::create_dir_all(root.join("bin")).unwrap();
    fs::write(root.join("bin").join("winws.exe"), b"").unwrap();
    fs::write(root.join("bin").join("WinDivert64.sys"), b"").unwrap();
    fs::create_dir_all(root.join("lists")).unwrap();
    fs::create_dir_all(root.join("utils")).unwrap();
    dir
}

pub fn folder(dir: &tempfile::TempDir) -> zapret::folder::Folder {
    zapret::folder::Folder::open(dir.path()).unwrap()
}
```

`crates/zapret/tests/folder.rs`:

```rust
mod common;

use std::path::PathBuf;

use zapret::folder::{Folder, root_from_image_path};

#[test]
fn opens_a_complete_folder_and_reads_its_version() {
    let dir = common::fake_zapret();
    let folder = Folder::open(dir.path()).unwrap();
    assert_eq!(folder.version().as_deref(), Some("1.10.3"));
    assert!(folder.root_dp0().ends_with('\\'));
    assert_eq!(folder.winws(), dir.path().join("bin").join("winws.exe"));
}

#[test]
fn names_everything_that_is_missing() {
    let dir = tempfile::tempdir().unwrap();
    let err = Folder::open(dir.path()).unwrap_err().to_string();
    for part in ["service.bat", "winws.exe", "WinDivert64.sys", "lists"] {
        assert!(err.contains(part), "{err}");
    }
    let gone = dir.path().join("moved-away");
    assert!(Folder::open(&gone).is_err());
}

#[test]
fn root_from_the_service_image_path() {
    assert_eq!(root_from_image_path(r#""E:\Zapret\bin\winws.exe"   --wf-tcp 80"#), Some(PathBuf::from(r"E:\Zapret")));
    assert_eq!(root_from_image_path(r"C:\z\bin\winws.exe --x"), Some(PathBuf::from(r"C:\z")));
}

#[test]
fn strategies_come_in_service_bat_order() {
    let dir = common::fake_zapret();
    for name in [
        "general (ALT10).bat",
        "general (ALT2).bat",
        "general.bat",
        "general (ALT).bat",
        "service.bat",
        "Service backup.bat",
        "general (SIMPLE FAKE).bat",
        "general (SIMPLE FAKE ALT).bat",
        "notes.txt",
    ] {
        std::fs::write(dir.path().join(name), "").unwrap();
    }
    let names: Vec<String> = zapret::strategies::list(&common::folder(&dir)).unwrap().into_iter().map(|s| s.name).collect();
    assert_eq!(
        names,
        ["general (ALT)", "general (ALT2)", "general (ALT10)", "general (SIMPLE FAKE ALT)", "general (SIMPLE FAKE)", "general"]
    );
    assert!(zapret::strategies::find(&common::folder(&dir), "GENERAL (alt2)").is_ok());
    assert!(zapret::strategies::find(&common::folder(&dir), "missing").is_err());
}

#[test]
fn natural_key_pads_numbers_like_service_bat() {
    assert_eq!(zapret::strategies::natural_key("general (ALT10).bat"), "general (alt00000010).bat");
    assert_eq!(zapret::strategies::natural_key("a123456789"), "a123456789");
}

#[test]
fn parses_targets_like_the_ps1() {
    let text = concat!(
        "# comment = \"x\"\n",
        "DiscordMain = \"https://discord.com\"\n",
        "Bad Key = \"https://x\"\n",
        "Ping1 = \"PING: 1.1.1.1\"\n",
        "DiscordMain = \"https://discord.com/app\"\n",
        "Empty = \"\"\n",
    );
    let targets = zapret::targets::parse(text);
    assert_eq!(targets.len(), 2);
    assert_eq!(targets[0].name, "DiscordMain");
    assert_eq!(targets[0].url.as_deref(), Some("https://discord.com/app"));
    assert_eq!(targets[0].ping, "discord.com");
    assert_eq!(targets[1].url, None);
    assert_eq!(targets[1].ping, "1.1.1.1");
}

#[test]
fn without_targets_txt_the_ps1_defaults_are_used() {
    let dir = common::fake_zapret();
    let targets = zapret::targets::load(&common::folder(&dir));
    assert_eq!(targets.len(), 17);
    assert_eq!(targets.iter().filter(|t| t.url.is_some()).count(), 12);
    std::fs::write(dir.path().join("utils").join("targets.txt"), "A = \"https://a.example\"\r\n").unwrap();
    assert_eq!(zapret::targets::load(&common::folder(&dir)).len(), 1);
}
```

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test folder`
Expected: FAIL при компиляции, модулей нет.

- [ ] **Шаг 4: Реализация**

`crates/zapret/src/lib.rs`:

```rust
//! The Flowseal `zapret-discord-youtube` folder as `service.bat` sees it.

pub mod folder;
pub mod strategies;
pub mod targets;

use std::path::Path;

/// A text file Flowseal ships (ASCII or UTF-8); broken bytes become U+FFFD.
pub fn read_text(path: &Path) -> std::io::Result<String> {
    Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned())
}

/// Lower-case hex SHA-256.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest as _, Sha256};
    Sha256::digest(bytes).iter().map(|b| format!("{b:02x}")).collect()
}
```

`crates/zapret/src/folder.rs`:

```rust
//! Where the Flowseal folder is, whether it is complete, which version it is.

use std::path::{Path, PathBuf};

/// A Flowseal folder that has what `service.bat` needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Folder {
    root: PathBuf,
}

impl Folder {
    /// Fails with the list of what is missing.
    pub fn open(root: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let root = root.into();
        let missing = missing_parts(&root);
        anyhow::ensure!(
            missing.is_empty(),
            "{} is not a zapret folder: missing {}",
            root.display(),
            missing.join(", ")
        );
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn bin(&self) -> PathBuf {
        self.root.join("bin")
    }

    pub fn lists(&self) -> PathBuf {
        self.root.join("lists")
    }

    pub fn utils(&self) -> PathBuf {
        self.root.join("utils")
    }

    pub fn winws(&self) -> PathBuf {
        self.bin().join("winws.exe")
    }

    /// `%~dp0` of `service.bat`: the root with a trailing backslash.
    pub fn root_dp0(&self) -> String {
        let mut root = self.root.to_string_lossy().into_owned();
        if !root.ends_with('\\') {
            root.push('\\');
        }
        root
    }

    /// `LOCAL_VERSION` from `service.bat`.
    pub fn version(&self) -> Option<String> {
        local_version(&crate::read_text(&self.root.join("service.bat")).ok()?)
    }
}

/// What `Folder::open` checks: `service.bat`, `bin\winws.exe`, `bin\WinDivert64.sys`, `lists\`.
pub fn missing_parts(root: &Path) -> Vec<&'static str> {
    let mut missing = Vec::new();
    if !root.join("service.bat").is_file() {
        missing.push("service.bat");
    }
    if !root.join("bin").join("winws.exe").is_file() {
        missing.push("bin\\winws.exe");
    }
    if !root.join("bin").join("WinDivert64.sys").is_file() {
        missing.push("bin\\WinDivert64.sys");
    }
    if !root.join("lists").is_dir() {
        missing.push("lists\\");
    }
    missing
}

/// `set "LOCAL_VERSION=1.10.3"` → `1.10.3`.
pub fn local_version(service_bat: &str) -> Option<String> {
    let line = service_bat.lines().find(|line| line.contains("LOCAL_VERSION="))?;
    let value = line.split_once("LOCAL_VERSION=")?.1.trim().trim_end_matches('"').trim();
    (!value.is_empty()).then(|| value.to_owned())
}

/// `"E:\Zapret\bin\winws.exe"   --wf-tcp ...` → `E:\Zapret`, the folder two levels above winws.
pub fn root_from_image_path(image_path: &str) -> Option<PathBuf> {
    let image_path = image_path.trim();
    let exe = match image_path.strip_prefix('"') {
        Some(rest) => rest.split('"').next()?,
        None => image_path.split(' ').next()?,
    };
    Some(Path::new(exe).parent()?.parent()?.to_path_buf())
}
```

`crates/zapret/src/strategies.rs`:

```rust
//! Strategy `.bat` files in the folder root, in the order of `service.bat`'s menu.

use std::path::PathBuf;

use crate::folder::Folder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StrategyFile {
    /// File stem: the name `service.bat` writes to the registry, `general (ALT11)`.
    pub name: String,
    pub path: PathBuf,
}

/// Every `*.bat` except `service*`, sorted as `service.bat` sorts them.
pub fn list(folder: &Folder) -> std::io::Result<Vec<StrategyFile>> {
    let mut files: Vec<(String, StrategyFile)> = Vec::new();
    for entry in std::fs::read_dir(folder.root())? {
        let path = entry?.path();
        let Some(file_name) = path.file_name().and_then(|n| n.to_str()) else { continue };
        let lower = file_name.to_lowercase();
        if !lower.ends_with(".bat") || lower.starts_with("service") || !path.is_file() {
            continue;
        }
        let Some(name) = path.file_stem().and_then(|s| s.to_str()).map(str::to_owned) else { continue };
        files.push((natural_key(file_name), StrategyFile { name, path }));
    }
    files.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(files.into_iter().map(|(_, file)| file).collect())
}

pub fn find(folder: &Folder, name: &str) -> anyhow::Result<StrategyFile> {
    list(folder)?
        .into_iter()
        .find(|file| file.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| anyhow::anyhow!("strategy {name} not found in {}", folder.root().display()))
}

/// `[Regex]::Replace($_.Name, '(\d+)', { $args[0].Value.PadLeft(8, '0') })`, lower-cased.
// ponytail: ordinal compare of the padded name; PowerShell compares by culture, which orders
// Flowseal's ASCII names the same way.
pub fn natural_key(file_name: &str) -> String {
    let mut key = String::with_capacity(file_name.len() + 16);
    let mut digits = String::new();
    for c in file_name.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
            continue;
        }
        flush_digits(&mut key, &mut digits);
        key.extend(c.to_lowercase());
    }
    flush_digits(&mut key, &mut digits);
    key
}

fn flush_digits(key: &mut String, digits: &mut String) {
    if digits.is_empty() {
        return;
    }
    for _ in digits.len()..8 {
        key.push('0');
    }
    key.push_str(digits);
    digits.clear();
}
```

`crates/zapret/src/targets.rs`:

```rust
//! `utils\targets.txt`, read the way `test zapret.ps1` reads it.

use crate::folder::Folder;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    pub name: String,
    /// `https://...` for the HTTP/TLS checks; `None` for a ping-only target.
    pub url: Option<String>,
    /// Host or IP the ping goes to.
    pub ping: String,
}

/// `targets.txt`, or the script's built-in list when the file is missing or empty.
pub fn load(folder: &Folder) -> Vec<Target> {
    let parsed = crate::read_text(&folder.utils().join("targets.txt")).map(|text| parse(&text)).unwrap_or_default();
    if parsed.is_empty() { defaults() } else { parsed }
}

/// Lines `Key = "value"`; a repeated key keeps its first place and takes the last value
/// (`OrderedDictionary` + `Add-OrSet`).
pub fn parse(text: &str) -> Vec<Target> {
    let mut targets: Vec<Target> = Vec::new();
    for line in text.lines() {
        let Some((key, value)) = parse_line(line) else { continue };
        let target = convert(key, value);
        match targets.iter_mut().find(|t| t.name == key) {
            Some(existing) => *existing = target,
            None => targets.push(target),
        }
    }
    targets
}

/// `^\s*(\w+)\s*=\s*"(.+)"\s*$`
fn parse_line(line: &str) -> Option<(&str, &str)> {
    let (key, rest) = line.split_once('=')?;
    let key = key.trim();
    if key.is_empty() || !key.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return None;
    }
    let value = rest.trim().strip_prefix('"')?.strip_suffix('"')?;
    (!value.is_empty()).then_some((key, value))
}

/// `Convert-Target`: `PING:1.2.3.4` is ping-only; a URL pings its host.
fn convert(name: &str, value: &str) -> Target {
    if value.get(..5).is_some_and(|p| p.eq_ignore_ascii_case("PING:")) {
        return Target { name: name.to_owned(), url: None, ping: value[5..].trim_start().to_owned() };
    }
    Target { name: name.to_owned(), url: Some(value.to_owned()), ping: url_host(value).to_owned() }
}

/// `https://host/path` → `host` (`-replace "^https?://", "" -replace "/.*$", ""`).
pub fn url_host(url: &str) -> &str {
    let rest = ["https://", "http://"]
        .iter()
        .find_map(|scheme| url.get(..scheme.len()).filter(|p| p.eq_ignore_ascii_case(scheme)).map(|_| &url[scheme.len()..]))
        .unwrap_or(url);
    rest.split('/').next().unwrap_or(rest)
}

/// The list `test zapret.ps1` falls back to.
pub fn defaults() -> Vec<Target> {
    [
        ("Discord Main", "https://discord.com"),
        ("Discord Gateway", "https://gateway.discord.gg"),
        ("Discord CDN", "https://cdn.discordapp.com"),
        ("Discord Updates", "https://updates.discord.com"),
        ("YouTube Web", "https://www.youtube.com"),
        ("YouTube Short", "https://youtu.be"),
        ("YouTube Image", "https://i.ytimg.com"),
        ("YouTube Video Redirect", "https://redirector.googlevideo.com"),
        ("Google Main", "https://www.google.com"),
        ("Google Gstatic", "https://www.gstatic.com"),
        ("Cloudflare Web", "https://www.cloudflare.com"),
        ("Cloudflare CDN", "https://cdnjs.cloudflare.com"),
        ("Cloudflare DNS 1.1.1.1", "PING:1.1.1.1"),
        ("Cloudflare DNS 1.0.0.1", "PING:1.0.0.1"),
        ("Google DNS 8.8.8.8", "PING:8.8.8.8"),
        ("Google DNS 8.8.4.4", "PING:8.8.4.4"),
        ("Quad9 DNS 9.9.9.9", "PING:9.9.9.9"),
    ]
    .into_iter()
    .map(|(name, value)| convert(name, value))
    .collect()
}
```

- [ ] **Шаг 5: Тесты проходят**

Run: `cargo test -p zapret --test folder`
Expected: PASS, 7 тестов.

- [ ] **Шаг 6: Коммит**

```bash
git add crates/zapret Cargo.lock
git commit -m "feat(zapret): read the Flowseal folder, its strategies and targets"
```

---

### Задача 4: `zapret` — порт разбора `.bat` и эталоны

**Files:**
- Create: `crates/zapret/src/batparse.rs`, `tools/gen-bat-fixtures.cmd`
- Create (генерируются): `crates/zapret/tests/fixtures/*.bat`, `*.args`, `ROOT.txt`
- Modify: `crates/zapret/src/lib.rs` (добавить `pub mod batparse;`)
- Test: `crates/zapret/tests/batparse.rs`, `crates/zapret/tests/golden.rs`

**Interfaces:**
- Consumes: `zapret::folder::Folder`, `zapret::read_text`, `zapret::sha256_hex`
- Produces:
  - `zapret::batparse::GameFilterVars { all: String, tcp: String, udp: String }`, `GameFilterVars::disabled()` (везде `12`)
  - `zapret::batparse::ParsedArgs { final_args: String }`: `.command_tail() -> String` (`\"` → `"`), `.bin_path(winws: &Path) -> String`, `.sha256() -> String`
  - `zapret::batparse::parse(bat: &str, root_dp0: &str, vars: &GameFilterVars) -> anyhow::Result<ParsedArgs>`
  - `pub(crate) fn find_ci(hay: &str, needle: &str) -> Option<usize>` (для `custom.rs`)
  - `parse_file(folder, path)` добавит задача 5: ему нужен `game_filter`.

- [ ] **Шаг 1: Падающие тесты разбора**

`crates/zapret/tests/batparse.rs`:

```rust
use zapret::batparse::{GameFilterVars, parse};

fn all_ports() -> GameFilterVars {
    GameFilterVars { all: "1024-65535".into(), tcp: "1024-65535".into(), udp: "12".into() }
}

#[test]
fn parses_a_strategy_like_service_bat() {
    let bat = concat!(
        "@echo off\r\n",
        "set \"BIN=%~dp0bin\\\"\r\n",
        "start \"zapret: %~n0\" /min \"%BIN%winws.exe\" --wf-tcp=80,443,%GameFilterTCP% --wf-udp=443 ^\r\n",
        "--filter-udp=443 --hostlist=\"%LISTS%list-general.txt\" --dpi-desync=fake --dpi-desync-fake-tls-mod=rnd,sni=www.google.com --new ^\r\n",
        "--filter-tcp=80 --dpi-desync-fake-tls=\"@bin\\x.bin\" --hostlist=\"lists\\y.txt\"\r\n",
    );
    let parsed = parse(bat, "E:\\Zapret\\", &all_ports()).unwrap();
    assert_eq!(
        parsed.final_args,
        concat!(
            "  --wf-tcp 80,443,1024-65535 --wf-udp 443",
            "  --filter-udp 443 --hostlist \\\"E:\\Zapret\\lists\\list-general.txt\\\" --dpi-desync fake",
            " --dpi-desync-fake-tls-mod rnd,sni=www.google.com --new",
            "  --filter-tcp 80 --dpi-desync-fake-tls \\\"@E:\\Zapret\\bin\\x.bin\\\" --hostlist \\\"E:\\Zapret\\lists\\y.txt\\\"",
        )
    );
    assert_eq!(parsed.command_tail(), parsed.final_args.replace("\\\"", "\""));
    assert_eq!(
        parsed.bin_path(std::path::Path::new(r"E:\Zapret\bin\winws.exe")),
        format!("\"E:\\Zapret\\bin\\winws.exe\" {}", parsed.command_tail())
    );
    assert_eq!(parsed.sha256().len(), 64);
}

#[test]
fn keeps_a_cyrillic_root_intact() {
    let bat = "start \"z\" /min \"%BIN%winws.exe\" --hostlist=\"%LISTS%a.txt\"\r\n";
    let parsed = parse(bat, "C:\\Запрет\\", &GameFilterVars::disabled()).unwrap();
    assert_eq!(parsed.final_args, "  --hostlist \\\"C:\\Запрет\\lists\\a.txt\\\"");
}

#[test]
fn disabled_game_filter_puts_port_12() {
    let bat = "start \"z\" /min \"%BIN%winws.exe\" --wf-udp=443,%GameFilterUDP% ^\r\n--filter-tcp=%GameFilter%\r\n";
    let parsed = parse(bat, "C:\\z\\", &GameFilterVars::disabled()).unwrap();
    assert_eq!(parsed.final_args, "  --wf-udp 443,12  --filter-tcp 12");
}

#[test]
fn a_caret_before_an_exclamation_mark_is_an_escape() {
    // general (FAKE TLS AUTO).bat: service.bat passes `!` to winws, not `^!`.
    let bat = "start \"z\" /min \"%BIN%winws.exe\" --filter-tcp=443 --dpi-desync-fake-tls=^! ^\r\n--new\r\n";
    let parsed = parse(bat, "C:\\z\\", &GameFilterVars::disabled()).unwrap();
    assert!(parsed.final_args.contains("--dpi-desync-fake-tls !"), "{}", parsed.final_args);
    assert!(!parsed.final_args.contains('^'), "{}", parsed.final_args);
}

#[test]
fn a_file_without_winws_is_not_a_strategy() {
    assert!(parse("@echo off\r\necho hi\r\n", "C:\\z\\", &GameFilterVars::disabled()).is_err());
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test batparse`
Expected: FAIL при компиляции, модуля `batparse` нет.

- [ ] **Шаг 3: Реализация**

В `crates/zapret/src/lib.rs` после `pub mod folder;` добавьте `pub mod batparse;`.

`crates/zapret/src/batparse.rs`:

```rust
//! Port of the argument parser in `service.bat :service_install`: a strategy `.bat` becomes the
//! exact `winws` arguments the service is created with. Golden files (tests/golden.rs) hold it to
//! byte equality with the batch original.

use std::path::Path;

/// `%GameFilter%`, `%GameFilterTCP%`, `%GameFilterUDP%`, as `:game_switch_status` sets them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameFilterVars {
    pub all: String,
    pub tcp: String,
    pub udp: String,
}

impl GameFilterVars {
    /// Game filter off: the placeholder port `12` everywhere.
    pub fn disabled() -> Self {
        Self { all: "12".into(), tcp: "12".into(), udp: "12".into() }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedArgs {
    /// What `service.bat` prints after `Final args: `, quotes still escaped as `\"`.
    pub final_args: String,
}

impl ParsedArgs {
    /// The arguments as the service process gets them: `sc` turns every `\"` into `"`.
    pub fn command_tail(&self) -> String {
        self.final_args.replace("\\\"", "\"")
    }

    /// The service `binPath`: `"<bin>\winws.exe"` and the tail, spacing kept.
    pub fn bin_path(&self, winws: &Path) -> String {
        format!("\"{}\" {}", winws.display(), self.command_tail())
    }

    /// Identifies these exact arguments in test results.
    pub fn sha256(&self) -> String {
        crate::sha256_hex(self.final_args.as_bytes())
    }
}

/// Parses `bat` the way `service.bat` does for a folder whose `%~dp0` is `root_dp0`.
pub fn parse(bat: &str, root_dp0: &str, vars: &GameFilterVars) -> anyhow::Result<ParsedArgs> {
    let bin = format!("{root_dp0}bin\\");
    let lists = format!("{root_dp0}lists\\");
    let mut args = String::new();
    let mut capture = false;
    // 2 = an option was just written, 1 = its values follow (joined by `,`),
    // 3 = the next value joins with `=` (after `sni`, `host`, `altorder`), 0 = plain.
    let mut merge = 0u8;
    for raw in bat.lines() {
        // `for /f "tokens=*"`: leading blanks go; empty lines and `;` lines are skipped.
        let line = raw.trim_start_matches([' ', '\t']);
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let line = if line.contains('!') { unescape_carets(line) } else { line.to_owned() };
        let mut line = expand(&line, root_dp0, &bin, &lists, vars);
        if !capture && line.to_ascii_lowercase().contains("winws.exe") {
            capture = true;
        }
        if !capture {
            continue;
        }
        if args.is_empty() {
            // `set "line=!line:*winws.exe"=!"`
            if let Some(end) = find_ci(&line, "winws.exe\"") {
                line = line[end..].to_owned();
            }
        }
        let mut temp = String::new();
        for token in for_tokens(&line) {
            if token == "^" || token == "^^" {
                continue;
            }
            let mut arg = token;
            if arg.starts_with("--") && merge != 0 {
                merge = 0;
            }
            if arg.starts_with('"') {
                arg = quote_arg(&arg, root_dp0);
            }
            match merge {
                1 => {
                    temp.push(',');
                    temp.push_str(&arg);
                }
                3 => {
                    temp.push('=');
                    temp.push_str(&arg);
                    merge = 1;
                }
                _ => {
                    temp.push(' ');
                    temp.push_str(&arg);
                }
            }
            if arg.starts_with("--") {
                merge = 2;
            } else if merge >= 1 {
                if merge == 2 {
                    merge = 1;
                }
                if ["sni", "host", "altorder"].iter().any(|v| v.eq_ignore_ascii_case(&arg)) {
                    merge = 3;
                }
            }
        }
        if !temp.is_empty() {
            args.push(' ');
            args.push_str(&temp);
        }
    }
    anyhow::ensure!(capture, "no winws.exe command in the strategy file");
    Ok(ParsedArgs { final_args: args })
}

/// `set "line=%%a"` under delayed expansion: in a line that has a `!`, cmd reads every `^` as an
/// escape and keeps the character after it (`--dpi-desync-fake-tls=^!` → `=!`); a trailing `^` goes.
fn unescape_carets(line: &str) -> String {
    let mut out = String::with_capacity(line.len());
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        if c == '^' {
            out.extend(chars.next());
        } else {
            out.push(c);
        }
    }
    out
}

/// Byte offset right after the first case-insensitive occurrence of an ASCII `needle`.
pub(crate) fn find_ci(hay: &str, needle: &str) -> Option<usize> {
    hay.to_ascii_lowercase().find(&needle.to_ascii_lowercase()).map(|i| i + needle.len())
}

/// The `%` pass of `call set "line=!line!"`, for the variables strategies use.
fn expand(line: &str, root_dp0: &str, bin: &str, lists: &str, vars: &GameFilterVars) -> String {
    let mut out = replace_ci(line, "%~dp0", root_dp0);
    for (name, value) in [
        ("%BIN%", bin),
        ("%LISTS%", lists),
        ("%GameFilterTCP%", vars.tcp.as_str()),
        ("%GameFilterUDP%", vars.udp.as_str()),
        ("%GameFilter%", vars.all.as_str()),
    ] {
        out = replace_ci(&out, name, value);
    }
    out
}

fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
    let lower = hay.to_ascii_lowercase();
    let needle_lower = needle.to_ascii_lowercase();
    let mut out = String::with_capacity(hay.len());
    let mut last = 0;
    for (start, _) in lower.match_indices(&needle_lower) {
        out.push_str(&hay[last..start]);
        out.push_str(with);
        last = start + needle.len();
    }
    out.push_str(&hay[last..]);
    out
}

/// Tokens of `for %%i in (!line!)`: split on space, tab, `,`, `;`, `=` outside quotes, quotes kept.
fn for_tokens(line: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in line.chars() {
        if c == '"' {
            quoted = !quoted;
            current.push(c);
        } else if !quoted && matches!(c, ' ' | '\t' | ',' | ';' | '=') {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// A quoted token: `"X"` with a `:` stays absolute, `"@X"` and `"X"` become root-relative;
/// the quotes come back escaped as `\"` (`set "arg=\!QUOTE!...\!QUOTE!"`).
fn quote_arg(arg: &str, root_dp0: &str) -> String {
    // `!arg:~1,-1!`: drop the first and the last character.
    let mut chars = arg.chars();
    chars.next();
    chars.next_back();
    let inner = chars.as_str();
    if inner.contains(':') {
        format!("\\\"{inner}\\\"")
    } else if let Some(rest) = inner.strip_prefix('@') {
        format!("\\\"@{root_dp0}{rest}\\\"")
    } else {
        format!("\\\"{root_dp0}{inner}\\\"")
    }
}
```

- [ ] **Шаг 4: Тесты разбора проходят**

Run: `cargo test -p zapret --test batparse`
Expected: PASS, 5 тестов.

- [ ] **Шаг 5: Генератор эталонов**

`tools/gen-bat-fixtures.cmd` (блок `:parse` — дословная копия `service.bat :service_install` между `set "args_with_value=..."` и `echo Final args`, где `%~dp0` заменено на `%ROOT%`):

```bat
@echo off
:: Regenerates crates\zapret\tests\fixtures from a Flowseal folder: a copy of every general*.bat
:: strategy and the "Final args" line that service.bat :service_install builds for it. The :parse
:: block is copied from service.bat, with %~dp0 replaced by %ROOT%.
:: Usage: tools\gen-bat-fixtures.cmd E:\Zapret
setlocal EnableDelayedExpansion
chcp 437 > nul
if "%~1"=="" (
    echo Usage: %~nx0 ^<zapret folder^>
    exit /b 1
)
for %%I in ("%~1") do set "ROOT=%%~fI"
if not "!ROOT:~-1!"=="\" set "ROOT=!ROOT!\"
set "OUT=%~dp0..\crates\zapret\tests\fixtures\"
if not exist "%OUT%" mkdir "%OUT%"
del /q "%OUT%*.bat" "%OUT%*.args" 2>nul
>"%OUT%ROOT.txt" echo !ROOT!

:: The game filter the Rust golden test uses: TCP and UDP on 1024-65535.
set "GameFilter=1024-65535"
set "GameFilterTCP=1024-65535"
set "GameFilterUDP=1024-65535"

:: Flowseal's strategies only: other .bat files in the folder may be the user's own scripts.
for %%F in ("%ROOT%general*.bat") do (
    copy /y "%%~fF" "%OUT%" > nul
    call :parse "%%~fF" > "%OUT%%%~nF.args"
)
echo Fixtures written to %OUT%
exit /b 0

:parse
set "selectedFile=%~1"
:: --- copied from service.bat :service_install ---
set "args_with_value=sni host altorder"
set "args="
set "capture=0"
set "mergeargs=0"
set "BIN=%ROOT%bin\"
set "LISTS=%ROOT%lists\"
set QUOTE="

for /f "tokens=*" %%a in ('type "!selectedFile!"') do (
    set "line=%%a"
    call set "line=%%line:^!=EXCL_MARK%%"
    call set "line=!line!"

    echo !line! | findstr /i "winws.exe" >nul
    if not errorlevel 1 (
        set "capture=1"
    )

    if !capture!==1 (
        if not defined args (
            set "line=!line:*winws.exe"=!"
        )

        set "temp_args="
        for %%i in (!line!) do (
            set "arg=%%i"

            if not "!arg!"=="^" if not "!arg!"=="^^" (
                if "!arg:~0,2!" EQU "--" if not !mergeargs!==0 (
                    set "mergeargs=0"
                )

                if "!arg:~0,1!" EQU "!QUOTE!" (
                    set "arg=!arg:~1,-1!"

                    echo !arg! | findstr ":" >nul
                    if !errorlevel!==0 (
                        set "arg=\!QUOTE!!arg!\!QUOTE!"
                    ) else if "!arg:~0,1!"=="@" (
                        set "arg=\!QUOTE!@%ROOT%!arg:~1!\!QUOTE!"
                    ) else (
                        set "arg=\!QUOTE!%ROOT%!arg!\!QUOTE!"
                    )
                )

                if !mergeargs!==1 (
                    set "temp_args=!temp_args!,!arg!"
                ) else if !mergeargs!==3 (
                    set "temp_args=!temp_args!=!arg!"
                    set "mergeargs=1"
                ) else (
                    set "temp_args=!temp_args! !arg!"
                )

                if "!arg:~0,2!" EQU "--" (
                    set "mergeargs=2"
                ) else if !mergeargs! GEQ 1 (
                    if !mergeargs!==2 set "mergeargs=1"

                    for %%x in (!args_with_value!) do (
                        if /i "%%x"=="!arg!" (
                            set "mergeargs=3"
                        )
                    )
                )
            )
        )

        if not "!temp_args!"=="" (
            set "args=!args! !temp_args!"
        )
    )
)

set ARGS=%args%
call set "ARGS=%%ARGS:EXCL_MARK=^!%%"
echo Final args: !ARGS!
exit /b
```

Перед запуском сверьте блок с текущим `E:\Zapret\service.bat` (строки `:service_install`, с `set "args_with_value=..."` по `echo Final args`): если Flowseal поменял разбор, скопируйте новый текст и замените `%~dp0` на `%ROOT%`.

- [ ] **Шаг 6: Сгенерировать эталоны**

Run: `cmd /c tools\gen-bat-fixtures.cmd E:\Zapret`
Expected: `Fixtures written to ...`; в `crates/zapret/tests/fixtures/` по паре `.bat` + `.args` на каждый файл стратегии и `ROOT.txt` с `E:\Zapret\`. Права администратора не нужны.

- [ ] **Шаг 7: Падающий эталонный тест**

`crates/zapret/tests/golden.rs`:

```rust
//! The Rust parser against `service.bat` itself: fixtures from `tools\gen-bat-fixtures.cmd`.

use std::path::Path;

use zapret::batparse::{GameFilterVars, parse};

#[test]
fn final_args_match_service_bat_byte_for_byte() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures");
    let root = std::fs::read_to_string(dir.join("ROOT.txt")).expect("run tools\\gen-bat-fixtures.cmd first");
    let root = root.trim_end();
    let vars = GameFilterVars { all: "1024-65535".into(), tcp: "1024-65535".into(), udp: "1024-65535".into() };
    let mut compared = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bat")) {
            continue;
        }
        let bat = zapret::read_text(&path).unwrap();
        let expected = zapret::read_text(&path.with_extension("args")).unwrap();
        let expected = expected.trim_end_matches(['\r', '\n']);
        let parsed = parse(&bat, root, &vars);
        if !expected.contains("--") {
            // service.bat captured nothing: not a strategy (e.g. a helper .bat in the root).
            assert!(parsed.is_err(), "{} should not parse", path.display());
            continue;
        }
        let parsed = parsed.unwrap_or_else(|e| panic!("{}: {e:#}", path.display()));
        assert_eq!(format!("Final args: {}", parsed.final_args), expected, "{}", path.display());
        compared += 1;
    }
    assert!(compared >= 20, "only {compared} strategies compared");
}
```

- [ ] **Шаг 8: Прогнать эталоны**

Run: `cargo test -p zapret --test golden`
Expected: PASS. Если какой-то файл расходится, сравните строку `Final args` посимвольно: расхождение — ошибка порта (`for_tokens`, `merge`, `quote_arg`, `expand`), а не эталона. Исправляйте порт, пока все стратегии не совпадут.

- [ ] **Шаг 9: Коммит**

```bash
git add crates/zapret/src/batparse.rs crates/zapret/src/lib.rs crates/zapret/tests/batparse.rs crates/zapret/tests/golden.rs crates/zapret/tests/fixtures tools/gen-bat-fixtures.cmd
git commit -m "feat(zapret): port the service.bat argument parser with golden files"
```

---

### Задача 5: `zapret` — игровой фильтр, ipset, свои списки, фейки, флаг обновлений

**Files:**
- Create: `crates/zapret/src/game_filter.rs`, `crates/zapret/src/ipset.rs`, `crates/zapret/src/user_lists.rs`, `crates/zapret/src/fakes.rs`, `crates/zapret/src/flags.rs`
- Modify: `crates/zapret/src/lib.rs` (модули), `crates/zapret/src/batparse.rs` (добавить `parse_file`)
- Modify: `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md` (5.5, игровой фильтр)
- Test: `crates/zapret/tests/settings_files.rs`

**Interfaces:**
- Consumes: `Folder`, `read_text`, `sha256_hex`, `batparse::{GameFilterVars, ParsedArgs, parse}`
- Produces:
  - `game_filter::GameFilterMode { Disabled, All, Tcp, Udp }` (serde lowercase), `GameFilter { mode, tcp: String, udp: String }` + `Default`, `.vars() -> GameFilterVars`, `DEFAULT_RANGE`, `read(&Folder) -> GameFilter`, `parse(&str) -> GameFilter`, `write(&Folder, &GameFilter) -> anyhow::Result<()>`, `validate_range(&str) -> Option<String>`
  - `batparse::parse_file(folder: &Folder, path: &Path) -> anyhow::Result<ParsedArgs>` (с текущим игровым фильтром)
  - `ipset::IpsetMode { Loaded, None, Any }` (serde lowercase), `NONE_MARKER`, `mode(&Folder)`, `mode_of(&str)`, `has_backup(&Folder) -> bool`, `set_mode(&Folder, IpsetMode) -> anyhow::Result<()>`, `enter_test_any(&Folder)`, `leave_test_any(&Folder)`, `install(&Folder, downloaded: &Path) -> anyhow::Result<()>`
  - `user_lists::UserList { General, Exclude, IpsetExclude }`, `UserList::ALL`, `.file_name()`, `ensure_defaults(&Folder) -> io::Result<()>`, `read(&Folder, UserList) -> io::Result<Vec<String>>`, `add(&Folder, UserList, &str) -> anyhow::Result<()>`, `remove(&Folder, UserList, &str) -> anyhow::Result<()>`, `normalize(UserList, &str) -> anyhow::Result<String>`
  - `fakes::FakeKind { Discord, Game }`, `.file_name()`, `Fakes { files: Vec<String>, discord: Option<String>, game: Option<String> }`, `state(&Folder) -> anyhow::Result<Fakes>`, `replace(&Folder, FakeKind, source: &str) -> anyhow::Result<()>`
  - `flags::{check_updates(&Folder) -> bool, set_check_updates(&Folder, bool) -> io::Result<()>}`

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret/tests/settings_files.rs`:

```rust
mod common;

use std::fs;

use zapret::batparse::GameFilterVars;
use zapret::fakes::FakeKind;
use zapret::game_filter::{GameFilter, GameFilterMode, parse, validate_range};
use zapret::ipset::IpsetMode;
use zapret::user_lists::UserList;

#[test]
fn game_filter_reads_the_service_bat_format() {
    let gf = parse("mode=all\r\ntcp=1024-65535\r\nudp=50000-50100,1024-2000\r\n");
    assert_eq!(gf.mode, GameFilterMode::All);
    assert_eq!(gf.udp, "50000-50100,1024-2000");
    assert_eq!(gf.vars().tcp, "1024-65535");
}

#[test]
fn game_filter_reads_legacy_single_word_files() {
    assert_eq!(parse("udp\r\n").mode, GameFilterMode::Udp);
    assert_eq!(parse("all").mode, GameFilterMode::All);
    assert_eq!(parse("tcp=80\r\n").mode, GameFilterMode::Disabled);
    assert_eq!(parse("mode=all \r\n").mode, GameFilterMode::Disabled, "service.bat compares the value as is");
}

#[test]
fn bad_ranges_fall_back_to_the_default() {
    assert_eq!(parse("mode=tcp\ntcp=0-10\n").tcp, "1024-65535");
    assert_eq!(validate_range(" 80 , 443-444").as_deref(), Some("80,443-444"));
    for bad in ["", "70000", "10-5", "123456", "1-2-3", "a", "80,,81", "080"] {
        assert_eq!(validate_range(bad), None, "{bad}");
    }
}

#[test]
fn game_filter_vars_follow_the_mode() {
    let udp = GameFilter { mode: GameFilterMode::Udp, tcp: "1000-2000".into(), udp: "3000".into() };
    assert_eq!(udp.vars(), GameFilterVars { all: "3000".into(), tcp: "12".into(), udp: "3000".into() });
    assert_eq!(GameFilter::default().vars(), GameFilterVars::disabled());
}

#[test]
fn game_filter_write_then_read() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let gf = GameFilter { mode: GameFilterMode::Tcp, tcp: "1024-2000".into(), udp: "1024-65535".into() };
    zapret::game_filter::write(&folder, &gf).unwrap();
    assert_eq!(zapret::game_filter::read(&folder), gf);
    assert_eq!(
        fs::read_to_string(dir.path().join("utils").join("game_filter.enabled")).unwrap(),
        "mode=tcp\r\ntcp=1024-2000\r\nudp=1024-65535\r\n"
    );
    let bad = GameFilter { mode: GameFilterMode::All, tcp: "x".into(), udp: "1".into() };
    assert!(zapret::game_filter::write(&folder, &bad).is_err());
}

#[test]
fn parse_file_uses_the_current_game_filter() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let bat = dir.path().join("general.bat");
    fs::write(&bat, "start \"z\" /min \"%BIN%winws.exe\" --wf-tcp=80,%GameFilterTCP%\r\n").unwrap();
    assert_eq!(zapret::batparse::parse_file(&folder, &bat).unwrap().final_args, "  --wf-tcp 80,12");
    fs::write(dir.path().join("utils").join("game_filter.enabled"), "mode=tcp\r\ntcp=2000\r\n").unwrap();
    assert_eq!(zapret::batparse::parse_file(&folder, &bat).unwrap().final_args, "  --wf-tcp 80,2000");
}

#[test]
fn ipset_modes_cycle_like_service_bat() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let list = dir.path().join("lists").join("ipset-all.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    assert_eq!(zapret::ipset::mode(&folder), IpsetMode::Loaded);

    zapret::ipset::set_mode(&folder, IpsetMode::None).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "203.0.113.113/32\r\n");
    assert!(zapret::ipset::has_backup(&folder));

    zapret::ipset::set_mode(&folder, IpsetMode::Any).unwrap();
    assert_eq!(fs::read(&list).unwrap().len(), 0);

    zapret::ipset::set_mode(&folder, IpsetMode::Loaded).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
    assert!(!zapret::ipset::has_backup(&folder));
}

#[test]
fn ipset_loaded_needs_a_backup() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    assert_eq!(zapret::ipset::mode(&folder), IpsetMode::Any, "a missing list counts as any");
    let err = zapret::ipset::set_mode(&folder, IpsetMode::Loaded).unwrap_err().to_string();
    assert!(err.contains("backup"), "{err}");
    assert_eq!(zapret::ipset::mode_of("\u{feff}\r\n"), IpsetMode::Any);
}

#[test]
fn ipset_test_switch_puts_the_list_back() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let list = dir.path().join("lists").join("ipset-all.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    zapret::ipset::enter_test_any(&folder).unwrap();
    assert_eq!(zapret::ipset::mode(&folder), IpsetMode::Any);
    zapret::ipset::leave_test_any(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
    zapret::ipset::leave_test_any(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
}

#[test]
fn ipset_install_keeps_the_previous_list() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let lists = dir.path().join("lists");
    fs::write(lists.join("ipset-all.txt"), "old\r\n").unwrap();
    fs::write(lists.join("ipset-all.txt.download"), "new\r\n").unwrap();
    zapret::ipset::install(&folder, &lists.join("ipset-all.txt.download")).unwrap();
    assert_eq!(fs::read_to_string(lists.join("ipset-all.txt")).unwrap(), "new\r\n");
    assert_eq!(fs::read_to_string(lists.join("ipset-all.txt.hub-prev")).unwrap(), "old\r\n");
}

#[test]
fn user_lists_start_with_service_bat_defaults() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    zapret::user_lists::ensure_defaults(&folder).unwrap();
    assert_eq!(
        zapret::user_lists::read(&folder, UserList::General).unwrap(),
        ["# Never leave this file empty", "domain.example.abc"]
    );
    assert_eq!(zapret::user_lists::read(&folder, UserList::IpsetExclude).unwrap(), ["203.0.113.113/32"]);
}

#[test]
fn user_list_entries_are_validated_and_deduplicated() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    zapret::user_lists::ensure_defaults(&folder).unwrap();
    zapret::user_lists::add(&folder, UserList::General, " Discord.COM ").unwrap();
    zapret::user_lists::add(&folder, UserList::General, "discord.com").unwrap();
    let general = zapret::user_lists::read(&folder, UserList::General).unwrap();
    assert_eq!(general.iter().filter(|l| *l == "discord.com").count(), 1);
    assert!(zapret::user_lists::add(&folder, UserList::General, "not a domain").is_err());
    zapret::user_lists::add(&folder, UserList::IpsetExclude, "10.0.0.0/8").unwrap();
    zapret::user_lists::add(&folder, UserList::IpsetExclude, "::1").unwrap();
    assert!(zapret::user_lists::add(&folder, UserList::IpsetExclude, "10.0.0.0/33").is_err());
}

#[test]
fn removing_the_last_entry_puts_the_placeholder_back() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    zapret::user_lists::ensure_defaults(&folder).unwrap();
    zapret::user_lists::add(&folder, UserList::IpsetExclude, "10.0.0.1").unwrap();
    zapret::user_lists::remove(&folder, UserList::IpsetExclude, "203.0.113.113/32").unwrap();
    assert_eq!(zapret::user_lists::read(&folder, UserList::IpsetExclude).unwrap(), ["10.0.0.1"]);
    zapret::user_lists::remove(&folder, UserList::IpsetExclude, "10.0.0.1").unwrap();
    assert_eq!(zapret::user_lists::read(&folder, UserList::IpsetExclude).unwrap(), ["203.0.113.113/32"]);
}

#[test]
fn finds_and_replaces_the_active_fake() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let bin = dir.path().join("bin");
    fs::write(bin.join("stun.bin"), "a").unwrap();
    fs::write(bin.join("quic.bin"), "b").unwrap();
    fs::write(bin.join("ACTIVE_DISCORD_UDP.bin"), "a").unwrap();
    let state = zapret::fakes::state(&folder).unwrap();
    assert_eq!(state.files, ["quic", "stun"]);
    assert_eq!(state.discord.as_deref(), Some("stun"));
    assert_eq!(state.game, None);

    zapret::fakes::replace(&folder, FakeKind::Discord, "quic").unwrap();
    assert_eq!(zapret::fakes::state(&folder).unwrap().discord.as_deref(), Some("quic"));
    assert!(zapret::fakes::replace(&folder, FakeKind::Game, "..\\service").is_err());
}

#[test]
fn check_updates_flag_toggles() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    assert!(!zapret::flags::check_updates(&folder));
    zapret::flags::set_check_updates(&folder, true).unwrap();
    assert!(zapret::flags::check_updates(&folder));
    zapret::flags::set_check_updates(&folder, false).unwrap();
    zapret::flags::set_check_updates(&folder, false).unwrap();
    assert!(!zapret::flags::check_updates(&folder));
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test settings_files`
Expected: FAIL при компиляции, модулей нет.

- [ ] **Шаг 3: Реализация**

В `crates/zapret/src/lib.rs` модули теперь такие:

```rust
pub mod batparse;
pub mod fakes;
pub mod flags;
pub mod folder;
pub mod game_filter;
pub mod ipset;
pub mod strategies;
pub mod targets;
pub mod user_lists;
```

В конец `crates/zapret/src/batparse.rs`:

```rust
/// A strategy file with the folder's current game filter, as «Применить» and the tests see it.
pub fn parse_file(folder: &crate::folder::Folder, path: &Path) -> anyhow::Result<ParsedArgs> {
    let bat = crate::read_text(path)?;
    parse(&bat, &folder.root_dp0(), &crate::game_filter::read(folder).vars())
        .map_err(|err| err.context(path.display().to_string()))
}
```

`crates/zapret/src/game_filter.rs`:

```rust
//! `utils\game_filter.enabled`, read like `:game_switch_status`, written like
//! `:save_game_filter_settings`.

use crate::batparse::GameFilterVars;
use crate::folder::Folder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum GameFilterMode {
    Disabled,
    All,
    Tcp,
    Udp,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct GameFilter {
    pub mode: GameFilterMode,
    pub tcp: String,
    pub udp: String,
}

pub const DEFAULT_RANGE: &str = "1024-65535";

impl Default for GameFilter {
    fn default() -> Self {
        Self { mode: GameFilterMode::Disabled, tcp: DEFAULT_RANGE.into(), udp: DEFAULT_RANGE.into() }
    }
}

impl GameFilter {
    /// The values `service.bat` substitutes; `12` stands for "off".
    pub fn vars(&self) -> GameFilterVars {
        let off = || "12".to_owned();
        match self.mode {
            GameFilterMode::All => GameFilterVars { all: self.tcp.clone(), tcp: self.tcp.clone(), udp: self.udp.clone() },
            GameFilterMode::Tcp => GameFilterVars { all: self.tcp.clone(), tcp: self.tcp.clone(), udp: off() },
            GameFilterMode::Udp => GameFilterVars { all: self.udp.clone(), tcp: off(), udp: self.udp.clone() },
            GameFilterMode::Disabled => GameFilterVars::disabled(),
        }
    }
}

fn path(folder: &Folder) -> std::path::PathBuf {
    folder.utils().join("game_filter.enabled")
}

pub fn read(folder: &Folder) -> GameFilter {
    crate::read_text(&path(folder)).map(|text| parse(&text)).unwrap_or_default()
}

/// `for /f "usebackq tokens=1,* delims==" %%A in (file)` and the range checks after it.
pub fn parse(text: &str) -> GameFilter {
    let mut mode = String::from("disabled");
    let (mut tcp, mut udp) = (None, None);
    for line in text.lines() {
        let line = line.trim_start_matches('=');
        if line.is_empty() || line.starts_with(';') {
            continue;
        }
        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key, value.trim_start_matches('=')),
            None => (line, ""),
        };
        match (key.to_ascii_lowercase().as_str(), value.is_empty()) {
            ("mode", _) => mode = value.to_owned(),
            ("all", _) => mode = "all".into(),
            ("udp", true) => mode = "udp".into(),
            ("udp", false) => udp = Some(value.to_owned()),
            ("tcp", true) => mode = "tcp".into(),
            ("tcp", false) => tcp = Some(value.to_owned()),
            _ => {}
        }
    }
    let mode = match mode.to_ascii_lowercase().as_str() {
        "all" => GameFilterMode::All,
        "tcp" => GameFilterMode::Tcp,
        "udp" => GameFilterMode::Udp,
        _ => GameFilterMode::Disabled,
    };
    GameFilter {
        mode,
        tcp: tcp.as_deref().and_then(validate_range).unwrap_or_else(|| DEFAULT_RANGE.into()),
        udp: udp.as_deref().and_then(validate_range).unwrap_or_else(|| DEFAULT_RANGE.into()),
    }
}

/// `:validate_game_filter_range`: spaces dropped; every comma item is `N` or `N-M`,
/// at most 5 digits, no leading zero, up to 65535, `N <= M`.
pub fn validate_range(input: &str) -> Option<String> {
    let range: String = input.chars().filter(|c| *c != ' ').collect();
    if range.is_empty() {
        return None;
    }
    let number = |part: &str| {
        !part.is_empty() && part.len() <= 5 && !part.starts_with('0') && part.bytes().all(|b| b.is_ascii_digit())
    };
    for item in range.split(',') {
        let (start, end) = item.split_once('-').unwrap_or((item, item));
        if !number(start) || !number(end) {
            return None;
        }
        let (start, end): (u32, u32) = (start.parse().ok()?, end.parse().ok()?);
        if start > 65535 || end > 65535 || start > end {
            return None;
        }
    }
    Some(range)
}

/// `:save_game_filter_settings`: `mode=`, `tcp=`, `udp=` with CRLF. "Off" is `mode=disabled`
/// and keeps the ranges, as `service.bat` does.
pub fn write(folder: &Folder, gf: &GameFilter) -> anyhow::Result<()> {
    let tcp = validate_range(&gf.tcp).ok_or_else(|| anyhow::anyhow!("invalid TCP port range: {}", gf.tcp))?;
    let udp = validate_range(&gf.udp).ok_or_else(|| anyhow::anyhow!("invalid UDP port range: {}", gf.udp))?;
    let mode = match gf.mode {
        GameFilterMode::Disabled => "disabled",
        GameFilterMode::All => "all",
        GameFilterMode::Tcp => "tcp",
        GameFilterMode::Udp => "udp",
    };
    std::fs::write(path(folder), format!("mode={mode}\r\ntcp={tcp}\r\nudp={udp}\r\n"))?;
    log::info!("game filter: mode={mode} tcp={tcp} udp={udp}");
    Ok(())
}
```

`crates/zapret/src/ipset.rs`:

```rust
//! `lists\ipset-all.txt` modes as `:ipset_switch_status` / `:ipset_switch` keep them, the
//! test-time "any" switch, and installing a downloaded list.

use std::path::{Path, PathBuf};

use crate::folder::Folder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum IpsetMode {
    Loaded,
    None,
    Any,
}

/// The address `service.bat` writes to mean "match nothing".
pub const NONE_MARKER: &str = "203.0.113.113/32";

fn list(folder: &Folder) -> PathBuf {
    folder.lists().join("ipset-all.txt")
}

fn backup(folder: &Folder) -> PathBuf {
    folder.lists().join("ipset-all.txt.backup")
}

fn test_backup(folder: &Folder) -> PathBuf {
    folder.lists().join("ipset-all.hub-test-backup.txt")
}

/// A missing file counts as `any`, as in `service.bat` (zero lines).
pub fn mode(folder: &Folder) -> IpsetMode {
    match crate::read_text(&list(folder)) {
        Ok(content) => mode_of(&content),
        Err(_) => IpsetMode::Any,
    }
}

/// Blank → any; the marker → none; anything else → loaded.
pub fn mode_of(content: &str) -> IpsetMode {
    let body = content.trim_start_matches('\u{feff}');
    if body.trim().is_empty() {
        IpsetMode::Any
    } else if body.contains(NONE_MARKER) {
        IpsetMode::None
    } else {
        IpsetMode::Loaded
    }
}

pub fn has_backup(folder: &Folder) -> bool {
    backup(folder).is_file()
}

/// Switches to `target`. Leaving `loaded` moves the list to `.backup`; going back needs it.
pub fn set_mode(folder: &Folder, target: IpsetMode) -> anyhow::Result<()> {
    let current = mode(folder);
    if current == target {
        return Ok(());
    }
    let (list, backup) = (list(folder), backup(folder));
    if target == IpsetMode::Loaded {
        anyhow::ensure!(backup.is_file(), "no ipset backup to restore: update the ipset list first");
    }
    if current == IpsetMode::Loaded {
        std::fs::rename(&list, &backup)?;
    }
    match target {
        IpsetMode::None => std::fs::write(&list, format!("{NONE_MARKER}\r\n"))?,
        IpsetMode::Any => std::fs::write(&list, "")?,
        IpsetMode::Loaded => std::fs::rename(&backup, &list)?,
    }
    log::info!("ipset: {current:?} -> {target:?}");
    Ok(())
}

/// DPI tests need every address matched: the list is emptied and its content kept aside.
pub fn enter_test_any(folder: &Folder) -> anyhow::Result<()> {
    let (list, saved) = (list(folder), test_backup(folder));
    if list.is_file() {
        std::fs::copy(&list, &saved)?;
    } else {
        std::fs::write(&saved, "")?;
    }
    std::fs::write(&list, "")?;
    log::info!("ipset: emptied for DPI tests, kept in {}", saved.display());
    Ok(())
}

/// Undoes `enter_test_any`; nothing to do when it was not entered.
pub fn leave_test_any(folder: &Folder) -> anyhow::Result<()> {
    let (list, saved) = (list(folder), test_backup(folder));
    if saved.is_file() {
        std::fs::rename(&saved, &list)?;
        log::info!("ipset: restored after DPI tests");
    }
    Ok(())
}

/// Swaps a downloaded list in (`downloaded` sits next to it, so the rename is atomic); the old
/// list stays as `ipset-all.txt.hub-prev`.
pub fn install(folder: &Folder, downloaded: &Path) -> anyhow::Result<()> {
    let list = list(folder);
    anyhow::ensure!(std::fs::metadata(downloaded)?.len() > 0, "the downloaded ipset list is empty");
    if list.is_file() {
        std::fs::copy(&list, folder.lists().join("ipset-all.txt.hub-prev"))?;
    }
    std::fs::rename(downloaded, &list)?;
    log::info!("ipset: installed a new ipset-all.txt");
    Ok(())
}
```

`crates/zapret/src/user_lists.rs`:

```rust
//! The three lists `service.bat :load_user_lists` creates, with entry checks.

use crate::folder::Folder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum UserList {
    General,
    Exclude,
    IpsetExclude,
}

impl UserList {
    pub const ALL: [UserList; 3] = [Self::General, Self::Exclude, Self::IpsetExclude];

    pub fn file_name(self) -> &'static str {
        match self {
            Self::General => "list-general-user.txt",
            Self::Exclude => "list-exclude-user.txt",
            Self::IpsetExclude => "ipset-exclude-user.txt",
        }
    }

    /// What `:load_user_lists` writes into a missing file.
    fn default_content(self) -> &'static str {
        match self {
            Self::General => "# Never leave this file empty\r\ndomain.example.abc\r\n",
            Self::Exclude => "domain.example.abc\r\n",
            Self::IpsetExclude => "203.0.113.113/32\r\n",
        }
    }

    /// An empty list matches everything in winws, so the last entry is replaced by this.
    fn placeholder(self) -> &'static str {
        match self {
            Self::General | Self::Exclude => "domain.example.abc",
            Self::IpsetExclude => "203.0.113.113/32",
        }
    }
}

pub fn ensure_defaults(folder: &Folder) -> std::io::Result<()> {
    for list in UserList::ALL {
        let path = folder.lists().join(list.file_name());
        if !path.exists() {
            std::fs::write(&path, list.default_content())?;
        }
    }
    Ok(())
}

/// Every non-blank line, comments included.
pub fn read(folder: &Folder, list: UserList) -> std::io::Result<Vec<String>> {
    let path = folder.lists().join(list.file_name());
    if !path.exists() {
        return Ok(Vec::new());
    }
    Ok(crate::read_text(&path)?.lines().filter(|l| !l.trim().is_empty()).map(str::to_owned).collect())
}

pub fn add(folder: &Folder, list: UserList, entry: &str) -> anyhow::Result<()> {
    let entry = normalize(list, entry)?;
    let mut lines = read(folder, list)?;
    if lines.iter().any(|line| line.trim().eq_ignore_ascii_case(&entry)) {
        return Ok(());
    }
    lines.push(entry);
    save(folder, list, &lines)
}

pub fn remove(folder: &Folder, list: UserList, entry: &str) -> anyhow::Result<()> {
    let mut lines = read(folder, list)?;
    lines.retain(|line| line.trim() != entry.trim());
    if !lines.iter().any(|line| !line.trim_start().starts_with('#')) {
        lines.push(list.placeholder().to_owned());
    }
    save(folder, list, &lines)
}

fn save(folder: &Folder, list: UserList, lines: &[String]) -> anyhow::Result<()> {
    let mut text = lines.join("\r\n");
    text.push_str("\r\n");
    std::fs::write(folder.lists().join(list.file_name()), text)?;
    log::info!("{}: {} lines", list.file_name(), lines.len());
    Ok(())
}

/// A lower-cased domain for the host lists; an IP or CIDR for the ipset list.
pub fn normalize(list: UserList, entry: &str) -> anyhow::Result<String> {
    let entry = entry.trim();
    match list {
        UserList::IpsetExclude => {
            anyhow::ensure!(is_ip_or_cidr(entry), "not an IP address or CIDR: {entry}");
            Ok(entry.to_owned())
        }
        UserList::General | UserList::Exclude => {
            let domain = entry.to_ascii_lowercase();
            anyhow::ensure!(is_domain(&domain), "not a domain name: {entry}");
            Ok(domain)
        }
    }
}

fn is_domain(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    domain.len() <= 253
        && labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
}

fn is_ip_or_cidr(entry: &str) -> bool {
    let (ip, prefix) = match entry.split_once('/') {
        Some((ip, prefix)) => (ip, Some(prefix)),
        None => (entry, None),
    };
    let Ok(ip) = ip.parse::<std::net::IpAddr>() else { return false };
    let max = if ip.is_ipv4() { 32 } else { 128 };
    prefix.is_none_or(|p| p.parse::<u8>().is_ok_and(|p| p <= max))
}
```

`crates/zapret/src/fakes.rs`:

```rust
//! `bin\ACTIVE_DISCORD_UDP.bin` and `bin\ACTIVE_GAME_UDP.bin`: which fake each one is a copy of,
//! and swapping it (`:replace_active_fakes`).

use std::path::Path;

use crate::folder::Folder;

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum FakeKind {
    Discord,
    Game,
}

impl FakeKind {
    pub fn file_name(self) -> &'static str {
        match self {
            Self::Discord => "ACTIVE_DISCORD_UDP.bin",
            Self::Game => "ACTIVE_GAME_UDP.bin",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Fakes {
    /// Base names of `bin\*.bin` except `ACTIVE_*`, sorted.
    pub files: Vec<String>,
    /// The file each active fake is a copy of (same SHA-256), when one matches.
    pub discord: Option<String>,
    pub game: Option<String>,
}

pub fn state(folder: &Folder) -> anyhow::Result<Fakes> {
    let bin = folder.bin();
    let mut files: Vec<(String, String)> = Vec::new();
    for entry in std::fs::read_dir(&bin)? {
        let path = entry?.path();
        if !path.is_file() || !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bin")) {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else { continue };
        if stem.get(..7).is_some_and(|p| p.eq_ignore_ascii_case("ACTIVE_")) {
            continue;
        }
        files.push((stem.to_owned(), hash(&path)?));
    }
    files.sort();
    let current = |kind: FakeKind| -> Option<String> {
        let active = hash(&bin.join(kind.file_name())).ok()?;
        files.iter().find(|(_, h)| *h == active).map(|(name, _)| name.clone())
    };
    Ok(Fakes {
        discord: current(FakeKind::Discord),
        game: current(FakeKind::Game),
        files: files.iter().map(|(name, _)| name.clone()).collect(),
    })
}

/// Copies `bin\<source>.bin` over the active fake; `source` must be one of `state().files`.
pub fn replace(folder: &Folder, kind: FakeKind, source: &str) -> anyhow::Result<()> {
    anyhow::ensure!(state(folder)?.files.iter().any(|f| f == source), "no fake named {source} in bin");
    let bin = folder.bin();
    let active = bin.join(kind.file_name());
    let _ = std::fs::remove_file(&active);
    std::fs::copy(bin.join(format!("{source}.bin")), &active)?;
    log::info!("{} := {source}.bin", kind.file_name());
    Ok(())
}

fn hash(path: &Path) -> std::io::Result<String> {
    Ok(crate::sha256_hex(&std::fs::read(path)?))
}
```

`crates/zapret/src/flags.rs`:

```rust
//! `utils\check_updates.enabled`: Flowseal's own "check for updates" switch.

use crate::folder::Folder;

fn path(folder: &Folder) -> std::path::PathBuf {
    folder.utils().join("check_updates.enabled")
}

pub fn check_updates(folder: &Folder) -> bool {
    path(folder).is_file()
}

pub fn set_check_updates(folder: &Folder, on: bool) -> std::io::Result<()> {
    let path = path(folder);
    if on {
        std::fs::write(&path, "ENABLED\r\n")
    } else if path.exists() {
        std::fs::remove_file(&path)
    } else {
        Ok(())
    }
}
```

- [ ] **Шаг 4: Тесты проходят**

Run: `cargo test -p zapret`
Expected: PASS (все тесты `folder`, `batparse`, `golden`, `settings_files`).

- [ ] **Шаг 5: Поправить спецификацию**

В разделе 5.5 спецификации замените фразу «Отключение удаляет файл.» на «Отключение пишет `mode=disabled` и сохраняет диапазоны — так же, как `service.bat`.».

- [ ] **Шаг 6: Коммит**

```bash
git add crates/zapret docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md
git commit -m "feat(zapret): manage game filter, ipset, lists, fakes and update flag"
```

---

### Задача 6: `zapret` — процессы и служба

**Files:**
- Create: `crates/zapret/src/procs.rs`, `crates/zapret/src/service.rs`
- Modify: `crates/zapret/src/lib.rs` (`pub mod procs; pub mod service;`), `crates/zapret/src/folder.rs` (добавить `candidates`)
- Test: `crates/zapret/tests/service.rs`

**Interfaces:**
- Consumes: `exec::{run, CREATE_NO_WINDOW, decode}`, `probe::curl::{check, CheckKind, CheckStatus}`, `Folder`, `strategies::find`, `batparse::{ParsedArgs, parse_file}`, `targets::load`
- Produces:
  - `procs::Proc { pid: u32, exe: Option<PathBuf>, args: Vec<String> }` (serde), `find(image: &str) -> Vec<Proc>`, `is_running(image) -> bool`, `kill_all(image) -> anyhow::Result<()>`, `spawn_detached(&Proc) -> anyhow::Result<()>`
  - `procs::Winws::start(folder, tail: &str) -> io::Result<Winws>`, `.wait_ready(max: Duration) -> Result<(), String>`, `.is_alive(&mut self) -> bool`, `.output_text() -> String`, `.stop(self) -> String`
  - `service::SERVICE = "zapret"`, `SvcState { NotInstalled, Stopped, StartPending, StopPending, Running, Other }`
  - `service::status(name) -> Option<(SvcState, u32 /* pid */)>`, `state(name) -> SvcState`, `SvcConfig { image_path: String, auto_start: bool }`, `config(name) -> Option<SvcConfig>`, `active_services() -> Vec<(String, String)>`
  - `service::strategy() -> Option<String>`, `set_strategy(name) -> anyhow::Result<()>`
  - `service::create(bin_path, auto_start) -> anyhow::Result<()>`, `install(folder, name, &ParsedArgs) -> anyhow::Result<()>`, `request_delete(name)`, `stop_and_delete(name) -> anyhow::Result<()>`, `remove() -> anyhow::Result<()>`, `start(name)`, `stop(name)`, `restart(name)`, `restart_if_running(name)`, `reinstall_active(folder)`
  - `service::tcp_timestamps() -> Option<bool>`, `parse_timestamps(&str) -> Option<bool>`, `enable_tcp_timestamps() -> anyhow::Result<()>`
  - `service::QuickItem { name, status: CheckStatus, time_ms: Option<u32> }`, `QuickCheck { ok, service: SvcState, winws_running, checks: Vec<QuickItem> }`, `quick_check(folder) -> QuickCheck`, `apply(folder, name) -> anyhow::Result<QuickCheck>`
  - `folder::candidates() -> Vec<PathBuf>`

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret/tests/service.rs`:

```rust
use std::process::Command;
use std::time::Duration;

use zapret::service::{SvcState, parse_timestamps, state};

#[test]
fn reads_the_rfc1323_line_in_any_language() {
    let english = "TCP Global Parameters\r\n----\r\nECN Capability                      : disabled\r\nRFC 1323 Timestamps                 : enabled \r\n";
    assert_eq!(parse_timestamps(english), Some(true));
    let russian = "Временные метки RFC 1323            : disabled\r\n";
    assert_eq!(parse_timestamps(russian), Some(false));
    assert_eq!(parse_timestamps("nothing here"), None);
}

#[test]
fn reads_service_states_through_the_scm() {
    assert_eq!(state("BFE"), SvcState::Running);
    assert_eq!(state("zapret-hub-no-such-service"), SvcState::NotInstalled);
    assert!(!zapret::service::active_services().is_empty());
}

#[test]
fn finds_a_process_with_its_arguments() {
    let mut child = Command::new("ping").args(["-n", "20", "127.0.0.1"]).spawn().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    let found = zapret::procs::find("PING.EXE");
    let ours = found.iter().find(|p| p.pid == child.id()).expect("our ping is listed");
    assert_eq!(ours.args, ["-n", "20", "127.0.0.1"]);
    assert!(ours.exe.as_ref().is_some_and(|e| e.to_string_lossy().to_lowercase().ends_with("ping.exe")));
    child.kill().unwrap();
    child.wait().unwrap();
}

#[test]
#[ignore = "needs admin rights and the zapret service installed from E:\\Zapret"]
fn live_status_of_the_zapret_service() {
    let status = zapret::service::status(zapret::service::SERVICE);
    let config = zapret::service::config(zapret::service::SERVICE);
    let strategy = zapret::service::strategy();
    let timestamps = zapret::service::tcp_timestamps();
    let candidates = zapret::folder::candidates();
    eprintln!("zapret: {status:?}\nconfig: {config:?}\nstrategy: {strategy:?}\ntimestamps: {timestamps:?}\ncandidates: {candidates:?}");
    assert!(status.is_some(), "the zapret service is not installed");
    let config = config.expect("the service has a configuration");
    assert!(config.image_path.to_lowercase().contains("winws.exe"), "{}", config.image_path);
    assert!(strategy.is_some(), "service.bat records the installed strategy in the registry");
    assert!(timestamps.is_some(), "netsh reports the TCP timestamps setting");
    assert!(
        candidates.iter().any(|root| root.to_string_lossy().trim_end_matches('\\').eq_ignore_ascii_case(r"E:\Zapret")),
        "{candidates:?}"
    );
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test service`
Expected: FAIL при компиляции, модулей `procs` и `service` нет.

- [ ] **Шаг 3: Реализация процессов**

`crates/zapret/src/procs.rs`:

```rust
//! Running processes: finding `winws.exe` and its arguments, stopping it, starting it again,
//! and the winws the tests run themselves.

use std::io::Read;
use std::os::windows::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

use crate::folder::Folder;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Proc {
    pub pid: u32,
    pub exe: Option<PathBuf>,
    /// Arguments after the program name.
    pub args: Vec<String>,
}

/// Processes whose image name is `image` (case-insensitive), with exe path and arguments.
pub fn find(image: &str) -> Vec<Proc> {
    let mut system = System::new();
    system.refresh_processes_specifics(
        ProcessesToUpdate::All,
        true,
        ProcessRefreshKind::nothing().with_exe(UpdateKind::Always).with_cmd(UpdateKind::Always),
    );
    system
        .processes()
        .values()
        .filter(|p| p.name().to_string_lossy().eq_ignore_ascii_case(image))
        .map(|p| Proc {
            pid: p.pid().as_u32(),
            exe: p.exe().map(Path::to_path_buf),
            args: p.cmd().iter().skip(1).map(|a| a.to_string_lossy().into_owned()).collect(),
        })
        .collect()
}

pub fn is_running(image: &str) -> bool {
    !find(image).is_empty()
}

/// `taskkill /IM <image> /F`, when one runs.
pub fn kill_all(image: &str) -> anyhow::Result<()> {
    if !is_running(image) {
        return Ok(());
    }
    exec::run(Command::new("taskkill").args(["/IM", image, "/F"]), Duration::from_secs(15), None)?.check()?;
    Ok(())
}

/// Starts a process from a snapshot again, without a window, not waiting for it.
pub fn spawn_detached(proc: &Proc) -> anyhow::Result<()> {
    let exe = proc.exe.as_ref().ok_or_else(|| anyhow::anyhow!("pid {} had no executable path", proc.pid))?;
    let mut cmd = Command::new(exe);
    cmd.args(&proc.args).creation_flags(exec::CREATE_NO_WINDOW);
    if let Some(dir) = exe.parent() {
        cmd.current_dir(dir);
    }
    let child = cmd.spawn()?;
    log::info!(target: "exec", "started {} (pid {}) {}", exe.display(), child.id(), proc.args.join(" "));
    Ok(())
}

/// A `winws.exe` the tests start directly, with the arguments the service would get (7.3).
pub struct Winws {
    child: Child,
    output: Arc<Mutex<Vec<u8>>>,
    readers: Vec<JoinHandle<()>>,
}

impl Winws {
    /// `"<bin>\winws.exe" <tail>`, working directory `bin`, no window.
    pub fn start(folder: &Folder, tail: &str) -> std::io::Result<Self> {
        let mut child = Command::new(folder.winws())
            .raw_arg(tail)
            .current_dir(folder.bin())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(exec::CREATE_NO_WINDOW)
            .spawn()?;
        let output = Arc::new(Mutex::new(Vec::new()));
        let mut readers = Vec::new();
        let pipes: [Option<Box<dyn Read + Send>>; 2] = [
            child.stdout.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
            child.stderr.take().map(|p| Box::new(p) as Box<dyn Read + Send>),
        ];
        for mut pipe in pipes.into_iter().flatten() {
            let output = Arc::clone(&output);
            readers.push(std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while let Ok(n) = pipe.read(&mut buf) {
                    if n == 0 {
                        break;
                    }
                    output.lock().unwrap().extend_from_slice(&buf[..n]);
                }
            }));
        }
        log::info!(target: "exec", "started winws (pid {}){tail}", child.id());
        Ok(Self { child, output, readers })
    }

    /// Waits until winws reports capture started, exits, or `max` passes. A winws that exited
    /// is a launch error: `Err` carries its exit status and output.
    pub fn wait_ready(&mut self, max: Duration) -> Result<(), String> {
        let started = Instant::now();
        loop {
            if let Ok(Some(status)) = self.child.try_wait() {
                std::thread::sleep(Duration::from_millis(100)); // let the readers drain
                return Err(format!("winws exited ({status}):\n{}", self.output_text()));
            }
            if self.output_text().contains("capture is started") || started.elapsed() >= max {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    pub fn output_text(&self) -> String {
        exec::decode(&self.output.lock().unwrap())
    }

    /// A winws that exits while the checks run is a launch error too.
    pub fn is_alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }

    /// Kills winws and returns everything it printed.
    pub fn stop(mut self) -> String {
        let _ = self.child.kill();
        let _ = self.child.wait();
        for reader in self.readers.drain(..) {
            let _ = reader.join();
        }
        let text = self.output_text();
        log::info!(target: "exec", "stopped winws (pid {})\n{text}", self.child.id());
        text
    }
}

impl Drop for Winws {
    fn drop(&mut self) {
        let _ = self.child.kill();
    }
}
```

- [ ] **Шаг 4: Реализация службы**

`crates/zapret/src/service.rs`:

```rust
//! The `zapret` Windows service: state through the Service Control Manager, changes through
//! `sc`/`net` exactly as `service.bat` makes them, and the quick check after an install (5.4).

use std::process::Command;
use std::time::{Duration, Instant};

use probe::curl::{CheckKind, CheckStatus};
use windows::Win32::System::Services::{
    CloseServiceHandle, ENUM_SERVICE_STATUS_PROCESSW, EnumServicesStatusExW, OpenSCManagerW, OpenServiceW,
    QUERY_SERVICE_CONFIGW, QueryServiceConfigW, QueryServiceStatusEx, SC_ENUM_PROCESS_INFO, SC_HANDLE,
    SC_MANAGER_CONNECT, SC_MANAGER_ENUMERATE_SERVICE, SC_STATUS_PROCESS_INFO, SERVICE_ACTIVE, SERVICE_AUTO_START,
    SERVICE_QUERY_CONFIG, SERVICE_QUERY_STATUS, SERVICE_RUNNING, SERVICE_START_PENDING, SERVICE_STATUS_PROCESS,
    SERVICE_STOP_PENDING, SERVICE_STOPPED, SERVICE_WIN32,
};
use windows::core::{HSTRING, PCWSTR};

use crate::batparse::ParsedArgs;
use crate::folder::Folder;

pub const SERVICE: &str = "zapret";
const SERVICE_KEY: &str = r"SYSTEM\CurrentControlSet\Services\zapret";
const STRATEGY_VALUE: &str = "zapret-discord-youtube";

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum SvcState {
    NotInstalled,
    Stopped,
    StartPending,
    StopPending,
    Running,
    Other,
}

struct Handle(SC_HANDLE);

impl Drop for Handle {
    fn drop(&mut self) {
        // SAFETY: the handle came from OpenSCManagerW/OpenServiceW and is closed once.
        unsafe {
            let _ = CloseServiceHandle(self.0);
        }
    }
}

fn manager() -> Option<Handle> {
    // SAFETY: plain call; the handle is wrapped for closing.
    unsafe {
        OpenSCManagerW(PCWSTR::null(), PCWSTR::null(), SC_MANAGER_CONNECT | SC_MANAGER_ENUMERATE_SERVICE)
            .ok()
            .map(Handle)
    }
}

fn open(name: &str, access: u32) -> Option<Handle> {
    let scm = manager()?;
    // SAFETY: `scm` is open; the service handle is wrapped for closing.
    unsafe { OpenServiceW(scm.0, &HSTRING::from(name), access).ok().map(Handle) }
}

/// State and process id; `None` when the service does not exist.
pub fn status(name: &str) -> Option<(SvcState, u32)> {
    let service = open(name, SERVICE_QUERY_STATUS)?;
    let mut buf = vec![0u8; size_of::<SERVICE_STATUS_PROCESS>()];
    let mut needed = 0u32;
    // SAFETY: the buffer has the size of the structure asked for.
    unsafe { QueryServiceStatusEx(service.0, SC_STATUS_PROCESS_INFO, Some(&mut buf), &mut needed) }.ok()?;
    // SAFETY: the call above filled one SERVICE_STATUS_PROCESS.
    let status = unsafe { std::ptr::read_unaligned(buf.as_ptr().cast::<SERVICE_STATUS_PROCESS>()) };
    let state = match status.dwCurrentState {
        SERVICE_STOPPED => SvcState::Stopped,
        SERVICE_START_PENDING => SvcState::StartPending,
        SERVICE_STOP_PENDING => SvcState::StopPending,
        SERVICE_RUNNING => SvcState::Running,
        _ => SvcState::Other,
    };
    Some((state, status.dwProcessId))
}

pub fn state(name: &str) -> SvcState {
    status(name).map_or(SvcState::NotInstalled, |(state, _)| state)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SvcConfig {
    pub image_path: String,
    pub auto_start: bool,
}

pub fn config(name: &str) -> Option<SvcConfig> {
    let service = open(name, SERVICE_QUERY_CONFIG)?;
    let mut needed = 0u32;
    // SAFETY: a size query with no buffer.
    let _ = unsafe { QueryServiceConfigW(service.0, None, 0, &mut needed) };
    let mut buf = vec![0u64; (needed as usize).div_ceil(8).max(1)];
    let config = buf.as_mut_ptr().cast::<QUERY_SERVICE_CONFIGW>();
    // SAFETY: `buf` is 8-byte aligned and at least `needed` bytes long.
    unsafe { QueryServiceConfigW(service.0, Some(config), needed, &mut needed) }.ok()?;
    // SAFETY: filled by the call above; its strings point into `buf`, which is still alive.
    unsafe {
        Some(SvcConfig {
            image_path: (*config).lpBinaryPathName.to_string().ok()?,
            auto_start: (*config).dwStartType == SERVICE_AUTO_START,
        })
    }
}

/// Name and display name of every running Win32 service: what a bare `sc query` lists.
pub fn active_services() -> Vec<(String, String)> {
    let Some(scm) = manager() else { return Vec::new() };
    let (mut needed, mut count) = (0u32, 0u32);
    // SAFETY: a size query with no buffer.
    let _ = unsafe {
        EnumServicesStatusExW(scm.0, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_ACTIVE, None, &mut needed, &mut count, None, PCWSTR::null())
    };
    let mut buf = vec![0u8; needed as usize + 4096];
    // SAFETY: `buf` is larger than the size asked for.
    let listed = unsafe {
        EnumServicesStatusExW(scm.0, SC_ENUM_PROCESS_INFO, SERVICE_WIN32, SERVICE_ACTIVE, Some(&mut buf), &mut needed, &mut count, None, PCWSTR::null())
    };
    if listed.is_err() {
        return Vec::new();
    }
    let item = size_of::<ENUM_SERVICE_STATUS_PROCESSW>();
    (0..count as usize)
        .map(|i| {
            // SAFETY: the call wrote `count` entries at the start of `buf`; their strings point into it.
            unsafe {
                let entry = std::ptr::read_unaligned(buf.as_ptr().add(i * item).cast::<ENUM_SERVICE_STATUS_PROCESSW>());
                (entry.lpServiceName.to_string().unwrap_or_default(), entry.lpDisplayName.to_string().unwrap_or_default())
            }
        })
        .collect()
}

/// The strategy name `service.bat` records next to the service.
pub fn strategy() -> Option<String> {
    windows_registry::LOCAL_MACHINE.open(SERVICE_KEY).ok()?.get_string(STRATEGY_VALUE).ok()
}

pub fn set_strategy(name: &str) -> anyhow::Result<()> {
    windows_registry::LOCAL_MACHINE.create(SERVICE_KEY)?.set_string(STRATEGY_VALUE, name)?;
    log::info!(target: "exec", r"registry: HKLM\{SERVICE_KEY} {STRATEGY_VALUE} = {name}");
    Ok(())
}

fn cmd(program: &str, args: &[&str]) -> anyhow::Result<exec::Output> {
    Ok(exec::run(Command::new(program).args(args), Duration::from_secs(60), None)?)
}

fn wait_until(max: Duration, mut done: impl FnMut() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < max {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    done()
}

/// `sc create zapret binPath= ... DisplayName= "zapret" start= auto|demand` and the description.
pub fn create(bin_path: &str, auto_start: bool) -> anyhow::Result<()> {
    let start = if auto_start { "auto" } else { "demand" };
    cmd("sc", &["create", SERVICE, "binPath=", bin_path, "DisplayName=", "zapret", "start=", start])?.check()?;
    let _ = cmd("sc", &["description", SERVICE, "Zapret DPI bypass software"]);
    Ok(())
}

/// `:service_install` after the menu: timestamps on, the old service gone, the new one created
/// and started, the strategy name recorded.
pub fn install(folder: &Folder, name: &str, parsed: &ParsedArgs) -> anyhow::Result<()> {
    let _ = enable_tcp_timestamps();
    stop_and_delete(SERVICE)?;
    create(&parsed.bin_path(&folder.winws()), true)?;
    cmd("sc", &["start", SERVICE])?.check()?;
    set_strategy(name)
}

/// `net stop` + `sc delete` with errors ignored, as `service.bat` does where it does not care.
pub fn request_delete(name: &str) {
    let _ = cmd("net", &["stop", name]);
    let _ = cmd("sc", &["delete", name]);
}

/// `request_delete`, then waits until the SCM forgets the service.
pub fn stop_and_delete(name: &str) -> anyhow::Result<()> {
    if state(name) == SvcState::NotInstalled {
        return Ok(());
    }
    request_delete(name);
    anyhow::ensure!(
        wait_until(Duration::from_secs(10), || state(name) == SvcState::NotInstalled),
        "service {name} is still there after sc delete (marked for deletion?): close services.msc and try again"
    );
    Ok(())
}

/// `:service_remove`: zapret, every winws, WinDivert, WinDivert14.
pub fn remove() -> anyhow::Result<()> {
    stop_and_delete(SERVICE)?;
    crate::procs::kill_all("winws.exe")?;
    stop_and_delete("WinDivert")?;
    stop_and_delete("WinDivert14")?;
    Ok(())
}

pub fn start(name: &str) -> anyhow::Result<()> {
    if state(name) != SvcState::Running {
        cmd("net", &["start", name])?.check()?;
    }
    Ok(())
}

pub fn stop(name: &str) -> anyhow::Result<()> {
    if matches!(state(name), SvcState::Running | SvcState::StartPending | SvcState::Other) {
        cmd("net", &["stop", name])?.check()?;
    }
    Ok(())
}

pub fn restart(name: &str) -> anyhow::Result<()> {
    anyhow::ensure!(state(name) != SvcState::NotInstalled, "service {name} is not installed");
    stop(name)?;
    start(name)
}

/// After a list or ipset change: winws reads its files at start.
pub fn restart_if_running(name: &str) -> anyhow::Result<()> {
    if state(name) == SvcState::Running { restart(name) } else { Ok(()) }
}

/// The arguments live in the service: after a game filter change the active strategy is
/// installed again.
pub fn reinstall_active(folder: &Folder) -> anyhow::Result<()> {
    if state(SERVICE) == SvcState::NotInstalled {
        return Ok(());
    }
    let Some(name) = strategy() else { return Ok(()) };
    let file = crate::strategies::find(folder, &name)?;
    install(folder, &file.name, &crate::batparse::parse_file(folder, &file.path)?)
}

/// `netsh interface tcp show global`, the `RFC 1323 Timestamps` line. Labels are translated, the
/// RFC number and the value are not. `None` when the line is missing.
pub fn tcp_timestamps() -> Option<bool> {
    parse_timestamps(&cmd("netsh", &["interface", "tcp", "show", "global"]).ok()?.stdout)
}

pub fn parse_timestamps(text: &str) -> Option<bool> {
    let line = text.lines().find(|line| line.contains("1323"))?;
    Some(line.rsplit(':').next()?.trim().eq_ignore_ascii_case("enabled"))
}

pub fn enable_tcp_timestamps() -> anyhow::Result<()> {
    cmd("netsh", &["interface", "tcp", "set", "global", "timestamps=enabled"])?.check()?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct QuickItem {
    pub name: String,
    pub status: CheckStatus,
    pub time_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct QuickCheck {
    pub ok: bool,
    pub service: SvcState,
    pub winws_running: bool,
    pub checks: Vec<QuickItem>,
}

/// 5.4: winws runs and the first two URL targets answer `HTTP:OK`.
pub fn quick_check(folder: &Folder) -> QuickCheck {
    let service = state(SERVICE);
    let winws_running = crate::procs::is_running("winws.exe");
    let checks: Vec<QuickItem> = crate::targets::load(folder)
        .into_iter()
        .filter_map(|target| Some((target.name, target.url?)))
        .take(2)
        .map(|(name, url)| {
            let result = probe::curl::check(&url, CheckKind::Http, None);
            QuickItem { name, status: result.status, time_ms: result.time_ms }
        })
        .collect();
    let ok = winws_running && !checks.is_empty() && checks.iter().all(|c| c.status == CheckStatus::Ok);
    QuickCheck { ok, service, winws_running, checks }
}

/// «Применить»: install `name` as the service, wait 2 s, run the quick check.
pub fn apply(folder: &Folder, name: &str) -> anyhow::Result<QuickCheck> {
    let file = crate::strategies::find(folder, name)?;
    let parsed = crate::batparse::parse_file(folder, &file.path)?;
    install(folder, &file.name, &parsed)?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(quick_check(folder))
}
```

В конец `crates/zapret/src/folder.rs`:

```rust
/// Where a Flowseal folder probably is: the `zapret` service's binary, then any running winws.
pub fn candidates() -> Vec<PathBuf> {
    let from_service = crate::service::config(crate::service::SERVICE).and_then(|c| root_from_image_path(&c.image_path));
    let from_processes = crate::procs::find("winws.exe")
        .into_iter()
        .filter_map(|p| p.exe.as_deref().and_then(Path::parent).and_then(Path::parent).map(Path::to_path_buf));
    let mut roots: Vec<PathBuf> = Vec::new();
    for root in from_service.into_iter().chain(from_processes) {
        if !roots.contains(&root) {
            roots.push(root);
        }
    }
    roots
}
```

И `pub mod procs;` и `pub mod service;` в `crates/zapret/src/lib.rs`.

- [ ] **Шаг 5: Тесты проходят**

Run: `cargo test -p zapret --test service`
Expected: PASS, 3 теста (живой тест пропущен).

Run (из повышенного терминала): `cargo test -p zapret --test service -- --ignored --nocapture`
Expected: PASS; печатает состояние службы `zapret`, её `binPath`, имя стратегии, значение TCP timestamps и `E:\Zapret` среди кандидатов.

- [ ] **Шаг 6: Правка спецификации**

В `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md`, раздел 10, после пункта «У каждого внешнего процесса таймаут; зависший процесс завершается, это записывается как ошибка.» добавить пункт:

```
- Исключение — долгоживущий winws: тестовый (его запускает, останавливает и читает `procs::Winws`)
  и ручной winws, возвращаемый из снимка. Таймаута у них нет по смыслу; командная строка и pid
  пишутся в журнал при запуске.
```

- [ ] **Шаг 7: Коммит**

```bash
git add crates/zapret docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md
git commit -m "feat(zapret): manage the zapret service and winws processes"
```

---

### Задача 7: `zapret` — обновления Flowseal и блок в hosts

**Files:**
- Create: `crates/zapret/src/hosts.rs`, `crates/zapret/src/updates.rs`
- Modify: `crates/zapret/src/lib.rs` (`pub mod hosts; pub mod updates;`)
- Test: `crates/zapret/tests/hosts.rs`

**Interfaces:**
- Consumes: `probe::curl::{fetch_text, download}`, `ipset::install`, `Folder::version`
- Produces:
  - `hosts::{BEGIN, END}`, `hosts::path() -> PathBuf`, `hosts::read() -> anyhow::Result<String>`, `needs_update(current, flowseal) -> bool`, `merge(current, block) -> String`, `HostsPlan { needs_update: bool, block: String, added: Vec<String>, removed: Vec<String> }`, `plan(current, flowseal) -> HostsPlan`, `apply(block) -> anyhow::Result<()>`
  - `updates::{version_url(), ipset_url(), hosts_url(), RELEASE_URL}`, `latest_version(socks5) -> anyhow::Result<String>`, `newer_version(&Folder, socks5) -> anyhow::Result<Option<String>>`, `update_ipset(&Folder, socks5) -> anyhow::Result<()>`, `hosts_plan(socks5) -> anyhow::Result<HostsPlan>`, `apply_hosts(socks5) -> anyhow::Result<()>`

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret/tests/hosts.rs`:

```rust
use zapret::hosts::{merge, needs_update, plan};

#[test]
fn needs_update_compares_the_first_and_the_last_line() {
    let flowseal = "1.2.3.4 a.com\r\n\r\n5.6.7.8 b.com\r\n";
    assert!(needs_update("127.0.0.1 localhost\r\n", flowseal));
    assert!(needs_update("1.2.3.4 a.com\r\n", flowseal));
    assert!(!needs_update("x\r\n1.2.3.4 a.com\r\n5.6.7.8 b.com\r\n", flowseal));
}

#[test]
fn merge_replaces_only_its_own_block() {
    let first = merge("127.0.0.1 localhost\r\n", "1.2.3.4 a.com");
    assert_eq!(first, "127.0.0.1 localhost\r\n\r\n# zapret-hub begin\r\n1.2.3.4 a.com\r\n# zapret-hub end\r\n");
    let second = merge(&first, "5.6.7.8 b.com\r\n");
    assert_eq!(second, "127.0.0.1 localhost\r\n\r\n# zapret-hub begin\r\n5.6.7.8 b.com\r\n# zapret-hub end\r\n");
    assert_eq!(merge("", "x"), "# zapret-hub begin\r\nx\r\n# zapret-hub end\r\n");
}

#[test]
fn plan_lists_what_comes_and_what_goes() {
    let current = merge("x\r\n", "1.1.1.1 old.com\r\n2.2.2.2 keep.com");
    let p = plan(&current, "2.2.2.2 keep.com\r\n3.3.3.3 new.com\r\n");
    assert!(p.needs_update);
    assert_eq!(p.added, ["3.3.3.3 new.com"]);
    assert_eq!(p.removed, ["1.1.1.1 old.com"]);
}

#[test]
#[ignore = "needs the network"]
fn live_flowseal_version_is_readable() {
    let version = zapret::updates::latest_version(None).unwrap();
    assert!(version.chars().next().is_some_and(|c| c.is_ascii_digit()), "{version}");
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test hosts`
Expected: FAIL при компиляции, модуля `hosts` нет.

- [ ] **Шаг 3: Реализация**

`crates/zapret/src/hosts.rs`:

```rust
//! The `zapret-hub` block in the hosts file and when it needs refreshing (`:hosts_update`).

use std::path::PathBuf;

pub const BEGIN: &str = "# zapret-hub begin";
pub const END: &str = "# zapret-hub end";

pub fn path() -> PathBuf {
    let windows = std::env::var_os("SystemRoot").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    windows.join("System32").join("drivers").join("etc").join("hosts")
}

/// Up to date when the first and the last non-empty line of Flowseal's file are in hosts.
pub fn needs_update(current: &str, flowseal: &str) -> bool {
    let lines: Vec<&str> = flowseal.lines().filter(|line| !line.trim().is_empty()).collect();
    let (Some(first), Some(last)) = (lines.first(), lines.last()) else { return false };
    !current.contains(first) || !current.contains(last)
}

/// `current` with its zapret-hub block replaced by `block`, or the block appended.
pub fn merge(current: &str, block: &str) -> String {
    let mut kept: Vec<&str> = Vec::new();
    let mut inside = false;
    for line in current.lines() {
        match line.trim() {
            BEGIN => inside = true,
            END => inside = false,
            _ if !inside => kept.push(line),
            _ => {}
        }
    }
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    let mut out = kept.join("\r\n");
    if !out.is_empty() {
        out.push_str("\r\n\r\n");
    }
    out.push_str(BEGIN);
    out.push_str("\r\n");
    for line in block.lines() {
        out.push_str(line);
        out.push_str("\r\n");
    }
    out.push_str(END);
    out.push_str("\r\n");
    out
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct HostsPlan {
    pub needs_update: bool,
    pub block: String,
    /// Lines of Flowseal's file that hosts does not have yet.
    pub added: Vec<String>,
    /// Lines of the current zapret-hub block that Flowseal no longer has.
    pub removed: Vec<String>,
}

pub fn plan(current: &str, flowseal: &str) -> HostsPlan {
    let mut inside = false;
    let mut old_block: Vec<&str> = Vec::new();
    for line in current.lines() {
        match line.trim() {
            BEGIN => inside = true,
            END => inside = false,
            _ if inside => old_block.push(line),
            _ => {}
        }
    }
    let added = flowseal
        .lines()
        .filter(|line| !line.trim().is_empty() && !current.lines().any(|c| c == *line))
        .map(str::to_owned)
        .collect();
    let removed = old_block
        .into_iter()
        .filter(|line| !line.trim().is_empty() && !flowseal.lines().any(|f| f == *line))
        .map(str::to_owned)
        .collect();
    HostsPlan { needs_update: needs_update(current, flowseal), block: flowseal.to_owned(), added, removed }
}

/// The hosts file; a missing one is empty. A file that is not UTF-8 is refused, not rewritten:
/// a lossy round trip would mangle its comments.
pub fn read() -> anyhow::Result<String> {
    let path = path();
    match std::fs::read(&path) {
        Ok(bytes) => String::from_utf8(bytes)
            .map_err(|_| anyhow::anyhow!("{} is not UTF-8 text: edit it by hand, nothing was changed", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(anyhow::Error::new(err).context(path.display().to_string())),
    }
}

/// Writes `block` into hosts; the previous file is kept as `hosts.zapret-hub.bak`.
pub fn apply(block: &str) -> anyhow::Result<()> {
    let path = path();
    let current = read()?;
    if path.is_file() {
        std::fs::copy(&path, path.with_file_name("hosts.zapret-hub.bak"))?;
    }
    std::fs::write(&path, merge(&current, block))?;
    log::info!(target: "exec", "hosts: zapret-hub block written, {} lines", block.lines().count());
    let _ = exec::run(std::process::Command::new("ipconfig").arg("/flushdns"), std::time::Duration::from_secs(15), None);
    Ok(())
}
```

`crates/zapret/src/updates.rs`:

```rust
//! Flowseal's update endpoints: the version file, the ipset list, the hosts file (5.6).

use crate::folder::Folder;
use crate::hosts::HostsPlan;

const RAW: &str = "https://raw.githubusercontent.com/Flowseal/zapret-discord-youtube";
/// The release page of a version: `RELEASE_URL` + version.
pub const RELEASE_URL: &str = "https://github.com/Flowseal/zapret-discord-youtube/releases/tag/";

pub fn version_url() -> String {
    format!("{RAW}/main/.service/version.txt")
}

pub fn ipset_url() -> String {
    format!("{RAW}/refs/heads/main/.service/ipset-service.txt")
}

pub fn hosts_url() -> String {
    format!("{RAW}/refs/heads/main/.service/hosts")
}

/// The latest released version, as `:service_check_updates` reads it.
pub fn latest_version(socks5: Option<&str>) -> anyhow::Result<String> {
    Ok(probe::curl::fetch_text(&version_url(), socks5)?.trim().to_owned())
}

/// `Some(latest)` when it differs from the folder's version: `service.bat` compares the strings.
pub fn newer_version(folder: &Folder, socks5: Option<&str>) -> anyhow::Result<Option<String>> {
    let latest = latest_version(socks5)?;
    Ok((folder.version().as_deref() != Some(latest.as_str())).then_some(latest))
}

/// Downloads `ipset-service.txt` next to the list, then swaps it in.
pub fn update_ipset(folder: &Folder, socks5: Option<&str>) -> anyhow::Result<()> {
    let download = folder.lists().join("ipset-all.txt.download");
    let result = probe::curl::download(&ipset_url(), &download, socks5).and_then(|()| crate::ipset::install(folder, &download));
    let _ = std::fs::remove_file(&download);
    result
}

fn flowseal_hosts(socks5: Option<&str>) -> anyhow::Result<String> {
    let nonce = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    probe::curl::fetch_text(&format!("{}?t={nonce}", hosts_url()), socks5)
}

/// What applying Flowseal's hosts would change.
pub fn hosts_plan(socks5: Option<&str>) -> anyhow::Result<HostsPlan> {
    let flowseal = flowseal_hosts(socks5)?;
    Ok(crate::hosts::plan(&crate::hosts::read()?, &flowseal))
}

/// Downloads Flowseal's hosts again and writes it as the zapret-hub block.
pub fn apply_hosts(socks5: Option<&str>) -> anyhow::Result<()> {
    crate::hosts::apply(&flowseal_hosts(socks5)?)
}
```

- [ ] **Шаг 4: Тесты проходят**

Run: `cargo test -p zapret --test hosts`
Expected: PASS, 3 теста.

Run: `cargo test -p zapret --test hosts -- --ignored`
Expected: PASS при работающей сети.

- [ ] **Шаг 5: Коммит**

```bash
git add crates/zapret
git commit -m "feat(zapret): check Flowseal updates, refresh ipset and the hosts block"
```

---

### Задача 8: `zapret` — системные проверки и очистка кэша Discord

**Files:**
- Create: `crates/zapret/src/checks.rs`, `crates/zapret/src/discord_cache.rs`
- Modify: `crates/zapret/src/lib.rs` (`pub mod checks; pub mod discord_cache;`)
- Test: `crates/zapret/tests/checks.rs`

**Interfaces:**
- Consumes: `service::{active_services, state, tcp_timestamps, enable_tcp_timestamps, stop_and_delete, request_delete, SvcState}`, `procs::{is_running, kill_all}`, `hosts::read`, `Folder`
- Produces:
  - `checks::CheckLevel { Ok, Warn, Problem }`, `CheckId { Bfe, Proxy, TcpTimestamps, Adguard, Killer, IntelConnectivity, CheckPoint, SmartByte, CyrillicPath, OneDrive, WinDivertSys, Vpn, SecureDns, HostsYoutube, WinDivertOrphan, Conflicts }`, `CheckAction { EnableTcpTimestamps, RemoveWinDivert, RemoveConflicts }`, `SystemCheck { id, level, detail: String, action: Option<CheckAction> }`, `CONFLICTING`
  - `checks::run_all(&Folder) -> Vec<SystemCheck>`, `checks::fix(CheckAction) -> anyhow::Result<()>`, `matching_services(&[(String, String)], &[&str]) -> Vec<String>`, `has_cyrillic(&str) -> bool`, `in_onedrive(root_dp0: &str, onedrive: Option<&str>) -> bool`
  - `discord_cache::{INSTALLS, CacheOutcome { Deleted, Missing, Failed }, CacheDir { path, outcome, error: Option<String> }, CacheReport { install, closed: bool, dirs }}`, `clear() -> anyhow::Result<Vec<CacheReport>>`, `clear_in(appdata: &Path, close: impl FnMut(&str) -> bool) -> Vec<CacheReport>`
  - Ссылки на issues из оригинала (Adguard, Killer, Intel) живут во фронтенде, в `strings.ts` (задача 20).

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret/tests/checks.rs`:

```rust
use zapret::checks::{has_cyrillic, in_onedrive, matching_services};
use zapret::discord_cache::{CacheOutcome, clear_in};

#[test]
fn a_service_matches_when_one_line_has_every_word() {
    let services = vec![
        ("ICNS".to_owned(), "Intel(R) Connectivity Network Service".to_owned()),
        ("NetSvc".to_owned(), "Intel Connectivity Helper".to_owned()),
        ("KillerNetworkService".to_owned(), "Killer Network Service".to_owned()),
    ];
    assert_eq!(matching_services(&services, &["intel", "connectivity", "network"]), ["ICNS"]);
    assert_eq!(matching_services(&services, &["killer"]), ["KillerNetworkService"]);
    assert!(matching_services(&services, &["vpn"]).is_empty());
}

#[test]
fn cyrillic_and_onedrive_paths_are_spotted() {
    assert!(has_cyrillic(r"C:\Запрет\"));
    assert!(has_cyrillic("ёлка"));
    assert!(!has_cyrillic(r"E:\Zapret\"));
    assert!(in_onedrive(r"C:\Users\me\OneDrive\zapret\", Some(r"C:\Users\me\OneDrive")));
    assert!(in_onedrive(r"c:\users\me\onedrive\", Some(r"C:\Users\me\OneDrive\")));
    assert!(!in_onedrive(r"C:\Users\me\OneDriveBackup\z\", Some(r"C:\Users\me\OneDrive")));
    assert!(!in_onedrive(r"E:\Zapret\", None));
}

#[test]
fn discord_cache_is_cleared_for_installed_builds_only() {
    let appdata = tempfile::tempdir().unwrap();
    let discord = appdata.path().join("discord");
    std::fs::create_dir_all(discord.join("Cache").join("Cache_Data")).unwrap();
    std::fs::write(discord.join("Cache").join("Cache_Data").join("f_000001"), "x").unwrap();
    std::fs::create_dir_all(discord.join("GPUCache")).unwrap();
    let mut asked = Vec::new();
    let reports = clear_in(appdata.path(), |image| {
        asked.push(image.to_owned());
        false
    });
    assert_eq!(asked, ["Discord.exe"]);
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].install, "Discord");
    let outcomes: Vec<CacheOutcome> = reports[0].dirs.iter().map(|d| d.outcome).collect();
    assert_eq!(outcomes, [CacheOutcome::Deleted, CacheOutcome::Missing, CacheOutcome::Deleted]);
    assert!(!discord.join("Cache").exists());
}

#[test]
#[ignore = "reads this machine's services, registry and E:\\Zapret"]
fn live_checks_of_this_machine() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let checks = zapret::checks::run_all(&folder);
    for check in &checks {
        eprintln!("{check:?}");
    }
    // Every diagnostic of service.bat, once each.
    let ids: Vec<&zapret::checks::CheckId> = checks.iter().map(|check| &check.id).collect();
    assert_eq!(ids.len(), 16, "{ids:?}");
    for (i, id) in ids.iter().enumerate() {
        assert!(!ids[i + 1..].contains(id), "{id:?} twice");
    }
    // zapret works on this machine, so the filtering engine runs.
    assert!(checks.iter().any(|c| c.id == zapret::checks::CheckId::Bfe && c.level == zapret::checks::CheckLevel::Ok));
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test checks`
Expected: FAIL при компиляции, модулей `checks` и `discord_cache` нет.

- [ ] **Шаг 3: Реализация**

`crates/zapret/src/checks.rs`:

```rust
//! The checks of `service.bat :service_diagnostics` (spec 5.7). Each gets a level; what the
//! original fixes on its own becomes an action behind a button.

use crate::folder::Folder;
use crate::service::{self, SvcState};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckLevel {
    Ok,
    Warn,
    Problem,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckId {
    Bfe,
    Proxy,
    TcpTimestamps,
    Adguard,
    Killer,
    IntelConnectivity,
    CheckPoint,
    SmartByte,
    CyrillicPath,
    OneDrive,
    WinDivertSys,
    Vpn,
    SecureDns,
    HostsYoutube,
    WinDivertOrphan,
    Conflicts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CheckAction {
    EnableTcpTimestamps,
    RemoveWinDivert,
    RemoveConflicts,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct SystemCheck {
    pub id: CheckId,
    pub level: CheckLevel,
    /// What the message names: the proxy address, service names, the folder. Empty when passed.
    pub detail: String,
    /// The fix offered next to a failed check.
    pub action: Option<CheckAction>,
}

/// Other bypasses `service.bat` offers to remove.
pub const CONFLICTING: [&str; 4] = ["GoodbyeDPI", "discordfix_zapret", "winws1", "winws2"];

pub fn run_all(folder: &Folder) -> Vec<SystemCheck> {
    use CheckLevel::{Problem, Warn};
    let services = service::active_services();
    let find = |words: &[&str]| matching_services(&services, words).join(", ");
    let root = folder.root_dp0();
    let proxy = proxy_server();
    let hosts = crate::hosts::read().unwrap_or_default().to_lowercase();
    let windivert = service::state("WinDivert");
    let orphan =
        matches!(windivert, SvcState::Running | SvcState::StopPending) && !crate::procs::is_running("winws.exe");
    let conflicts: Vec<&str> =
        CONFLICTING.into_iter().filter(|name| service::state(name) != SvcState::NotInstalled).collect();
    let checkpoint: Vec<String> =
        [find(&["tracsrvwrapper"]), find(&["epwd"])].into_iter().filter(|names| !names.is_empty()).collect();
    vec![
        check(CheckId::Bfe, service::state("BFE") != SvcState::Running, Problem, String::new(), None),
        check(CheckId::Proxy, proxy.is_some(), Warn, proxy.unwrap_or_default(), None),
        check(
            CheckId::TcpTimestamps,
            service::tcp_timestamps() != Some(true),
            Warn,
            String::new(),
            Some(CheckAction::EnableTcpTimestamps),
        ),
        check(CheckId::Adguard, crate::procs::is_running("AdguardSvc.exe"), Problem, String::new(), None),
        found(CheckId::Killer, find(&["killer"]), Problem),
        found(CheckId::IntelConnectivity, find(&["intel", "connectivity", "network"]), Problem),
        found(CheckId::CheckPoint, checkpoint.join(", "), Problem),
        found(CheckId::SmartByte, find(&["smartbyte"]), Problem),
        check(CheckId::CyrillicPath, has_cyrillic(&root), Warn, root.clone(), None),
        check(CheckId::OneDrive, in_onedrive(&root, std::env::var("OneDrive").ok().as_deref()), Problem, root.clone(), None),
        check(CheckId::WinDivertSys, !has_driver(folder), Problem, folder.bin().display().to_string(), None),
        found(CheckId::Vpn, find(&["vpn"]), Warn),
        check(CheckId::SecureDns, !doh_configured(), Warn, String::new(), None),
        check(
            CheckId::HostsYoutube,
            hosts.contains("youtube.com") || hosts.contains("youtu.be"),
            Warn,
            String::new(),
            None,
        ),
        check(CheckId::WinDivertOrphan, orphan, Warn, format!("{windivert:?}"), Some(CheckAction::RemoveWinDivert)),
        check(CheckId::Conflicts, !conflicts.is_empty(), Problem, conflicts.join(", "), Some(CheckAction::RemoveConflicts)),
    ]
}

fn check(id: CheckId, failed: bool, level: CheckLevel, detail: String, action: Option<CheckAction>) -> SystemCheck {
    if failed {
        SystemCheck { id, level, detail, action }
    } else {
        SystemCheck { id, level: CheckLevel::Ok, detail: String::new(), action: None }
    }
}

fn found(id: CheckId, names: String, level: CheckLevel) -> SystemCheck {
    check(id, !names.is_empty(), level, names, None)
}

/// The button next to a failed check: what `service.bat` does for it.
pub fn fix(action: CheckAction) -> anyhow::Result<()> {
    match action {
        CheckAction::EnableTcpTimestamps => service::enable_tcp_timestamps(),
        CheckAction::RemoveWinDivert => service::stop_and_delete("WinDivert").or_else(|first| {
            // A GoodbyeDPI service keeps WinDivert loaded: it goes, then WinDivert once more.
            if service::state("GoodbyeDPI") == SvcState::NotInstalled {
                return Err(first);
            }
            service::stop_and_delete("GoodbyeDPI")?;
            service::stop_and_delete("WinDivert")
        }),
        CheckAction::RemoveConflicts => {
            for name in CONFLICTING {
                service::stop_and_delete(name)?;
            }
            // As in `service.bat`: a running winws keeps WinDivert until it stops.
            service::request_delete("WinDivert");
            service::request_delete("WinDivert14");
            Ok(())
        }
    }
}

/// Names of the services whose name or display name holds every word: `sc query | findstr`
/// matches one line at a time.
pub fn matching_services(services: &[(String, String)], words: &[&str]) -> Vec<String> {
    services
        .iter()
        .filter(|(name, display)| {
            [name, display].iter().any(|text| {
                let text = text.to_lowercase();
                words.iter().all(|word| text.contains(word))
            })
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// `[\u0430-\u044F\u0410-\u042F\u0451\u0401]`
pub fn has_cyrillic(text: &str) -> bool {
    text.chars().any(|c| matches!(c, 'а'..='я' | 'А'..='Я' | 'ё' | 'Ё'))
}

/// `echo %~dp0\ | findstr /I /C:"%OneDrive%\\"`
pub fn in_onedrive(root_dp0: &str, onedrive: Option<&str>) -> bool {
    onedrive.is_some_and(|dir| {
        let dir = dir.trim_end_matches('\\');
        !dir.is_empty() && root_dp0.to_lowercase().contains(&format!("{dir}\\").to_lowercase())
    })
}

fn has_driver(folder: &Folder) -> bool {
    std::fs::read_dir(folder.bin()).is_ok_and(|entries| {
        entries.flatten().any(|entry| entry.path().extension().is_some_and(|ext| ext.eq_ignore_ascii_case("sys")))
    })
}

/// `ProxyServer` when `ProxyEnable` is 1 in the user's Internet Settings.
fn proxy_server() -> Option<String> {
    let key = windows_registry::CURRENT_USER.open(r"Software\Microsoft\Windows\CurrentVersion\Internet Settings").ok()?;
    (key.get_u32("ProxyEnable").ok()? == 1).then(|| key.get_string("ProxyServer").unwrap_or_default())
}

/// A `DohFlags` above zero anywhere under the DNS client's per-interface settings.
fn doh_configured() -> bool {
    fn search(key: &windows_registry::Key, depth: u8) -> bool {
        key.get_u64("DohFlags").is_ok_and(|flags| flags > 0)
            || (depth > 0
                && key.keys().is_ok_and(|mut names| {
                    names.any(|name| key.open(&name).is_ok_and(|sub| search(&sub, depth - 1)))
                }))
    }
    windows_registry::LOCAL_MACHINE
        .open(r"SYSTEM\CurrentControlSet\Services\Dnscache\InterfaceSpecificParameters")
        .is_ok_and(|key| search(&key, 6))
}
```

`crates/zapret/src/discord_cache.rs`:

```rust
//! `:clear_discord_cache`: close each Discord build, delete `Cache`, `Code Cache`, `GPUCache`.

use std::path::Path;
use std::time::Duration;

/// Process image, the name the report shows, the folder under `%APPDATA%`.
pub const INSTALLS: [(&str, &str, &str); 4] = [
    ("Discord.exe", "Discord", "discord"),
    ("DiscordPTB.exe", "Discord PTB", "discordptb"),
    ("DiscordCanary.exe", "Discord Canary", "discordcanary"),
    ("DiscordDevelopment.exe", "Discord Development", "discorddevelopment"),
];

const CACHE_DIRS: [&str; 3] = ["Cache", "Code Cache", "GPUCache"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum CacheOutcome {
    Deleted,
    Missing,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CacheDir {
    pub path: String,
    pub outcome: CacheOutcome,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CacheReport {
    pub install: String,
    /// It was running and was closed.
    pub closed: bool,
    pub dirs: Vec<CacheDir>,
}

/// Every build installed under `%APPDATA%`; empty when none is.
pub fn clear() -> anyhow::Result<Vec<CacheReport>> {
    let appdata = std::env::var_os("APPDATA").ok_or_else(|| anyhow::anyhow!("APPDATA is not set"))?;
    Ok(clear_in(Path::new(&appdata), |image| {
        let closed = crate::procs::is_running(image) && crate::procs::kill_all(image).is_ok();
        if closed {
            std::thread::sleep(Duration::from_secs(1)); // let it release its files
        }
        closed
    }))
}

/// `close(image)` stops a running build and says whether it did.
pub fn clear_in(appdata: &Path, mut close: impl FnMut(&str) -> bool) -> Vec<CacheReport> {
    INSTALLS
        .iter()
        .filter(|(_, _, dir)| appdata.join(dir).is_dir())
        .map(|&(image, name, dir)| {
            let closed = close(image);
            let dirs = CACHE_DIRS
                .iter()
                .map(|sub| {
                    let path = appdata.join(dir).join(sub);
                    let (outcome, error) = if !path.is_dir() {
                        (CacheOutcome::Missing, None)
                    } else {
                        match std::fs::remove_dir_all(&path) {
                            Ok(()) => (CacheOutcome::Deleted, None),
                            Err(err) => (CacheOutcome::Failed, Some(err.to_string())),
                        }
                    };
                    CacheDir { path: path.display().to_string(), outcome, error }
                })
                .collect();
            log::info!("discord cache: {name} cleared");
            CacheReport { install: name.to_owned(), closed, dirs }
        })
        .collect()
}
```

- [ ] **Шаг 4: Тесты проходят**

Run: `cargo test -p zapret --test checks`
Expected: PASS, 3 теста.

Run: `cargo test -p zapret --test checks -- --ignored --nocapture`
Expected: PASS; печатает 16 проверок, каждую по разу; `Bfe` — `Ok`, `Proxy` — по состоянию системного прокси.

- [ ] **Шаг 5: Коммит**

```bash
git add crates/zapret
git commit -m "feat(zapret): run service.bat diagnostics and clear the Discord cache"
```

---

### Задача 9: `zapret` — свои стратегии

**Files:**
- Create: `crates/zapret/src/custom.rs`
- Modify: `crates/zapret/src/lib.rs` (`pub mod custom;`), `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md` (раздел 14)
- Test: `crates/zapret/tests/custom.rs`

**Interfaces:**
- Consumes: `batparse::{find_ci, parse, ParsedArgs}`, `game_filter::read`, `strategies::find`, `procs::Winws`, `exec::run`, фикстуры задачи 4
- Produces:
  - `custom::CustomStrategy { name, cloned_from, wf: String, blocks: Vec<String> }` (serde, specta): `.file_stem() -> "general (HUB <name>)"`, `.to_bat() -> String`, `.normalized() -> anyhow::Result<CustomStrategy>`
  - `custom::split_bat(&str) -> anyhow::Result<(String, Vec<String>)>`, `validate_name(&str) -> anyhow::Result<String>`
  - `custom::dry_run(&Folder, &ParsedArgs) -> anyhow::Result<String>`, `validate(&Folder, &CustomStrategy) -> anyhow::Result<(CustomStrategy, ParsedArgs)>`
  - `custom::Store::new(dir)`, `.list() -> anyhow::Result<Vec<CustomStrategy>>`, `.get(name)`, `.save(&CustomStrategy)`, `.remove(name)`
  - `custom::save(&Folder, &Store, &CustomStrategy, new: bool) -> anyhow::Result<CustomStrategy>`, `clone_from(&Folder, &Store, source, name) -> anyhow::Result<CustomStrategy>`, `delete(&Folder, &Store, name, active: Option<&str>) -> anyhow::Result<()>`, `write_bat(&Folder, &CustomStrategy)`, `materialize_all(&Folder, &Store) -> anyhow::Result<Vec<String>>`

Все 22 стратегии Flowseal 1.10.3 устроены одинаково: первая строка команды содержит только `--wf-*`, дальше по одному блоку на строку, каждая строка кроме последней кончается на ` --new ^`. Поэтому сгенерированный `.bat` разбирается в те же `Final args` байт в байт — это проверяет первый тест.

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret/tests/custom.rs`:

```rust
mod common;

use std::path::Path;

use zapret::batparse::{GameFilterVars, parse};
use zapret::custom::{CustomStrategy, Store, split_bat, validate_name};

fn sample() -> CustomStrategy {
    CustomStrategy {
        name: "Test 1".into(),
        cloned_from: "general (ALT11)".into(),
        wf: "--wf-tcp=80,443".into(),
        blocks: vec![
            "--filter-tcp=80 --dpi-desync=fake".into(),
            "--filter-tcp=443 --hostlist=\"%LISTS%list-general.txt\"".into(),
        ],
    }
}

#[test]
fn every_flowseal_strategy_survives_split_and_regenerate() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests").join("fixtures");
    let root = std::fs::read_to_string(dir.join("ROOT.txt")).expect("run tools\\gen-bat-fixtures.cmd first");
    let root = root.trim_end();
    let vars = GameFilterVars { all: "1024-65535".into(), tcp: "1024-65535".into(), udp: "1024-65535".into() };
    let mut compared = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if !path.extension().is_some_and(|e| e.eq_ignore_ascii_case("bat")) {
            continue;
        }
        let bat = zapret::read_text(&path).unwrap();
        let Ok(original) = parse(&bat, root, &vars) else { continue };
        let (wf, blocks) = split_bat(&bat).unwrap();
        let regenerated = CustomStrategy { name: "x".into(), cloned_from: "y".into(), wf, blocks };
        assert_eq!(parse(&regenerated.to_bat(), root, &vars).unwrap(), original, "{}", path.display());
        compared += 1;
    }
    assert!(compared >= 20, "only {compared} strategies compared");
}

#[test]
fn the_generated_file_looks_like_flowseal() {
    assert_eq!(sample().file_stem(), "general (HUB Test 1)");
    let bat = sample().to_bat();
    assert!(bat.starts_with("@echo off\r\nchcp 65001 > nul\r\n"));
    assert!(bat.ends_with(concat!(
        "cd /d %BIN%\r\n\r\n",
        "start \"zapret: %~n0\" /min \"%BIN%winws.exe\" --wf-tcp=80,443 ^\r\n",
        "--filter-tcp=80 --dpi-desync=fake --new ^\r\n",
        "--filter-tcp=443 --hostlist=\"%LISTS%list-general.txt\"\r\n",
    )));
}

#[test]
fn editor_text_becomes_one_line_and_is_checked() {
    let s = CustomStrategy { blocks: vec!["--filter-tcp=80\r\n  --dpi-desync=fake".into(), "  \n".into()], ..sample() };
    assert_eq!(s.normalized().unwrap().blocks, ["--filter-tcp=80 --dpi-desync=fake"]);
    for (wf, block) in [
        ("--wf-tcp=80", "--filter-tcp=80 & calc"),
        ("--wf-tcp=80", "--filter-tcp=80 --new --filter-tcp=443"),
        ("--filter-tcp=80", "--dpi-desync=fake"),
        ("--wf-tcp=80", ""),
    ] {
        let s = CustomStrategy { wf: wf.into(), blocks: vec![block.into()], ..sample() };
        assert!(s.normalized().is_err(), "{wf} / {block}");
    }
}

#[test]
fn names_are_checked() {
    assert_eq!(validate_name("  My strat-1.2_x ").unwrap(), "My strat-1.2_x");
    let long = "x".repeat(41);
    for bad in ["", " ", "-x", "a/b", "привет", "a\"b", long.as_str()] {
        assert!(validate_name(bad).is_err(), "{bad}");
    }
}

#[test]
fn store_keeps_records_and_files_in_step() {
    let zapret_dir = common::fake_zapret();
    let folder = common::folder(&zapret_dir);
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    assert!(store.list().unwrap().is_empty(), "a missing folder is an empty store");

    let s = sample();
    store.save(&s).unwrap();
    zapret::custom::write_bat(&folder, &s).unwrap();
    assert_eq!(store.list().unwrap(), std::slice::from_ref(&s));
    assert_eq!(store.get("TEST 1").unwrap(), s);

    let bat = zapret_dir.path().join("general (HUB Test 1).bat");
    std::fs::remove_file(&bat).unwrap();
    assert_eq!(zapret::custom::materialize_all(&folder, &store).unwrap(), ["Test 1"]);
    assert!(bat.is_file());

    assert!(zapret::custom::delete(&folder, &store, "Test 1", Some("GENERAL (HUB TEST 1)")).is_err());
    zapret::custom::delete(&folder, &store, "Test 1", Some("general (ALT11)")).unwrap();
    assert!(store.list().unwrap().is_empty());
    assert!(!bat.exists());
}

#[test]
fn a_broken_record_names_its_file() {
    let data = tempfile::tempdir().unwrap();
    std::fs::write(data.path().join("bad.toml"), "name = ").unwrap();
    let err = Store::new(data.path()).list().unwrap_err().to_string();
    assert!(err.contains("bad.toml"), "{err}");
}

#[test]
#[ignore = "needs E:\\Zapret and admin rights"]
fn live_clone_of_alt11_passes_winws_dry_run() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let source = zapret::strategies::find(&folder, "general (ALT11)").unwrap();
    let (wf, blocks) = split_bat(&zapret::read_text(&source.path).unwrap()).unwrap();
    let s = CustomStrategy { name: "dry run probe".into(), cloned_from: source.name.clone(), wf, blocks };
    let (_, parsed) = zapret::custom::validate(&folder, &s).unwrap();
    assert_eq!(parsed, zapret::batparse::parse_file(&folder, &source.path).unwrap());
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test custom`
Expected: FAIL при компиляции, модуля `custom` нет.

- [ ] **Шаг 3: Реализация**

`crates/zapret/src/custom.rs`:

```rust
//! Custom strategies (spec 6). A TOML record in `data/strategies/` is the source of truth; a
//! `general (HUB <name>).bat` in Flowseal's format lets `service.bat` and `test zapret.ps1` use it.

use std::os::windows::process::CommandExt as _;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use crate::batparse::ParsedArgs;
use crate::folder::Folder;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct CustomStrategy {
    /// What follows `HUB ` in the file name.
    pub name: String,
    /// The strategy it was cloned from.
    pub cloned_from: String,
    /// `--wf-tcp=... --wf-udp=...`: what winws intercepts at all.
    pub wf: String,
    /// One entry per `--new` block, as the `.bat` has it (`%BIN%`, `%LISTS%`, `%GameFilter…%` kept).
    pub blocks: Vec<String>,
}

/// The lines before the winws command in every Flowseal 1.10.3 strategy.
const HEADER: &str = concat!(
    "@echo off\r\n",
    "chcp 65001 > nul\r\n",
    ":: 65001 - UTF-8\r\n",
    "\r\n",
    "cd /d \"%~dp0\"\r\n",
    "call service.bat status_zapret\r\n",
    "call service.bat check_updates\r\n",
    "call service.bat load_game_filter\r\n",
    "call service.bat load_user_lists\r\n",
    "echo:\r\n",
    "\r\n",
    "set \"BIN=%~dp0bin\\\"\r\n",
    "set \"LISTS=%~dp0lists\\\"\r\n",
    "cd /d %BIN%\r\n",
    "\r\n",
);

impl CustomStrategy {
    /// `general (HUB <name>)`: the strategy name `service.bat` and the registry see.
    pub fn file_stem(&self) -> String {
        format!("general (HUB {})", self.name)
    }

    /// Flowseal's header, then `start ... winws.exe` with one block per line.
    pub fn to_bat(&self) -> String {
        let mut bat = String::from(HEADER);
        bat.push_str("start \"zapret: %~n0\" /min \"%BIN%winws.exe\" ");
        bat.push_str(&self.wf);
        for (i, block) in self.blocks.iter().enumerate() {
            bat.push_str(" ^\r\n");
            bat.push_str(block);
            if i + 1 < self.blocks.len() {
                bat.push_str(" --new");
            }
        }
        bat.push_str("\r\n");
        bat
    }

    /// Editor text made fit for one `.bat` line: line breaks become spaces, empty blocks go,
    /// cmd's special characters and a typed `--new` are refused.
    pub fn normalized(&self) -> anyhow::Result<Self> {
        let wf = one_line(&self.wf)?;
        anyhow::ensure!(
            !wf.is_empty() && wf.split_whitespace().all(|token| token.starts_with("--wf-")),
            "the first line holds only --wf-* options: {wf}"
        );
        let mut blocks = Vec::new();
        for block in &self.blocks {
            let block = one_line(block)?;
            anyhow::ensure!(
                !block.split_whitespace().any(|token| token == "--new"),
                "split blocks with the block buttons, not with --new"
            );
            if !block.is_empty() {
                blocks.push(block);
            }
        }
        anyhow::ensure!(!blocks.is_empty(), "a strategy needs at least one block");
        Ok(Self { name: validate_name(&self.name)?, cloned_from: self.cloned_from.clone(), wf, blocks })
    }
}

fn one_line(text: &str) -> anyhow::Result<String> {
    if let Some(c) = text.chars().find(|c| "&|<>^".contains(*c)) {
        anyhow::bail!("the character {c} is not allowed in winws arguments");
    }
    Ok(text.lines().map(str::trim).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" "))
}

/// 1-40 characters: Latin letters, digits, space, `-`, `_`, `.`; a letter or digit first.
pub fn validate_name(name: &str) -> anyhow::Result<String> {
    let name = name.trim();
    let ok = (1..=40).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_alphanumeric())
        && name.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'));
    anyhow::ensure!(
        ok,
        "a name is 1-40 Latin letters, digits, spaces, '-', '_' or '.', starting with a letter or digit: {name}"
    );
    Ok(name.to_owned())
}

/// The `--wf-*` options and the `--new` blocks of a strategy file's winws command.
pub fn split_bat(bat: &str) -> anyhow::Result<(String, Vec<String>)> {
    let mut lines = bat.lines();
    let mut line = lines
        .by_ref()
        .find_map(|line| crate::batparse::find_ci(line, "winws.exe\"").map(|end| &line[end..]))
        .ok_or_else(|| anyhow::anyhow!("no winws.exe command in the strategy file"))?;
    let mut command = String::new();
    loop {
        let trimmed = line.trim_end();
        let Some(rest) = trimmed.strip_suffix('^') else {
            command.push_str(trimmed);
            break;
        };
        command.push_str(rest);
        command.push(' ');
        match lines.next() {
            Some(next) => line = next,
            None => break,
        }
    }
    let tokens = quoted_tokens(&command);
    let wf_len = tokens.iter().take_while(|token| token.starts_with("--wf-")).count();
    let blocks = tokens[wf_len..]
        .split(|token| token == "--new")
        .map(|block| block.join(" "))
        .filter(|block| !block.is_empty())
        .collect();
    Ok((tokens[..wf_len].join(" "), blocks))
}

/// Whitespace-separated; whitespace inside quotes does not split.
fn quoted_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for c in text.chars() {
        if c == '"' {
            quoted = !quoted;
        }
        if c.is_whitespace() && !quoted {
            if !current.is_empty() {
                tokens.push(std::mem::take(&mut current));
            }
        } else {
            current.push(c);
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// `winws --dry-run <args>` when this winws knows the option; otherwise winws runs for 2 s and
/// must not exit (spec 6). An error carries winws's whole output.
pub fn dry_run(folder: &Folder, parsed: &ParsedArgs) -> anyhow::Result<String> {
    let tail = parsed.command_tail();
    if std::fs::read(folder.winws())?.windows(9).any(|bytes| bytes == b"--dry-run") {
        let mut cmd = Command::new(folder.winws());
        cmd.raw_arg(format!("--dry-run{tail}")).current_dir(folder.bin());
        let out = exec::run(&mut cmd, Duration::from_secs(15), None)?.check()?;
        return Ok(format!("{}{}", out.stdout, out.stderr));
    }
    let mut winws = crate::procs::Winws::start(folder, &tail)?;
    let ready = winws.wait_ready(Duration::from_secs(2));
    let output = winws.stop();
    ready.map_err(anyhow::Error::msg)?;
    Ok(output)
}

/// Normalized, parsed as `service.bat` would, accepted by winws.
pub fn validate(folder: &Folder, s: &CustomStrategy) -> anyhow::Result<(CustomStrategy, ParsedArgs)> {
    let s = s.normalized()?;
    let parsed = crate::batparse::parse(&s.to_bat(), &folder.root_dp0(), &crate::game_filter::read(folder).vars())?;
    dry_run(folder, &parsed)?;
    Ok((s, parsed))
}

/// `data/strategies/<name>.toml`.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Every record by name. A record that does not parse is an error naming its file.
    pub fn list(&self) -> anyhow::Result<Vec<CustomStrategy>> {
        let entries = match std::fs::read_dir(&self.dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(err) => return Err(err.into()),
        };
        let mut records = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if !path.extension().is_some_and(|ext| ext.eq_ignore_ascii_case("toml")) {
                continue;
            }
            let text = std::fs::read_to_string(&path)?;
            records.push(toml::from_str::<CustomStrategy>(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?);
        }
        records.sort_by_key(|s| s.name.to_lowercase());
        Ok(records)
    }

    pub fn get(&self, name: &str) -> anyhow::Result<CustomStrategy> {
        self.list()?
            .into_iter()
            .find(|s| s.name.eq_ignore_ascii_case(name))
            .ok_or_else(|| anyhow::anyhow!("no custom strategy named {name}"))
    }

    pub fn save(&self, s: &CustomStrategy) -> anyhow::Result<()> {
        std::fs::create_dir_all(&self.dir)?;
        std::fs::write(self.path(&s.name), toml::to_string_pretty(s)?)?;
        Ok(())
    }

    pub fn remove(&self, name: &str) -> anyhow::Result<()> {
        let s = self.get(name)?;
        std::fs::remove_file(self.path(&s.name))?;
        Ok(())
    }

    fn path(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{name}.toml"))
    }
}

fn bat_path(folder: &Folder, s: &CustomStrategy) -> PathBuf {
    folder.root().join(format!("{}.bat", s.file_stem()))
}

pub fn write_bat(folder: &Folder, s: &CustomStrategy) -> anyhow::Result<()> {
    let path = bat_path(folder, s);
    std::fs::write(&path, s.to_bat())?;
    log::info!("custom strategy written: {}", path.display());
    Ok(())
}

/// Validates `s`, then writes its record and its `.bat`. `new` refuses a name already in use (a
/// record or a file); otherwise the record must exist.
pub fn save(folder: &Folder, store: &Store, s: &CustomStrategy, new: bool) -> anyhow::Result<CustomStrategy> {
    let (s, _) = validate(folder, s)?;
    let exists = store.get(&s.name).is_ok();
    if new {
        anyhow::ensure!(!exists && !bat_path(folder, &s).exists(), "the name {} is taken", s.name);
    } else {
        anyhow::ensure!(exists, "no custom strategy named {}", s.name);
    }
    store.save(&s)?;
    write_bat(folder, &s)?;
    Ok(s)
}

/// «Клонировать»: a new custom strategy with `source`'s filter and blocks.
pub fn clone_from(folder: &Folder, store: &Store, source: &str, name: &str) -> anyhow::Result<CustomStrategy> {
    let file = crate::strategies::find(folder, source)?;
    let (wf, blocks) = split_bat(&crate::read_text(&file.path)?)?;
    save(folder, store, &CustomStrategy { name: name.to_owned(), cloned_from: file.name, wf, blocks }, true)
}

/// Record and file. `active`, the registry's strategy name, cannot be deleted (spec 6).
pub fn delete(folder: &Folder, store: &Store, name: &str, active: Option<&str>) -> anyhow::Result<()> {
    let s = store.get(name)?;
    anyhow::ensure!(
        !active.is_some_and(|active| active.eq_ignore_ascii_case(&s.file_stem())),
        "{} is the active strategy: apply another one first",
        s.file_stem()
    );
    let bat = bat_path(folder, &s);
    if bat.exists() {
        std::fs::remove_file(&bat)?;
    }
    store.remove(&s.name)
}

/// A Flowseal update replaces the folder: the missing `.bat` files are written again (spec 6).
pub fn materialize_all(folder: &Folder, store: &Store) -> anyhow::Result<Vec<String>> {
    let mut restored = Vec::new();
    for s in store.list()? {
        if !bat_path(folder, &s).exists() {
            write_bat(folder, &s)?;
            restored.push(s.name);
        }
    }
    Ok(restored)
}
```

- [ ] **Шаг 4: Тесты проходят**

Run: `cargo test -p zapret --test custom`
Expected: PASS, 6 тестов (живой пропущен).

Run (из повышенного терминала): `cargo test -p zapret --test custom -- --ignored`
Expected: PASS: `winws --dry-run` принимает клон ALT11, аргументы совпадают с оригиналом.

- [ ] **Шаг 5: Поправить спецификацию**

В разделе 14 замените пункт про `winws --dry-run` на: «**`winws --dry-run`.** Проверено: `winws.exe` из Flowseal 1.10.3 знает `--dry-run` (строка есть в его справке). Запасной путь (6) остаётся для сборок без него.».

В разделе 6, пункт «Модель», замените «Блок хранится как список аргументов с подстановками (`%BIN%`, `%LISTS%`, `%GameFilter…%`).» на «Блок хранится строкой аргументов с подстановками (`%BIN%`, `%LISTS%`, `%GameFilter…%`), как в `.bat`: её показывает и правит редактор, а разбирает тот же парсер (5.3).».

- [ ] **Шаг 6: Коммит**

```bash
git add crates/zapret docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md
git commit -m "feat(zapret): clone, edit and store custom strategies as Flowseal files"
```

---

### Задача 10: `zapret` — снимок состояния и откат

**Files:**
- Create: `crates/zapret/src/snapshot.rs`
- Modify: `crates/zapret/src/lib.rs` (`pub mod snapshot;`)
- Test: `crates/zapret/tests/snapshot.rs`

**Interfaces:**
- Consumes: `service::{status, config, strategy, set_strategy, create, start, stop, stop_and_delete, SERVICE}`, `procs::{find, kill_all, spawn_detached, Proc}`, `ipset::{mode, has_backup, set_mode, leave_test_any, IpsetMode}`
- Produces:
  - `snapshot::ServiceSnap { image_path, auto_start, running }`, `Snapshot { operation, service: Option<ServiceSnap>, strategy: Option<String>, manual_winws: Vec<Proc>, ipset: IpsetMode, ipset_backup: bool }` (serde)
  - `snapshot::pending_path(data_dir: &Path) -> PathBuf` (`data/state/pending.json`)
  - `Snapshot::take(&Folder, operation) -> Snapshot`, `.save(&Path)`, `Snapshot::load(&Path) -> anyhow::Result<Option<Snapshot>>`, `Snapshot::clear(&Path)`, `.restore_files(&Folder)`, `.restore(&Folder) -> anyhow::Result<()>`

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret/tests/snapshot.rs`:

```rust
mod common;

use std::fs;
use std::path::PathBuf;

use zapret::ipset::IpsetMode;
use zapret::procs::Proc;
use zapret::snapshot::{ServiceSnap, Snapshot, pending_path};

fn sample(ipset: IpsetMode) -> Snapshot {
    Snapshot {
        operation: "tests".into(),
        service: Some(ServiceSnap {
            image_path: r#""E:\Zapret\bin\winws.exe"   --wf-tcp 80"#.into(),
            auto_start: true,
            running: true,
        }),
        strategy: Some("general (ALT11)".into()),
        manual_winws: vec![Proc {
            pid: 42,
            exe: Some(PathBuf::from(r"E:\Zapret\bin\winws.exe")),
            args: vec!["--wf-tcp=80".into()],
        }],
        ipset,
        ipset_backup: false,
    }
}

#[test]
fn pending_json_round_trips() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    assert_eq!(Snapshot::load(&path).unwrap(), None);
    sample(IpsetMode::Loaded).save(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), Some(sample(IpsetMode::Loaded)));
    Snapshot::clear(&path).unwrap();
    Snapshot::clear(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), None);
}

#[test]
fn a_damaged_pending_json_is_an_error_naming_the_file() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "{").unwrap();
    let err = Snapshot::load(&path).unwrap_err().to_string();
    assert!(err.contains("pending.json"), "{err}");
}

#[test]
fn restore_files_returns_the_ipset_after_a_killed_run() {
    let dir = common::fake_zapret();
    let folder = common::folder(&dir);
    let list = dir.path().join("lists").join("ipset-all.txt");
    fs::write(&list, "1.1.1.0/24\r\n").unwrap();
    let snap = sample(IpsetMode::Loaded);

    zapret::ipset::enter_test_any(&folder).unwrap(); // the app was killed during the DPI suite
    snap.restore_files(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");

    zapret::ipset::set_mode(&folder, IpsetMode::None).unwrap(); // a mode switch that failed later
    snap.restore_files(&folder).unwrap();
    assert_eq!(fs::read_to_string(&list).unwrap(), "1.1.1.0/24\r\n");
}

#[test]
#[ignore = "needs admin rights and the zapret service installed from E:\\Zapret"]
fn live_snapshot_of_this_machine() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let snap = Snapshot::take(&folder, "probe");
    eprintln!("{snap:#?}");
    assert_eq!(snap.operation, "probe");
    let service = snap.service.as_ref().expect("the zapret service is installed");
    assert!(service.image_path.to_lowercase().contains("winws.exe"), "{}", service.image_path);
    assert!(snap.strategy.is_some(), "service.bat records the installed strategy in the registry");
    // What a crash leaves behind reads back as it was taken.
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    snap.save(&path).unwrap();
    assert_eq!(Snapshot::load(&path).unwrap(), Some(snap));
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test snapshot`
Expected: FAIL при компиляции, модуля `snapshot` нет.

- [ ] **Шаг 3: Реализация**

`crates/zapret/src/snapshot.rs`:

```rust
//! What a mutating operation may change, written down before it starts (spec 10):
//! `data/state/pending.json` outlives a crash, `restore` puts everything back.

use std::path::{Path, PathBuf};

use crate::folder::Folder;
use crate::ipset::{self, IpsetMode};
use crate::procs::{self, Proc};
use crate::service::{self, SERVICE, SvcState};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ServiceSnap {
    pub image_path: String,
    pub auto_start: bool,
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Snapshot {
    /// The operation it guards: `apply`, `tests`, `ipset`, `game_filter`, `remove_service`.
    pub operation: String,
    pub service: Option<ServiceSnap>,
    /// `zapret-discord-youtube` in the registry.
    pub strategy: Option<String>,
    /// winws started by a `.bat`, not by the service.
    pub manual_winws: Vec<Proc>,
    pub ipset: IpsetMode,
    pub ipset_backup: bool,
}

/// `data/state/pending.json`
pub fn pending_path(data_dir: &Path) -> PathBuf {
    data_dir.join("state").join("pending.json")
}

impl Snapshot {
    pub fn take(folder: &Folder, operation: &str) -> Self {
        let status = service::status(SERVICE);
        let service = service::config(SERVICE).map(|config| ServiceSnap {
            image_path: config.image_path,
            auto_start: config.auto_start,
            running: matches!(status, Some((SvcState::Running | SvcState::StartPending, _))),
        });
        let service_pid = status.map(|(_, pid)| pid).filter(|pid| *pid != 0);
        Self {
            operation: operation.to_owned(),
            service,
            strategy: service::strategy(),
            manual_winws: procs::find("winws.exe").into_iter().filter(|p| Some(p.pid) != service_pid).collect(),
            ipset: ipset::mode(folder),
            ipset_backup: ipset::has_backup(folder),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// `None` when no operation was left unfinished.
    pub fn load(path: &Path) -> anyhow::Result<Option<Self>> {
        match std::fs::read_to_string(path) {
            Ok(text) => Ok(Some(serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?)),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    pub fn clear(path: &Path) -> anyhow::Result<()> {
        match std::fs::remove_file(path) {
            Err(err) if err.kind() != std::io::ErrorKind::NotFound => Err(err.into()),
            _ => Ok(()),
        }
    }

    /// The files: the DPI test switch undone, the ipset mode as it was.
    pub fn restore_files(&self, folder: &Folder) -> anyhow::Result<()> {
        ipset::leave_test_any(folder)?;
        ipset::set_mode(folder, self.ipset)
    }

    /// Everything. Every step is tried even after one fails; the failures come back together.
    pub fn restore(&self, folder: &Folder) -> anyhow::Result<()> {
        let mut errors: Vec<String> = Vec::new();
        let mut note = |step: &str, result: anyhow::Result<()>| {
            if let Err(err) = result {
                errors.push(format!("{step}: {err:#}"));
            }
        };
        note("ipset", self.restore_files(folder));
        note("stop zapret", service::stop(SERVICE));
        note("stop winws", procs::kill_all("winws.exe"));
        note("service", self.restore_service());
        for proc in &self.manual_winws {
            note("start winws", procs::spawn_detached(proc));
        }
        log::info!("state before {} restored, {} errors", self.operation, errors.len());
        anyhow::ensure!(errors.is_empty(), "{}", errors.join("\n\n"));
        Ok(())
    }

    fn restore_service(&self) -> anyhow::Result<()> {
        let Some(snap) = &self.service else { return service::stop_and_delete(SERVICE) };
        let same = service::config(SERVICE)
            .is_some_and(|config| config.image_path == snap.image_path && config.auto_start == snap.auto_start);
        if !same {
            service::stop_and_delete(SERVICE)?;
            service::create(&snap.image_path, snap.auto_start)?;
        }
        if let Some(name) = &self.strategy {
            service::set_strategy(name)?;
        }
        if snap.running { service::start(SERVICE) } else { service::stop(SERVICE) }
    }
}
```

После этой задачи модули `crates/zapret/src/lib.rs`:

```rust
pub mod batparse;
pub mod checks;
pub mod custom;
pub mod discord_cache;
pub mod fakes;
pub mod flags;
pub mod folder;
pub mod game_filter;
pub mod hosts;
pub mod ipset;
pub mod procs;
pub mod service;
pub mod snapshot;
pub mod strategies;
pub mod targets;
pub mod updates;
pub mod user_lists;
```

- [ ] **Шаг 4: Тесты проходят**

Run: `cargo test -p zapret`
Expected: PASS, все тесты крейта.

Run (из повышенного терминала): `cargo test -p zapret --test snapshot -- --ignored --nocapture`
Expected: PASS; печатает снимок: служба `zapret` с её `binPath`, стратегия, пустой `manual_winws`, режим ipset.

- [ ] **Шаг 5: Коммит**

```bash
git add crates/zapret
git commit -m "feat(zapret): snapshot the zapret state and restore it after a failure"
```

---

### Задача 11: `tester` — модель результатов, рейтинг, история, отчёт

**Files:**
- Create: `crates/tester/Cargo.toml`, `crates/tester/src/lib.rs`, `crates/tester/src/model.rs`, `crates/tester/src/rank.rs`, `crates/tester/src/history.rs`, `crates/tester/src/report.rs`
- Modify: `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md` (7.4)
- Test: `crates/tester/tests/rank.rs`, `crates/tester/tests/history.rs`

**Interfaces:**
- Consumes: `probe::curl::{CheckKind, CheckResult, CheckStatus, DpiResult, DpiStatus}`, `zapret::targets::{Target, url_host, defaults}`, `zapret::strategies::natural_key`
- Produces:
  - `model::Suite { Standard, Dpi }`, `TargetResult { name, url: Option<String>, extra: bool, checks: Vec<CheckResult>, ping_ms: Option<u32> }`, `DpiTargetResult { id, provider, country, host, checks: Vec<DpiResult> }`, `ProfileScore { ok, total, median_ms }`, `Score { ok, total, errors, ssl, unsup, median_ms, ping_ok, ping_total, ping_median_ms, fail, likely_blocked, discord, youtube, dbd }` + `.not_ok()`, `StrategyResult { name, args_sha256, launch_error: Option<String>, targets, dpi, score }`, `TestRun { id, suite, flowseal_version, canceled, restore_error, strategies }`
  - `rank::{DISCORD, YOUTUBE, DBD}`, `extra_targets(&[Target]) -> Vec<Target>`, `score_standard(&[TargetResult]) -> Score`, `score_dpi(&[DpiTargetResult]) -> Score`, `sort(Suite, &mut [StrategyResult])`, `median(&mut [u32]) -> Option<u32>`
  - `history::dir(data_dir) -> PathBuf`, `save(dir, &TestRun)`, `load(dir, id) -> anyhow::Result<TestRun>`, `all(dir) -> anyhow::Result<Vec<TestRun>>`, `RunSummary { id, suite, canceled, strategies: u32, best: Option<String>, best_score: Option<Score> }`, `summary(&TestRun) -> RunSummary`, `summaries(dir)`, `Latest { run_id, suite, result }`, `latest_results(dir) -> anyhow::Result<HashMap<String, Latest>>` (ключ — имя в нижнем регистре), `latest_ranking(dir) -> anyhow::Result<Option<TestRun>>`
  - `report::to_text(&TestRun) -> String`

Рейтинг считает `UNSUP` ошибкой, в отличие от раздела 7.4 спецификации. Причина: `curl.exe` из Windows собран со schannel и отдаёт сброс TLS-соединения — обычный ответ DPI — кодом 35, а скрипт по коду 35 ставит `UNSUP`. Если не считать `UNSUP`, стратегия, которую режет DPI, выглядит безошибочной. Шаг 5 переносит это в спецификацию.

- [ ] **Шаг 1: Манифест и модель**

`crates/tester/Cargo.toml`:

```toml
[package]
name = "tester"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "Strategy test runs for Zapret Hub: the procedure, the ranking, the history."

[dependencies]
exec.workspace = true
probe.workspace = true
zapret.workspace = true
anyhow.workspace = true
log.workspace = true
serde.workspace = true
serde_json.workspace = true
specta.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`crates/tester/src/lib.rs`:

```rust
//! Strategy tests (spec 7): run, rank, keep, report.

pub mod history;
pub mod model;
pub mod rank;
pub mod report;
```

`crates/tester/src/model.rs`:

```rust
//! What a run produces and `data/tests/<id>.json` keeps (spec 7.5).

use probe::curl::{CheckResult, DpiResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum Suite {
    Standard,
    Dpi,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TargetResult {
    pub name: String,
    /// `None` for a ping-only target.
    pub url: Option<String>,
    /// A profile host `targets.txt` lacks: shown, never counted (spec 7.4).
    pub extra: bool,
    pub checks: Vec<CheckResult>,
    /// ICMP round trip; `None` on timeout.
    pub ping_ms: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct DpiTargetResult {
    pub id: String,
    pub provider: String,
    pub country: String,
    pub host: String,
    pub checks: Vec<DpiResult>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ProfileScore {
    pub ok: u32,
    pub total: u32,
    pub median_ms: Option<u32>,
}

/// The counts behind the ranking (spec 7.4). A DPI run fills `ok`, `total`, `fail`, `unsup`,
/// `likely_blocked`.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Score {
    pub ok: u32,
    /// 3 checks per URL target of `targets.txt`: 36 with Flowseal's file.
    pub total: u32,
    pub errors: u32,
    pub ssl: u32,
    pub unsup: u32,
    /// Median `time_total` of the checks that passed.
    pub median_ms: Option<u32>,
    pub ping_ok: u32,
    pub ping_total: u32,
    pub ping_median_ms: Option<u32>,
    pub fail: u32,
    pub likely_blocked: u32,
    pub discord: ProfileScore,
    pub youtube: ProfileScore,
    pub dbd: ProfileScore,
}

impl Score {
    /// Checks that did not pass: the first ranking key.
    pub fn not_ok(&self) -> u32 {
        self.total.saturating_sub(self.ok)
    }
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StrategyResult {
    pub name: String,
    /// SHA-256 of the arguments it ran with: once they change, this result is stale.
    pub args_sha256: String,
    /// The file did not parse, winws did not start or exited: the whole output.
    pub launch_error: Option<String>,
    pub targets: Vec<TargetResult>,
    pub dpi: Vec<DpiTargetResult>,
    pub score: Score,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct TestRun {
    /// `2026-10-10_05-03-34`, also the file name.
    pub id: String,
    pub suite: Suite,
    pub flowseal_version: Option<String>,
    pub canceled: bool,
    /// What could not be put back after the run.
    pub restore_error: Option<String>,
    /// Best first.
    pub strategies: Vec<StrategyResult>,
}
```

- [ ] **Шаг 2: Падающие тесты**

`crates/tester/tests/rank.rs`:

```rust
use probe::curl::{CheckKind, CheckResult, CheckStatus, DpiResult, DpiStatus};
use tester::model::{DpiTargetResult, ProfileScore, StrategyResult, Suite, TargetResult, TestRun};
use tester::rank::{extra_targets, median, score_dpi, score_standard, sort};

use CheckStatus::{Error, Ok as Pass, Ssl, Unsup};

fn target(name: &str, url: &str, statuses: [CheckStatus; 3], ms: u32) -> TargetResult {
    let checks = CheckKind::ALL
        .iter()
        .zip(statuses)
        .map(|(kind, status)| CheckResult {
            kind: *kind,
            status,
            http_code: if status == Pass { "200".into() } else { "000".into() },
            time_ms: Some(ms),
            stderr: String::new(),
        })
        .collect();
    TargetResult { name: name.into(), url: Some(url.into()), extra: false, checks, ping_ms: Some(7) }
}

fn strategy(name: &str, targets: Vec<TargetResult>) -> StrategyResult {
    StrategyResult {
        name: name.into(),
        args_sha256: "x".into(),
        launch_error: None,
        score: score_standard(&targets),
        targets,
        dpi: Vec::new(),
    }
}

#[test]
fn unsup_and_ssl_count_as_failures() {
    let score = score_standard(&[target("A", "https://a.com", [Pass, Unsup, Ssl], 100)]);
    assert_eq!((score.ok, score.total, score.not_ok()), (1, 3, 2));
    assert_eq!((score.unsup, score.ssl, score.errors), (1, 1, 0));
    assert_eq!(score.median_ms, Some(100));
}

#[test]
fn ranking_is_failures_then_latency_then_menu_order() {
    let fast = |name| strategy(name, vec![target("A", "https://a.com", [Pass, Pass, Pass], 80)]);
    let mut results = vec![
        strategy("general (ALT3)", vec![target("A", "https://a.com", [Pass, Pass, Error], 10)]),
        fast("general (ALT10)"),
        strategy("general (ALT1)", vec![target("A", "https://a.com", [Pass, Pass, Pass], 120)]),
        StrategyResult { launch_error: Some("winws exited".into()), ..fast("general (ALT0)") },
        fast("general (ALT2)"),
    ];
    sort(Suite::Standard, &mut results);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["general (ALT2)", "general (ALT10)", "general (ALT1)", "general (ALT3)", "general (ALT0)"]);
}

#[test]
fn a_dead_network_ranks_by_menu_order() {
    let dead = |name| strategy(name, vec![target("A", "https://a.com", [Error, Error, Error], 2000)]);
    let mut results = vec![dead("general"), dead("general (ALT10)"), dead("general (ALT2)")];
    sort(Suite::Standard, &mut results);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["general (ALT2)", "general (ALT10)", "general"]);
}

#[test]
fn extra_profile_targets_show_but_do_not_count() {
    let targets = vec![
        target("DiscordMain", "https://discord.com", [Pass, Pass, Pass], 50),
        TargetResult { extra: true, ..target("steam.live.bhvrdbd.com", "https://steam.live.bhvrdbd.com", [Error, Error, Error], 0) },
    ];
    let score = score_standard(&targets);
    assert_eq!((score.ok, score.total), (3, 3));
    assert_eq!(score.discord, ProfileScore { ok: 3, total: 3, median_ms: Some(50) });
    assert_eq!((score.dbd.ok, score.dbd.total), (0, 3));
    assert_eq!(score.youtube.total, 0);
}

#[test]
fn extra_targets_are_the_profile_hosts_targets_txt_lacks() {
    let extra = extra_targets(&zapret::targets::defaults());
    let hosts: Vec<&str> = extra.iter().map(|t| t.name.as_str()).collect();
    assert_eq!(hosts, ["steam.live.bhvrdbd.com", "api.epicgames.dev", "store.steampowered.com"]);
    assert_eq!(extra[0].url.as_deref(), Some("https://steam.live.bhvrdbd.com"));
}

#[test]
fn dpi_ranks_blocked_then_failed() {
    let dpi = |name: &str, statuses: &[DpiStatus]| {
        let checks = statuses
            .iter()
            .map(|status| DpiResult {
                kind: CheckKind::Tls12,
                status: *status,
                code: "200".into(),
                up_bytes: 0,
                down_bytes: 0,
                time_s: 0.1,
            })
            .collect();
        let targets = vec![DpiTargetResult {
            id: "X".into(),
            provider: "P".into(),
            country: String::new(),
            host: "x.example".into(),
            checks,
        }];
        StrategyResult {
            name: name.into(),
            args_sha256: "x".into(),
            launch_error: None,
            targets: Vec::new(),
            score: score_dpi(&targets),
            dpi: targets,
        }
    };
    let mut results = vec![
        dpi("A", &[DpiStatus::LikelyBlocked, DpiStatus::Ok]),
        dpi("B", &[DpiStatus::Fail, DpiStatus::Fail]),
        dpi("C", &[DpiStatus::Fail, DpiStatus::Ok]),
    ];
    sort(Suite::Dpi, &mut results);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["C", "B", "A"]);
    assert_eq!((results[2].score.likely_blocked, results[2].score.total), (1, 2));
}

#[test]
fn median_is_the_lower_middle() {
    assert_eq!(median(&mut [4, 1, 3, 2]), Some(2));
    assert_eq!(median(&mut [5]), Some(5));
    assert_eq!(median(&mut []), None);
}

#[test]
fn report_reads_like_test_zapret_ps1() {
    let targets = vec![
        target("DiscordMain", "https://discord.com", [Pass, Ssl, Unsup], 50),
        TargetResult { name: "CloudflareDNS1111".into(), url: None, extra: false, checks: Vec::new(), ping_ms: None },
    ];
    let run = TestRun {
        id: "2026-10-10_05-03-34".into(),
        suite: Suite::Standard,
        flowseal_version: Some("1.10.3".into()),
        canceled: false,
        restore_error: None,
        strategies: vec![strategy("general (ALT11)", targets)],
    };
    assert_eq!(
        tester::report::to_text(&run),
        concat!(
            "Config: general (ALT11).bat (Type: standard)\r\n",
            "  DiscordMain : HTTP:OK    TLS1.2:SSL   TLS1.3:UNSUP | Ping: 7 ms\r\n",
            "  CloudflareDNS1111 :  | Ping: Timeout\r\n",
            "\r\n",
            "=== ANALYTICS ===\r\n",
            "general (ALT11).bat : HTTP OK:   1, ERR:   1, UNSUP:   1, Ping OK:   1, Fail:   1\r\n",
            "Best strategy: general (ALT11).bat\r\n",
        )
    );
}
```

`crates/tester/tests/history.rs`:

```rust
use tester::history;
use tester::model::{Score, StrategyResult, Suite, TestRun};

fn result(name: &str, sha: &str) -> StrategyResult {
    StrategyResult {
        name: name.into(),
        args_sha256: sha.into(),
        launch_error: None,
        targets: Vec::new(),
        dpi: Vec::new(),
        score: Score::default(),
    }
}

fn run(id: &str, suite: Suite, canceled: bool, strategies: Vec<StrategyResult>) -> TestRun {
    TestRun { id: id.into(), suite, flowseal_version: None, canceled, restore_error: None, strategies }
}

#[test]
fn runs_are_saved_listed_and_loaded() {
    let dir = tempfile::tempdir().unwrap();
    history::save(dir.path(), &run("2026-10-09_10-00-00", Suite::Standard, false, vec![result("general (ALT)", "a")])).unwrap();
    history::save(dir.path(), &run("2026-10-10_10-00-00", Suite::Dpi, false, vec![result("general (ALT2)", "b")])).unwrap();
    let ids: Vec<String> = history::summaries(dir.path()).unwrap().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, ["2026-10-10_10-00-00", "2026-10-09_10-00-00"]);
    assert_eq!(history::load(dir.path(), "2026-10-09_10-00-00").unwrap().strategies[0].name, "general (ALT)");
    assert!(history::load(dir.path(), r"..\..\secret").is_err());
}

#[test]
fn latest_result_per_strategy_and_the_latest_full_ranking() {
    let dir = tempfile::tempdir().unwrap();
    history::save(
        dir.path(),
        &run("2026-10-08_10-00-00", Suite::Standard, false, vec![result("general (ALT)", "old"), result("general (ALT2)", "x")]),
    )
    .unwrap();
    history::save(dir.path(), &run("2026-10-09_10-00-00", Suite::Standard, true, vec![result("general (ALT)", "new")])).unwrap();
    history::save(dir.path(), &run("2026-10-10_10-00-00", Suite::Dpi, false, vec![result("general (ALT3)", "d")])).unwrap();
    let latest = history::latest_results(dir.path()).unwrap();
    assert_eq!(latest["general (alt)"].result.args_sha256, "new");
    assert_eq!(latest["general (alt2)"].run_id, "2026-10-08_10-00-00");
    assert_eq!(latest["general (alt3)"].suite, Suite::Dpi);
    assert_eq!(history::latest_ranking(dir.path()).unwrap().unwrap().id, "2026-10-08_10-00-00");
}

#[test]
fn a_damaged_run_file_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("2026-10-10_00-00-00.json"), "{").unwrap();
    history::save(dir.path(), &run("2026-10-09_10-00-00", Suite::Standard, false, Vec::new())).unwrap();
    assert_eq!(history::summaries(dir.path()).unwrap().len(), 1);
}
```

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p tester`
Expected: FAIL при компиляции, модулей `rank`, `history`, `report` нет (создайте пустые файлы, чтобы `lib.rs` собрался).

- [ ] **Шаг 4: Реализация**

`crates/tester/src/rank.rs`:

```rust
//! Scores and order (spec 7.4).

use probe::curl::{CheckStatus, DpiStatus};
use zapret::targets::{Target, url_host};

use crate::model::{DpiTargetResult, ProfileScore, Score, StrategyResult, Suite, TargetResult};

pub const DISCORD: [&str; 4] = ["discord.com", "gateway.discord.gg", "cdn.discordapp.com", "updates.discord.com"];
pub const YOUTUBE: [&str; 3] = ["www.youtube.com", "i.ytimg.com", "redirector.googlevideo.com"];
pub const DBD: [&str; 3] = ["steam.live.bhvrdbd.com", "api.epicgames.dev", "store.steampowered.com"];

/// Profile hosts `targets` does not check: they run on the side, outside the count.
pub fn extra_targets(targets: &[Target]) -> Vec<Target> {
    let covered: Vec<String> =
        targets.iter().filter_map(|t| t.url.as_deref()).map(|url| url_host(url).to_ascii_lowercase()).collect();
    DISCORD
        .into_iter()
        .chain(YOUTUBE)
        .chain(DBD)
        .filter(|host| !covered.iter().any(|c| c == host))
        .map(|host| Target { name: host.to_owned(), url: Some(format!("https://{host}")), ping: host.to_owned() })
        .collect()
}

pub fn score_standard(targets: &[TargetResult]) -> Score {
    let mut score = Score::default();
    let (mut times, mut pings) = (Vec::new(), Vec::new());
    for target in targets.iter().filter(|t| !t.extra) {
        for check in &target.checks {
            score.total += 1;
            match check.status {
                CheckStatus::Ok => {
                    score.ok += 1;
                    times.extend(check.time_ms);
                }
                CheckStatus::Ssl => score.ssl += 1,
                CheckStatus::Unsup => score.unsup += 1,
                CheckStatus::Error => score.errors += 1,
            }
        }
        score.ping_total += 1;
        if let Some(ms) = target.ping_ms {
            score.ping_ok += 1;
            pings.push(ms);
        }
    }
    score.median_ms = median(&mut times);
    score.ping_median_ms = median(&mut pings);
    score.discord = profile(targets, &DISCORD);
    score.youtube = profile(targets, &YOUTUBE);
    score.dbd = profile(targets, &DBD);
    score
}

/// Every check of the targets whose host is in `hosts`, extra ones included.
fn profile(targets: &[TargetResult], hosts: &[&str]) -> ProfileScore {
    let mut profile = ProfileScore::default();
    let mut times = Vec::new();
    let in_profile = |t: &&TargetResult| {
        t.url.as_deref().is_some_and(|url| hosts.iter().any(|host| url_host(url).eq_ignore_ascii_case(host)))
    };
    for check in targets.iter().filter(in_profile).flat_map(|t| &t.checks) {
        profile.total += 1;
        if check.status == CheckStatus::Ok {
            profile.ok += 1;
            times.extend(check.time_ms);
        }
    }
    profile.median_ms = median(&mut times);
    profile
}

pub fn score_dpi(targets: &[DpiTargetResult]) -> Score {
    let mut score = Score::default();
    for check in targets.iter().flat_map(|t| &t.checks) {
        score.total += 1;
        match check.status {
            DpiStatus::Ok => score.ok += 1,
            DpiStatus::Fail => score.fail += 1,
            DpiStatus::Unsupported => score.unsup += 1,
            DpiStatus::LikelyBlocked => score.likely_blocked += 1,
        }
    }
    score
}

/// Best first. Launch errors go last; then standard: fewer failed checks, lower median latency;
/// DPI: fewer LIKELY_BLOCKED, fewer FAIL; then `service.bat`'s menu order.
pub fn sort(suite: Suite, results: &mut [StrategyResult]) {
    results.sort_by_cached_key(|r| {
        let s = &r.score;
        let (first, second) = match suite {
            Suite::Standard => (s.not_ok(), s.median_ms.unwrap_or(u32::MAX)),
            Suite::Dpi => (s.likely_blocked, s.fail),
        };
        (r.launch_error.is_some(), first, second, zapret::strategies::natural_key(&format!("{}.bat", r.name)))
    });
}

/// The lower middle: `[1, 2, 3, 4]` → 2.
pub fn median(values: &mut [u32]) -> Option<u32> {
    values.sort_unstable();
    values.get(values.len().checked_sub(1)? / 2).copied()
}
```

`crates/tester/src/history.rs`:

```rust
//! `data/tests/<id>.json`: every finished run (spec 7.5).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use crate::model::{Score, StrategyResult, Suite, TestRun};

pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("tests")
}

pub fn save(dir: &Path, run: &TestRun) -> anyhow::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(dir.join(format!("{}.json", run.id)), serde_json::to_string(run)?)?;
    Ok(())
}

pub fn load(dir: &Path, id: &str) -> anyhow::Result<TestRun> {
    anyhow::ensure!(
        !id.is_empty() && id.chars().all(|c| c.is_ascii_digit() || c == '-' || c == '_'),
        "not a run id: {id}"
    );
    let path = dir.join(format!("{id}.json"));
    let text = std::fs::read_to_string(&path).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
    serde_json::from_str(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))
}

/// Every readable run, newest first. A damaged file goes to the journal and is skipped: one bad
/// file must not hide the rest.
// ponytail: reads every run on each call; an index file when the history grows to hundreds.
pub fn all(dir: &Path) -> anyhow::Result<Vec<TestRun>> {
    let entries = match std::fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(err) => return Err(err.into()),
    };
    let mut runs = Vec::new();
    for entry in entries {
        let path = entry?.path();
        if !path.extension().is_some_and(|ext| ext == "json") {
            continue;
        }
        let parsed = std::fs::read_to_string(&path)
            .map_err(anyhow::Error::from)
            .and_then(|text| Ok(serde_json::from_str::<TestRun>(&text)?));
        match parsed {
            Ok(run) => runs.push(run),
            Err(err) => log::error!("test run skipped, {}: {err:#}", path.display()),
        }
    }
    runs.sort_by(|a, b| b.id.cmp(&a.id));
    Ok(runs)
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct RunSummary {
    pub id: String,
    pub suite: Suite,
    pub canceled: bool,
    pub strategies: u32,
    /// The first of the ranking, unless even it failed to start.
    pub best: Option<String>,
    pub best_score: Option<Score>,
}

pub fn summary(run: &TestRun) -> RunSummary {
    let best = run.strategies.first().filter(|s| s.launch_error.is_none());
    RunSummary {
        id: run.id.clone(),
        suite: run.suite,
        canceled: run.canceled,
        strategies: run.strategies.len() as u32,
        best: best.map(|s| s.name.clone()),
        best_score: best.map(|s| s.score.clone()),
    }
}

pub fn summaries(dir: &Path) -> anyhow::Result<Vec<RunSummary>> {
    Ok(all(dir)?.iter().map(summary).collect())
}

/// A strategy's newest result and the run it came from.
#[derive(Debug, Clone, PartialEq)]
pub struct Latest {
    pub run_id: String,
    pub suite: Suite,
    pub result: StrategyResult,
}

/// The newest result of every strategy, keyed by its lower-case name.
pub fn latest_results(dir: &Path) -> anyhow::Result<HashMap<String, Latest>> {
    let mut latest = HashMap::new();
    for run in all(dir)? {
        for result in run.strategies {
            latest
                .entry(result.name.to_lowercase())
                .or_insert_with(|| Latest { run_id: run.id.clone(), suite: run.suite, result });
        }
    }
    Ok(latest)
}

/// The newest standard run that finished: «Применить лучшую», the overview, the tray's top five.
pub fn latest_ranking(dir: &Path) -> anyhow::Result<Option<TestRun>> {
    Ok(all(dir)?.into_iter().find(|run| run.suite == Suite::Standard && !run.canceled))
}
```

`crates/tester/src/report.rs`:

```rust
//! A run as `test zapret.ps1` writes its results file: «Копировать».

use probe::curl::{CheckStatus, DpiStatus};

use crate::model::{Suite, TestRun};

pub fn to_text(run: &TestRun) -> String {
    let suite = match run.suite {
        Suite::Standard => "standard",
        Suite::Dpi => "dpi",
    };
    let mut lines: Vec<String> = Vec::new();
    for s in &run.strategies {
        lines.push(format!("Config: {}.bat (Type: {suite})", s.name));
        if let Some(err) = &s.launch_error {
            lines.extend(err.lines().map(|line| format!("  {line}")));
        }
        for t in s.targets.iter().filter(|t| !t.extra) {
            let http: Vec<String> = t.checks.iter().map(|c| format!("{}:{}", c.kind.label(), token(c.status))).collect();
            let ping = t.ping_ms.map_or_else(|| "Timeout".to_owned(), |ms| format!("{ms} ms"));
            lines.push(format!("  {} : {} | Ping: {ping}", t.name, http.join(" ")));
        }
        for t in &s.dpi {
            lines.push(if t.country.is_empty() {
                format!("  Target: {} ({})", t.id, t.provider)
            } else {
                format!("  Target: [{}] {} ({})", t.country, t.id, t.provider)
            });
            for c in &t.checks {
                lines.push(format!(
                    "    {}: code={}  up={} KB  down={} KB  time={}s  status={}",
                    c.kind.label(),
                    c.code,
                    kb(c.up_bytes),
                    kb(c.down_bytes),
                    c.time_s,
                    dpi_status(c.status)
                ));
            }
        }
        lines.push(String::new());
    }
    lines.push("=== ANALYTICS ===".to_owned());
    let width = run.strategies.iter().map(|s| s.name.len() + 4).max().unwrap_or(0);
    for s in &run.strategies {
        let config = format!("{:width$}", format!("{}.bat", s.name));
        let sc = &s.score;
        lines.push(match run.suite {
            Suite::Standard => format!(
                "{config} : HTTP OK: {:>3}, ERR: {:>3}, UNSUP: {:>3}, Ping OK: {:>3}, Fail: {:>3}",
                sc.ok,
                sc.errors + sc.ssl,
                sc.unsup,
                sc.ping_ok,
                sc.ping_total - sc.ping_ok
            ),
            Suite::Dpi => format!(
                "{config} : OK: {:>3}, FAIL: {:>3}, UNSUP: {:>3}, BLOCKED: {:>3}",
                sc.ok, sc.fail, sc.unsup, sc.likely_blocked
            ),
        });
    }
    if let Some(best) = run.strategies.first().filter(|s| s.launch_error.is_none()) {
        lines.push(format!("Best strategy: {}.bat", best.name));
    }
    lines.join("\r\n") + "\r\n"
}

fn token(status: CheckStatus) -> &'static str {
    match status {
        CheckStatus::Ok => "OK   ",
        CheckStatus::Ssl => "SSL  ",
        CheckStatus::Unsup => "UNSUP",
        CheckStatus::Error => "ERROR",
    }
}

fn dpi_status(status: DpiStatus) -> &'static str {
    match status {
        DpiStatus::Ok => "OK",
        DpiStatus::Fail => "FAIL",
        DpiStatus::Unsupported => "UNSUPPORTED",
        DpiStatus::LikelyBlocked => "LIKELY_BLOCKED",
    }
}

/// `[math]::Round(bytes / 1024, 1)` as PowerShell prints it: `64`, `63.5`.
fn kb(bytes: u32) -> String {
    format!("{}", (f64::from(bytes) / 1024.0 * 10.0).round() / 10.0)
}
```

- [ ] **Шаг 5: Тесты проходят, правка спецификации**

Run: `cargo test -p tester`
Expected: PASS, 11 тестов.

В разделе 7.4 спецификации замените первый пункт на: «1. **Ошибки:** число проверок не `OK` (`ERROR`, `SSL` и `UNSUP`) из 3 × (число URL-целей) — сейчас 36 (12 × 3). `UNSUP` считается: `curl.exe` из Windows (schannel) отдаёт сброс TLS-соединения, обычный ответ DPI, кодом 35, который скрипт помечает как `UNSUP`.» Третий пункт «Имя стратегии» замените на «Порядок меню `service.bat`.».

- [ ] **Шаг 6: Коммит**

```bash
git add crates/tester Cargo.lock docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md
git commit -m "feat(tester): score, rank, keep and report strategy test runs"
```

---

### Задача 12: `tester` — набор DPI, процедура прогона, каталог стратегий

**Files:**
- Create: `crates/tester/src/dpi.rs`, `crates/tester/src/run.rs`, `crates/tester/src/catalog.rs`
- Modify: `crates/tester/src/lib.rs` (`pub mod catalog; pub mod dpi; pub mod run;`)
- Test: `crates/tester/tests/run.rs`, `crates/tester/tests/catalog.rs`

**Interfaces:**
- Consumes: `zapret::{folder::Folder, procs::Winws, snapshot::{Snapshot, pending_path}, service, ipset, targets, strategies, batparse, custom}`, `probe::{curl, ping}`, всё из задачи 11
- Produces:
  - `dpi::{SUITE_URL, DpiTarget { id, provider, country, host }, parse(&str), load(cache_dir, socks5) -> anyhow::Result<Vec<DpiTarget>>, from_fetch(cache_dir, anyhow::Result<String>)}`
  - `run::Request { suite: Suite, strategies: Vec<String> }` (serde, specta), `run::Progress` (`#[serde(tag = "kind")]`: `Strategy { index, total, name }`, `Done { result: StrategyResult }`, `Restoring`)
  - `run::run(&Folder, data_dir: &Path, &Request, socks5: Option<&str>, cancel: &AtomicBool, progress: &(dyn Fn(Progress) + Sync)) -> anyhow::Result<TestRun>`
  - `run::par_map(&[T], workers, f) -> Vec<R>`, `run::parallelism() -> usize`
  - `catalog::{LastResult { run_id, suite, score, launch_error: bool, stale: bool }, StrategyRow { name, custom, active, error: Option<String>, last: Option<LastResult> }, StrategyDetail { row, wf, blocks, final_args: Option<String>, result: Option<StrategyResult>, record: Option<CustomStrategy> }}`
  - `catalog::rows(&Folder, &Store, data_dir, active: Option<&str>) -> anyhow::Result<Vec<StrategyRow>>`, `catalog::detail(&Folder, &Store, data_dir, active, name) -> anyhow::Result<StrategyDetail>`

winws ждёт строки `capture is started` до 5 с (спецификация 7.3), затем пауза 300 мс, как в `test zapret.ps1`. Если в журнале видно, что каждая стратегия ждёт полные 5 с, значит winws не сбрасывает stdout в канал: прогон медленнее, но верен.

- [ ] **Шаг 1: Падающие тесты**

`crates/tester/tests/run.rs`:

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tester::model::Suite;
use tester::run::{Progress, Request, par_map, parallelism};

#[test]
fn par_map_keeps_the_order() {
    let items: Vec<u32> = (0..50).collect();
    let doubled = par_map(&items, 8, |x| {
        std::thread::sleep(Duration::from_millis(5));
        x * 2
    });
    assert_eq!(doubled, items.iter().map(|x| x * 2).collect::<Vec<_>>());
    assert!(par_map(&Vec::<u32>::new(), 8, |x| *x).is_empty());
}

#[test]
fn parallelism_is_between_8_and_16() {
    assert!((8..=16).contains(&parallelism()));
}

#[test]
fn the_dpi_suite_falls_back_to_its_cache() {
    let dir = tempfile::tempdir().unwrap();
    let json = r#"[{ "id": "US.GH-HPRN", "provider": "Self check", "country": "🧠", "host": "hyperion-cs.github.io" }]"#;
    assert!(tester::dpi::from_fetch(dir.path(), Err(anyhow::anyhow!("offline"))).is_err());
    let fresh = tester::dpi::from_fetch(dir.path(), Ok(json.into())).unwrap();
    assert_eq!(fresh[0].host, "hyperion-cs.github.io");
    assert_eq!(tester::dpi::from_fetch(dir.path(), Err(anyhow::anyhow!("offline"))).unwrap(), fresh);
    assert_eq!(tester::dpi::from_fetch(dir.path(), Ok("<html>".into())).unwrap(), fresh, "a broken download keeps the cache");
}

#[test]
#[ignore = "needs admin rights, E:\\Zapret and the network; stops zapret for about a minute"]
fn live_run_of_two_strategies() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let data = tempfile::tempdir().unwrap();
    let request = Request { suite: Suite::Standard, strategies: vec!["general (ALT11)".into(), "general".into()] };
    let run = tester::run::run(&folder, data.path(), &request, None, &AtomicBool::new(false), &|p| eprintln!("{p:?}")).unwrap();
    eprintln!("{}", tester::report::to_text(&run));
    assert_eq!(run.strategies.len(), 2);
    assert!(run.restore_error.is_none(), "{:?}", run.restore_error);
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
    assert!(tester::history::dir(data.path()).join(format!("{}.json", run.id)).is_file());
}

#[test]
#[ignore = "needs admin rights, E:\\Zapret and the network"]
fn live_cancel_stops_after_the_current_strategy() {
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let data = tempfile::tempdir().unwrap();
    let cancel = AtomicBool::new(false);
    let request = Request { suite: Suite::Standard, strategies: vec!["general (ALT11)".into(), "general".into()] };
    let run = tester::run::run(&folder, data.path(), &request, None, &cancel, &|p| {
        if matches!(p, Progress::Done { .. }) {
            cancel.store(true, Ordering::Relaxed);
        }
    })
    .unwrap();
    assert!(run.canceled);
    assert_eq!(run.strategies.len(), 1);
    assert!(run.restore_error.is_none(), "{:?}", run.restore_error);
}
```

`crates/tester/tests/catalog.rs`:

```rust
use tester::history;
use tester::model::{Score, StrategyResult, Suite, TestRun};
use zapret::custom::{CustomStrategy, Store};
use zapret::folder::Folder;

/// A Flowseal folder with one real strategy and one helper `.bat`.
fn fake_zapret() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    std::fs::write(root.join("service.bat"), "set \"LOCAL_VERSION=1.10.3\"\r\n").unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("bin").join("winws.exe"), b"").unwrap();
    std::fs::write(root.join("bin").join("WinDivert64.sys"), b"").unwrap();
    std::fs::create_dir_all(root.join("lists")).unwrap();
    std::fs::create_dir_all(root.join("utils")).unwrap();
    std::fs::write(
        root.join("general.bat"),
        "start \"z\" /min \"%BIN%winws.exe\" --wf-tcp=80 ^\r\n--filter-tcp=80 --dpi-desync=fake\r\n",
    )
    .unwrap();
    std::fs::write(root.join("helper.bat"), "@echo off\r\necho hi\r\n").unwrap();
    dir
}

fn result(name: &str, sha: &str) -> StrategyResult {
    StrategyResult {
        name: name.into(),
        args_sha256: sha.into(),
        launch_error: None,
        targets: Vec::new(),
        dpi: Vec::new(),
        score: Score::default(),
    }
}

#[test]
fn rows_carry_custom_active_errors_and_stale_results() {
    let zapret_dir = fake_zapret();
    let folder = Folder::open(zapret_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    let mine = CustomStrategy {
        name: "mine".into(),
        cloned_from: "general".into(),
        wf: "--wf-tcp=80".into(),
        blocks: vec!["--filter-tcp=80".into()],
    };
    store.save(&mine).unwrap();
    zapret::custom::write_bat(&folder, &mine).unwrap();
    std::fs::write(zapret_dir.path().join("general (HUB orphan).bat"), mine.to_bat()).unwrap();

    let general_sha = zapret::batparse::parse_file(&folder, &zapret_dir.path().join("general.bat")).unwrap().sha256();
    let run = TestRun {
        id: "2026-10-10_10-00-00".into(),
        suite: Suite::Standard,
        flowseal_version: None,
        canceled: false,
        restore_error: None,
        strategies: vec![result("general", &general_sha), result("general (HUB mine)", "outdated")],
    };
    history::save(&history::dir(data.path()), &run).unwrap();

    let rows = tester::catalog::rows(&folder, &store, data.path(), Some("GENERAL")).unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(names, ["general (HUB mine)", "general (HUB orphan)", "general", "helper"]);
    let [mine_row, orphan, general, helper] = &rows[..] else { panic!("{rows:?}") };
    assert!(mine_row.custom && !mine_row.active && mine_row.last.as_ref().unwrap().stale);
    assert!(!orphan.custom && orphan.last.is_none());
    assert!(general.active && !general.last.as_ref().unwrap().stale);
    assert!(helper.error.as_deref().unwrap().contains("no winws.exe"));

    let detail = tester::catalog::detail(&folder, &store, data.path(), None, "general (HUB mine)").unwrap();
    assert_eq!(detail.wf, "--wf-tcp=80");
    assert_eq!(detail.blocks, ["--filter-tcp=80"]);
    assert_eq!(detail.record, Some(mine));
    assert_eq!(detail.result.unwrap().args_sha256, "outdated");
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p tester --test run --test catalog`
Expected: FAIL при компиляции, модулей `dpi`, `run`, `catalog` нет.

- [ ] **Шаг 3: Реализация**

В `crates/tester/src/lib.rs`:

```rust
pub mod catalog;
pub mod dpi;
pub mod history;
pub mod model;
pub mod rank;
pub mod report;
pub mod run;
```

`crates/tester/src/dpi.rs`:

```rust
//! The hyperion-cs DPI 16-20 suite (Apache-2.0), fetched as `test zapret.ps1` fetches it; the last
//! good copy stays in `data/cache` (spec 7.2).

use std::path::Path;

pub const SUITE_URL: &str = "https://hyperion-cs.github.io/dpi-checkers/ru/tcp-16-20/suite.v2.json";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DpiTarget {
    pub id: String,
    #[serde(default)]
    pub provider: String,
    #[serde(default)]
    pub country: String,
    pub host: String,
}

pub fn parse(json: &str) -> anyhow::Result<Vec<DpiTarget>> {
    Ok(serde_json::from_str(json)?)
}

pub fn load(cache_dir: &Path, socks5: Option<&str>) -> anyhow::Result<Vec<DpiTarget>> {
    from_fetch(cache_dir, probe::curl::fetch_text(SUITE_URL, socks5))
}

/// `fetched` when it parses (it becomes the cached copy), the cached copy otherwise.
pub fn from_fetch(cache_dir: &Path, fetched: anyhow::Result<String>) -> anyhow::Result<Vec<DpiTarget>> {
    let cache = cache_dir.join("dpi-suite.json");
    let fresh = fetched.and_then(|text| {
        let targets = parse(&text)?;
        anyhow::ensure!(!targets.is_empty(), "the DPI suite is empty");
        Ok((text, targets))
    });
    match fresh {
        Ok((text, targets)) => {
            std::fs::create_dir_all(cache_dir)?;
            std::fs::write(&cache, text)?;
            Ok(targets)
        }
        Err(err) => {
            log::warn!("DPI suite not downloaded, the cached copy is used: {err:#}");
            let text = std::fs::read_to_string(&cache)
                .map_err(|_| anyhow::anyhow!("the DPI suite could not be downloaded and there is no cached copy: {err:#}"))?;
            parse(&text)
        }
    }
}
```

`crates/tester/src/run.rs`:

```rust
//! A test run (spec 7.3): snapshot, zapret stopped, each strategy started as winws directly with
//! the arguments «Применить» would install, the checks, everything put back.

use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::time::Duration;

use probe::curl::CheckKind;
use zapret::folder::Folder;
use zapret::procs::Winws;
use zapret::snapshot::{Snapshot, pending_path};
use zapret::targets::Target;

use crate::dpi::DpiTarget;
use crate::model::{DpiTargetResult, Score, StrategyResult, Suite, TargetResult, TestRun};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Request {
    pub suite: Suite,
    /// Strategy names, in the order to run them.
    pub strategies: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(tag = "kind")]
pub enum Progress {
    /// Strategy `index` (from 0) of `total` starts.
    Strategy { index: u32, total: u32, name: String },
    Done { result: StrategyResult },
    Restoring,
}

/// winws gets this long to report `capture is started` (spec 7.3).
const READY: Duration = Duration::from_secs(5);
/// The pause `test zapret.ps1` makes once winws is up.
const SETTLE: Duration = Duration::from_millis(300);

pub fn run(
    folder: &Folder,
    data_dir: &Path,
    request: &Request,
    socks5: Option<&str>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Progress) + Sync),
) -> anyhow::Result<TestRun> {
    anyhow::ensure!(probe::curl::available(), "curl.exe was not found: the tests need Windows 10 1803 or newer");
    let id = exec::LocalTime::now().stamp();
    let pending = pending_path(data_dir);
    let snapshot = Snapshot::take(folder, "tests");
    snapshot.save(&pending)?;
    let outcome = run_strategies(folder, data_dir, request, socks5, cancel, progress);
    progress(Progress::Restoring);
    let restore_error = snapshot.restore(folder).err().map(|err| format!("{err:#}"));
    if let Err(err) = Snapshot::clear(&pending) {
        log::error!("{} not removed: {err:#}", pending.display());
    }
    let mut strategies = outcome?;
    crate::rank::sort(request.suite, &mut strategies);
    let run = TestRun {
        id,
        suite: request.suite,
        flowseal_version: folder.version(),
        canceled: cancel.load(Ordering::Relaxed),
        restore_error,
        strategies,
    };
    crate::history::save(&crate::history::dir(data_dir), &run)?;
    Ok(run)
}

/// What every strategy is checked against.
enum Work {
    /// Targets with their `extra` flag.
    Standard(Vec<(Target, bool)>),
    Dpi(Vec<DpiTarget>, PathBuf),
}

fn run_strategies(
    folder: &Folder,
    data_dir: &Path,
    request: &Request,
    socks5: Option<&str>,
    cancel: &AtomicBool,
    progress: &(dyn Fn(Progress) + Sync),
) -> anyhow::Result<Vec<StrategyResult>> {
    zapret::service::stop(zapret::service::SERVICE)?;
    zapret::procs::kill_all("winws.exe")?;
    let work = match request.suite {
        Suite::Standard => {
            let targets = zapret::targets::load(folder);
            let extra = crate::rank::extra_targets(&targets);
            Work::Standard(targets.into_iter().map(|t| (t, false)).chain(extra.into_iter().map(|t| (t, true))).collect())
        }
        Suite::Dpi => {
            let cache = data_dir.join("cache");
            let targets = crate::dpi::load(&cache, socks5)?;
            let payload = cache.join("dpi-payload.bin");
            probe::curl::write_payload(&payload)?;
            zapret::ipset::enter_test_any(folder)?;
            Work::Dpi(targets, payload)
        }
    };
    let total = request.strategies.len() as u32;
    let mut results = Vec::new();
    for (index, name) in request.strategies.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            break;
        }
        progress(Progress::Strategy { index: index as u32, total, name: name.clone() });
        let result = run_one(folder, name, &work, cancel);
        if cancel.load(Ordering::Relaxed) {
            break; // cut short: not a result
        }
        progress(Progress::Done { result: result.clone() });
        results.push(result);
    }
    Ok(results)
}

fn run_one(folder: &Folder, name: &str, work: &Work, cancel: &AtomicBool) -> StrategyResult {
    let mut result = StrategyResult {
        name: name.to_owned(),
        args_sha256: String::new(),
        launch_error: None,
        targets: Vec::new(),
        dpi: Vec::new(),
        score: Score::default(),
    };
    let parsed = zapret::strategies::find(folder, name).and_then(|file| zapret::batparse::parse_file(folder, &file.path));
    let parsed = match parsed {
        Ok(parsed) => parsed,
        Err(err) => {
            result.launch_error = Some(format!("{err:#}"));
            return result;
        }
    };
    result.args_sha256 = parsed.sha256();
    let mut winws = match Winws::start(folder, &parsed.command_tail()) {
        Ok(winws) => winws,
        Err(err) => {
            result.launch_error = Some(format!("winws did not start: {err}"));
            return result;
        }
    };
    if let Err(output) = winws.wait_ready(READY) {
        result.launch_error = Some(output);
        return result;
    }
    std::thread::sleep(SETTLE);
    let workers = parallelism();
    match work {
        Work::Standard(targets) => {
            result.targets = par_map(targets, workers, |(target, extra)| check_target(target, *extra, cancel));
            result.score = crate::rank::score_standard(&result.targets);
        }
        Work::Dpi(targets, payload) => {
            result.dpi = par_map(targets, workers, |target| check_dpi(target, payload, cancel));
            result.score = crate::rank::score_dpi(&result.dpi);
        }
    }
    if !winws.is_alive() {
        result.launch_error = Some(format!("winws exited during the checks:\n{}", winws.output_text()));
    }
    winws.stop();
    result
}

fn check_target(target: &Target, extra: bool, cancel: &AtomicBool) -> TargetResult {
    let checks = match &target.url {
        Some(url) => CheckKind::ALL.iter().map(|kind| probe::curl::check(url, *kind, Some(cancel))).collect(),
        None => Vec::new(),
    };
    TargetResult {
        name: target.name.clone(),
        url: target.url.clone(),
        extra,
        checks,
        ping_ms: probe::ping::ping(&target.ping, 1000),
    }
}

fn check_dpi(target: &DpiTarget, payload: &Path, cancel: &AtomicBool) -> DpiTargetResult {
    let checks =
        CheckKind::ALL.iter().map(|kind| probe::curl::dpi_check(&target.host, *kind, payload, Some(cancel))).collect();
    DpiTargetResult {
        id: target.id.clone(),
        provider: target.provider.clone(),
        country: target.country.clone(),
        host: target.host.clone(),
        checks,
    }
}

/// `min(16, max(8, cores × 2))` targets at a time (spec 7.1).
pub fn parallelism() -> usize {
    std::thread::available_parallelism().map_or(4, |n| n.get()).saturating_mul(2).clamp(8, 16)
}

/// `f` over `items` on `workers` threads; the results keep the input order.
pub fn par_map<T: Sync, R: Send>(items: &[T], workers: usize, f: impl Fn(&T) -> R + Sync) -> Vec<R> {
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<R>>> = items.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..workers.clamp(1, items.len().max(1)) {
            scope.spawn(|| {
                loop {
                    let i = next.fetch_add(1, Ordering::Relaxed);
                    let Some(item) = items.get(i) else { break };
                    *slots[i].lock().unwrap() = Some(f(item));
                }
            });
        }
    });
    slots.into_iter().map(|slot| slot.into_inner().unwrap().expect("every slot is filled")).collect()
}
```

`crates/tester/src/catalog.rs`:

```rust
//! The rows and the detail pane of the «Стратегии» tab.

use std::collections::HashMap;
use std::path::Path;

use zapret::custom::{CustomStrategy, Store};
use zapret::folder::Folder;
use zapret::strategies::StrategyFile;

use crate::history::Latest;
use crate::model::{Score, StrategyResult, Suite};

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LastResult {
    pub run_id: String,
    pub suite: Suite,
    pub score: Score,
    pub launch_error: bool,
    /// The arguments changed since that run (spec 7.5, «устарел»).
    pub stale: bool,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StrategyRow {
    /// File stem: `general (ALT11)`, `general (HUB mine)`.
    pub name: String,
    /// A `HUB` file with a record in `data/strategies` (spec 5.2).
    pub custom: bool,
    pub active: bool,
    /// Not a strategy (`service.bat` would capture nothing) or not readable.
    pub error: Option<String>,
    pub last: Option<LastResult>,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct StrategyDetail {
    pub row: StrategyRow,
    pub wf: String,
    pub blocks: Vec<String>,
    /// `Final args` as `service.bat` prints them.
    pub final_args: Option<String>,
    /// Every check of the last run the strategy was in.
    pub result: Option<StrategyResult>,
    /// The record behind a custom strategy, for the editor.
    pub record: Option<CustomStrategy>,
}

/// Every strategy file in `service.bat`'s order; `active` is the registry's strategy name.
pub fn rows(folder: &Folder, store: &Store, data_dir: &Path, active: Option<&str>) -> anyhow::Result<Vec<StrategyRow>> {
    let records = store.list()?;
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    Ok(zapret::strategies::list(folder)?.iter().map(|file| row(folder, &records, &latest, active, file)).collect())
}

pub fn detail(
    folder: &Folder,
    store: &Store,
    data_dir: &Path,
    active: Option<&str>,
    name: &str,
) -> anyhow::Result<StrategyDetail> {
    let file = zapret::strategies::find(folder, name)?;
    let records = store.list()?;
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    let row = row(folder, &records, &latest, active, &file);
    let (wf, blocks) = zapret::custom::split_bat(&zapret::read_text(&file.path)?).unwrap_or_default();
    Ok(StrategyDetail {
        row,
        wf,
        blocks,
        final_args: zapret::batparse::parse_file(folder, &file.path).ok().map(|parsed| parsed.final_args),
        result: latest.get(&file.name.to_lowercase()).map(|latest| latest.result.clone()),
        record: records.into_iter().find(|r| r.file_stem().eq_ignore_ascii_case(&file.name)),
    })
}

fn row(
    folder: &Folder,
    records: &[CustomStrategy],
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    file: &StrategyFile,
) -> StrategyRow {
    let parsed = zapret::batparse::parse_file(folder, &file.path);
    let last = latest.get(&file.name.to_lowercase()).map(|latest| LastResult {
        run_id: latest.run_id.clone(),
        suite: latest.suite,
        score: latest.result.score.clone(),
        launch_error: latest.result.launch_error.is_some(),
        stale: parsed.as_ref().is_ok_and(|parsed| parsed.sha256() != latest.result.args_sha256),
    });
    StrategyRow {
        name: file.name.clone(),
        custom: records.iter().any(|r| r.file_stem().eq_ignore_ascii_case(&file.name)),
        active: active.is_some_and(|active| active.eq_ignore_ascii_case(&file.name)),
        error: parsed.err().map(|err| format!("{err:#}")),
        last,
    }
}
```

- [ ] **Шаг 4: Тесты проходят**

Run: `cargo test -p tester`
Expected: PASS, 15 тестов (два живых пропущены).

Run (из повышенного терминала, zapret на минуту остановится): `cargo test -p tester --test run -- --ignored --nocapture`
Expected: PASS; в выводе — прогресс двух стратегий, отчёт в формате `test zapret.ps1`, служба `zapret` после теста снова работает (`sc query zapret` → `RUNNING`).

- [ ] **Шаг 5: Коммит**

```bash
git add crates/tester Cargo.lock
git commit -m "feat(tester): add the DPI suite, the test run and the strategy catalog"
```

---

### Задача 13: Каркас приложения — Tauri, окно, привязки IPC, фронтенд

**Files:**
- Modify: `Cargo.toml` (members), `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md` (4.1, 13)
- Create: `package.json`, `tools/make-icon.ps1`, `src-tauri/Cargo.toml`, `src-tauri/build.rs`, `src-tauri/common-controls.manifest`, `src-tauri/tauri.conf.json`, `src-tauri/capabilities/default.json`, `src-tauri/icons/icon.ico` (рисует скрипт), `src-tauri/src/main.rs`, `src-tauri/src/bin/export-bindings.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/state.rs`, `src-tauri/src/texts.rs`, `src-tauri/src/window.rs`, `frontend/package.json`, `frontend/vite.config.ts`, `frontend/svelte.config.js`, `frontend/tsconfig.json`, `frontend/index.html`, `frontend/src/main.ts`, `frontend/src/App.svelte`, `frontend/src/app.css`, `frontend/src/lib/ipc/bindings.ts` (генерируется)

**Interfaces:**
- Consumes: —
- Produces:
  - `zapret_hub_lib::{run() -> anyhow::Result<()>, export_bindings() -> anyhow::Result<()>, report(&anyhow::Error)}`, внутри крейта `fatal(&str)`, `specta_builder()` (задачи 14 и 15 добавляют в него команды и события)
  - `state::Paths { exe_dir: PathBuf }`, `Paths::current() -> anyhow::Result<Paths>`, `.data() -> PathBuf`
  - `window::{MAIN = "main", show(&AppHandle)}`
  - `texts::{APP, FAILED_TO_START, WEBVIEW2_MISSING}`
  - Фронтенд: `commands.appVersion(): Promise<string>` в `$lib/ipc/bindings`; токены темы в `app.css` (`--bg`, `--panel`, `--text`, `--muted`, `--border`, `--accent`, `--ok`, `--warn`, `--bad`, `--hover`), тема задаётся атрибутом `data-theme="light|dark"` на `<html>`; скрипты `npm run dev|build|check|test`.

`tauri-build` кладёт манифест приложения в ресурс, а ресурс попадает только в бинарники (`cargo:rustc-link-arg-bins`). Тестовый exe без манифеста Common-Controls v6 не запускается (`STATUS_ENTRYPOINT_NOT_FOUND`: диалоги Tauri импортируют `TaskDialogIndirect`), а exe с `requireAdministrator` не запускается из обычного терминала. Поэтому манифест передаётся компоновщику: Common-Controls v6 — всем целям (так делают примеры самого Tauri), повышение — только бинарнику `zapret-hub`. Тесты и экспорт привязок работают без прав администратора.

- [ ] **Шаг 1: Workspace и npm**

В корневом `Cargo.toml` заменить `members = ["crates/*"]` на:

```toml
members = ["crates/*", "src-tauri"]
```

`package.json`:

```json
{
  "name": "zapret-hub",
  "version": "0.1.0",
  "private": true,
  "license": "MIT",
  "type": "module",
  "engines": {
    "node": ">=24"
  },
  "workspaces": [
    "frontend"
  ],
  "scripts": {
    "dev": "npm run dev -w frontend",
    "build": "npm run build -w frontend",
    "check": "npm run check -w frontend",
    "test": "npm run test -w frontend",
    "tauri": "tauri"
  },
  "devDependencies": {
    "@tauri-apps/cli": "2.12.1"
  }
}
```

- [ ] **Шаг 2: Иконка**

`tools/make-icon.ps1`:

```powershell
# Draws src-tauri/icons/icon.ico: a green disc with a white "Z", 256 px, stored as PNG inside the
# ICO (Windows Vista and later read that). Run once; the result is committed.
Add-Type -AssemblyName System.Drawing

$size = 256
$bitmap = New-Object System.Drawing.Bitmap $size, $size
$g = [System.Drawing.Graphics]::FromImage($bitmap)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit
$g.Clear([System.Drawing.Color]::Transparent)
$green = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(46, 184, 92))
$g.FillEllipse($green, 8, 8, 240, 240)
$font = New-Object System.Drawing.Font 'Segoe UI', 150, ([System.Drawing.FontStyle]::Bold), ([System.Drawing.GraphicsUnit]::Pixel)
$format = New-Object System.Drawing.StringFormat
$format.Alignment = [System.Drawing.StringAlignment]::Center
$format.LineAlignment = [System.Drawing.StringAlignment]::Center
$g.DrawString('Z', $font, [System.Drawing.Brushes]::White, (New-Object System.Drawing.RectangleF 0, 0, $size, $size), $format)
$g.Dispose()

$png = New-Object System.IO.MemoryStream
$bitmap.Save($png, [System.Drawing.Imaging.ImageFormat]::Png)
$bytes = $png.ToArray()

$out = Join-Path $PSScriptRoot '..\src-tauri\icons\icon.ico'
New-Item -ItemType Directory -Force (Split-Path $out) | Out-Null
$writer = New-Object System.IO.BinaryWriter ([System.IO.File]::Create($out))
$writer.Write([UInt16]0)               # ICONDIR: reserved
$writer.Write([UInt16]1)               # type: icon
$writer.Write([UInt16]1)               # one image
$writer.Write([Byte]0)                 # width 256 is written as 0
$writer.Write([Byte]0)                 # height 256 is written as 0
$writer.Write([Byte]0)                 # no palette
$writer.Write([Byte]0)                 # reserved
$writer.Write([UInt16]1)               # planes
$writer.Write([UInt16]32)              # bits per pixel
$writer.Write([UInt32]$bytes.Length)   # image size
$writer.Write([UInt32]22)              # image offset: 6 + 16
$writer.Write($bytes)
$writer.Close()
Write-Output "written $out"
```

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File tools/make-icon.ps1`
Expected: `written ...\src-tauri\icons\icon.ico`, файл около 4 КБ. Без него `tauri-build` останавливает сборку («icons/icon.ico not found»).

- [ ] **Шаг 3: Манифест крейта, build.rs, конфигурация Tauri**

`src-tauri/Cargo.toml`:

```toml
[package]
name = "zapret-hub"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "Zapret Hub: a tray app that manages Flowseal zapret and tests its strategies."
# Two binaries: plain `cargo run`, which `tauri dev` calls, must pick the app.
default-run = "zapret-hub"

[lib]
name = "zapret_hub_lib"

# The app's own test harness would carry `requireAdministrator` and fail in a plain terminal
# (os error 740); its code has no tests, they live in the library.
[[bin]]
name = "zapret-hub"
path = "src/main.rs"
test = false

[build-dependencies]
tauri-build.workspace = true

[dependencies]
anyhow.workspace = true
log.workspace = true
specta.workspace = true
specta-typescript.workspace = true
tauri.workspace = true
tauri-plugin-dialog.workspace = true
tauri-plugin-single-instance.workspace = true
tauri-specta.workspace = true
windows = { workspace = true, features = ["Win32_UI_WindowsAndMessaging"] }
windows-registry.workspace = true
```

`src-tauri/common-controls.manifest` (тот же, что встраивает `tauri-build` по умолчанию):

```xml
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <dependency>
    <dependentAssembly>
      <assemblyIdentity
        type="win32"
        name="Microsoft.Windows.Common-Controls"
        version="6.0.0.0"
        processorArchitecture="*"
        publicKeyToken="6595b64144ccf1df"
        language="*"
      />
    </dependentAssembly>
  </dependency>
</assembly>
```

`src-tauri/build.rs`:

```rust
//! The manifests go to the linker, not into tauri-build's resource: that resource reaches only
//! binaries, and a test binary without Common-Controls v6 does not start
//! (STATUS_ENTRYPOINT_NOT_FOUND), the workaround Tauri's own examples use.

use std::path::Path;

fn main() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("common-controls.manifest");
    println!("cargo:rerun-if-changed={}", manifest.display());
    println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
    println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", manifest.display());
    // Only the app asks for elevation: tests and the bindings export run in a plain terminal.
    println!("cargo:rustc-link-arg-bin=zapret-hub=/MANIFESTUAC:level='requireAdministrator'");
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .windows_attributes(tauri_build::WindowsAttributes::new_without_app_manifest()),
    )
    .expect("failed to run tauri-build");
}
```

`src-tauri/tauri.conf.json` (окон в конфигурации нет: окно создаётся кодом и уничтожается крестиком, спецификация 4.2):

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Zapret Hub",
  "mainBinaryName": "zapret-hub",
  "identifier": "ZapretHub",
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:1420",
    "frontendDist": "../frontend/dist"
  },
  "app": {
    "windows": [],
    "security": {
      "csp": "default-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:"
    }
  },
  "bundle": {
    "active": false,
    "icon": ["icons/icon.ico"]
  }
}
```

`src-tauri/capabilities/default.json` (страница сама показывает окно после первой отрисовки):

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "The main window.",
  "windows": ["main"],
  "permissions": ["core:default", "core:window:allow-show"]
}
```

- [ ] **Шаг 4: Код на Rust**

`src-tauri/src/main.rs`:

```rust
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Err(err) = zapret_hub_lib::run() {
        zapret_hub_lib::report(&err);
    }
}
```

`src-tauri/src/bin/export-bindings.rs`:

```rust
//! Writes `frontend/src/lib/ipc/bindings.ts` without starting the app, so without elevation.

fn main() {
    if let Err(err) = zapret_hub_lib::export_bindings() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
```

`src-tauri/src/texts.rs`:

```rust
//! The Russian text the backend shows itself: message boxes, the tray, notifications, notices.
//! The page's text lives in `frontend/src/lib/strings.ts`.

pub const APP: &str = "Zapret Hub";

pub const FAILED_TO_START: &str = "Zapret Hub не запустился.";

pub const WEBVIEW2_MISSING: &str = "Для окна Zapret Hub нужен Microsoft Edge WebView2 Runtime, а на этом \
компьютере его нет.\n\nСкачайте его: https://go.microsoft.com/fwlink/p/?LinkId=2124703\n\n\
Ctrl+C копирует этот текст.";
```

`src-tauri/src/state.rs` (задача 14 дополнит его состоянием приложения):

```rust
//! Where the app keeps its files (spec 4.3).

use std::path::PathBuf;

use anyhow::Context as _;

/// Everything lives next to the exe: the app is portable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub exe_dir: PathBuf,
}

impl Paths {
    pub fn current() -> anyhow::Result<Self> {
        let exe = std::env::current_exe().context("cannot find the executable's path")?;
        let exe_dir = exe.parent().context("the executable has no folder")?.to_path_buf();
        Ok(Self { exe_dir })
    }

    pub fn data(&self) -> PathBuf {
        self.exe_dir.join("data")
    }
}
```

`src-tauri/src/window.rs`:

```rust
//! The main window: made when needed, destroyed by its close button, while the app stays in the
//! tray (spec 4.2).

use tauri::{AppHandle, Manager as _, WebviewUrl, WebviewWindowBuilder};

pub const MAIN: &str = "main";

/// Brings the window up, making it first if it is gone. A new one starts hidden: the page shows
/// it once it has drawn, so it never flashes white.
pub fn show(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let shown = window.unminimize().and_then(|()| window.show()).and_then(|()| window.set_focus());
        if let Err(err) = shown {
            log::warn!("cannot show the window: {err}");
        }
        return;
    }
    // Off the main thread: building a webview from a main-thread handler deadlocks on Windows,
    // and tray clicks and the second-instance callback run there.
    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(err) = build(&app) {
            log::error!("cannot create the window: {err:#}");
        }
    });
}

fn build(app: &AppHandle) -> anyhow::Result<()> {
    let profile = crate::state::Paths::current()?.data().join("webview");
    WebviewWindowBuilder::new(app, MAIN, WebviewUrl::default())
        .title(crate::texts::APP)
        .inner_size(1100.0, 720.0)
        .min_inner_size(900.0, 560.0)
        .center()
        .visible(false)
        .data_directory(profile)
        .build()?;
    Ok(())
}
```

`src-tauri/src/lib.rs`:

```rust
//! Zapret Hub: IPC routing, the window, the tray and background jobs. The logic lives in `crates/`.

mod state;
mod texts;
mod window;

use specta_typescript::Typescript;
use tauri_specta::{Builder, collect_commands};

/// Regenerated by `cargo run -p zapret-hub --bin export-bindings` and by every debug start.
const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../frontend/src/lib/ipc/bindings.ts");

#[tauri::command]
#[specta::specta]
fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![app_version])
}

pub fn export_bindings() -> anyhow::Result<()> {
    specta_builder()
        .export(Typescript::default(), BINDINGS_PATH)
        .map_err(|err| anyhow::anyhow!("failed to export IPC bindings: {err}"))
}

/// What `main` does with an error from `run`.
pub fn report(err: &anyhow::Error) {
    fatal(&format!("{}\n\n{err:#}", texts::FAILED_TO_START));
}

/// The one native message box: there is no window to show the error in.
fn fatal(text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;
    log::error!("{text}");
    // SAFETY: a plain Win32 call with valid, NUL-terminated wide strings.
    let _ = unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(texts::APP), MB_OK | MB_ICONERROR) };
}

const WEBVIEW2_CLIENT: &str = r"Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// `pv` of the WebView2 Runtime, per machine (WOW6432Node on 64-bit Windows) or per user. An
/// uninstall leaves an empty value or `0.0.0.0` behind.
fn webview2_installed() -> bool {
    [
        windows_registry::LOCAL_MACHINE.open(format!(r"SOFTWARE\WOW6432Node\{WEBVIEW2_CLIENT}")),
        windows_registry::CURRENT_USER.open(format!(r"Software\{WEBVIEW2_CLIENT}")),
    ]
    .into_iter()
    .flatten()
    .filter_map(|key| key.get_string("pv").ok())
    .any(|version| !version.trim().is_empty() && version.trim() != "0.0.0.0")
}

pub fn run() -> anyhow::Result<()> {
    // Without the runtime no window can exist: the user is told here (spec 12).
    if !webview2_installed() {
        fatal(texts::WEBVIEW2_MISSING);
        std::process::exit(1);
    }
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(err) = export_bindings() {
        eprintln!("{err:#}");
    }
    tauri::Builder::default()
        // First: a second copy hands over to this one before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| window::show(app)))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            window::show(app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}
```

- [ ] **Шаг 5: Фронтенд**

`frontend/package.json`:

```json
{
  "name": "@zapret-hub/frontend",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "vite build",
    "check": "svelte-check --tsconfig ./tsconfig.json",
    "test": "vitest run"
  },
  "dependencies": {
    "@tauri-apps/api": "2.12.1"
  },
  "devDependencies": {
    "@sveltejs/vite-plugin-svelte": "7.3.1",
    "@tsconfig/svelte": "5.0.8",
    "@types/node": "^24.13.6",
    "svelte": "5.57.1",
    "svelte-check": "4.7.6",
    "typescript": "~6.0.3",
    "vite": "8.3.2",
    "vitest": "5.0.3"
  }
}
```

`frontend/vite.config.ts`:

```ts
import { defineConfig } from "vitest/config";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { fileURLToPath, URL } from "node:url";

export default defineConfig({
  plugins: [svelte()],
  resolve: {
    alias: { $lib: fileURLToPath(new URL("./src/lib", import.meta.url)) },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**", "**/target/**"] },
  },
  build: { target: "esnext", outDir: "dist", emptyOutDir: true },
  // Stores are tested on Node: runes compile to plain values there, which the logic needs.
  test: { environment: "node", include: ["src/**/*.test.ts"] },
});
```

`frontend/svelte.config.js`:

```js
import { vitePreprocess } from "@sveltejs/vite-plugin-svelte";

export default {
  preprocess: vitePreprocess(),
  compilerOptions: {
    // Svelte 5 runes everywhere.
    runes: true,
  },
};
```

`frontend/tsconfig.json`:

```json
{
  "extends": "@tsconfig/svelte/tsconfig.json",
  "compilerOptions": {
    "target": "ESNext",
    "module": "ESNext",
    "moduleResolution": "bundler",
    "resolveJsonModule": true,
    "isolatedModules": true,
    "strict": true,
    "noUncheckedIndexedAccess": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "verbatimModuleSyntax": true,
    "skipLibCheck": true,
    "types": ["vite/client", "node"],
    "paths": { "$lib/*": ["./src/lib/*"] }
  },
  "include": ["src/**/*.ts", "src/**/*.svelte", "vite.config.ts"],
  "exclude": ["node_modules", "dist"]
}
```

`frontend/index.html`:

```html
<!doctype html>
<html lang="ru">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Zapret Hub</title>
  </head>
  <body>
    <div id="app"></div>
    <script type="module" src="/src/main.ts"></script>
  </body>
</html>
```

`frontend/src/app.css` (палитра окончательная: следующие задачи только пользуются токенами):

```css
:root {
  --bg: #f4f6f8;
  --panel: #ffffff;
  --text: #1c2126;
  --muted: #5d6874;
  --border: #d8dde3;
  --accent: #2563eb;
  --ok: #1f9d55;
  --warn: #b7791f;
  --bad: #d64545;
  --hover: #eaeef3;
  color-scheme: light;
  font: 14px/1.45 "Segoe UI Variable Text", "Segoe UI", system-ui, sans-serif;
}

:root[data-theme="dark"] {
  --bg: #15181c;
  --panel: #1d2126;
  --text: #e4e8ec;
  --muted: #98a2ad;
  --border: #323840;
  --accent: #5b8def;
  --ok: #3fbf72;
  --warn: #e0a83a;
  --bad: #ef6b6b;
  --hover: #262b31;
  color-scheme: dark;
}

* {
  box-sizing: border-box;
}

html,
body {
  margin: 0;
  height: 100%;
  background: var(--bg);
  color: var(--text);
}

button {
  font: inherit;
  color: var(--text);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 5px 12px;
  cursor: pointer;
}

button:hover:not(:disabled) {
  background: var(--hover);
}

button:disabled {
  opacity: 0.5;
  cursor: default;
}

button.primary {
  background: var(--accent);
  border-color: var(--accent);
  color: #fff;
}

input,
select,
textarea {
  font: inherit;
  color: var(--text);
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 5px 8px;
}

.ok {
  color: var(--ok);
}

.warn {
  color: var(--warn);
}

.bad {
  color: var(--bad);
}

.muted {
  color: var(--muted);
}
```

`frontend/src/main.ts`:

```ts
import { mount } from "svelte";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App.svelte";
import "./app.css";

// The system's theme until the settings arrive: the first frame matches the desktop.
document.documentElement.dataset.theme = matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";

const app = mount(App, { target: document.getElementById("app")! });
// The window is made hidden; shown once the page has drawn, so it never flashes white.
void getCurrentWindow().show();

export default app;
```

`frontend/src/App.svelte` (временный: задача 17 заменяет его целиком):

```svelte
<script lang="ts">
  import { commands } from "$lib/ipc/bindings";

  let version = $state("");
  void commands.appVersion().then((v) => (version = v));
</script>

<main>
  <h1>Zapret Hub</h1>
  <p class="muted">{version}</p>
</main>
```

- [ ] **Шаг 6: Сборка, манифесты, привязки**

Run: `cargo build -p zapret-hub`
Expected: сборка без ошибок и предупреждений; в `target/debug` есть `zapret-hub.exe` и `export-bindings.exe`.

Run (PowerShell): `Select-String -Path target\debug\zapret-hub.exe -Pattern "requireAdministrator" -SimpleMatch -Quiet; Select-String -Path target\debug\export-bindings.exe -Pattern "requireAdministrator" -SimpleMatch -Quiet`
Expected: `True`, затем `False`. Если первое `False`, компоновщик не принял сокращённую запись: замените строку `/MANIFESTUAC:level='requireAdministrator'` на `/MANIFESTUAC:"level='requireAdministrator' uiAccess='false'"` и повторите шаг.

Run: `cargo run -p zapret-hub --bin export-bindings`
Expected: код 0, появился `frontend/src/lib/ipc/bindings.ts` с `appVersion`.

Run: `npm install`, затем `npm run check`, затем `npm run build`
Expected: `svelte-check found 0 errors and 0 warnings`; `frontend/dist/index.html` собран.

Run: `cargo clippy -p zapret-hub --all-targets -- -D warnings`
Expected: без замечаний.

Run (обычный терминал): `cargo test -p zapret-hub`
Expected: код 0, тестов пока нет. `os error 740` значит, что тестовый exe получил манифест с повышением: проверьте `test = false` в `[[bin]]`.

Вручную, из повышенного терминала (исполнитель без прав администратора пропускает и пишет об этом в отчёте): `npm run tauri dev` → окно «Zapret Hub» с номером версии, без белой вспышки. Повторный запуск `target\debug\zapret-hub.exe` не открывает вторую копию, а поднимает первое окно. Крестик завершает приложение: трея пока нет.

- [ ] **Шаг 7: Правка спецификации**

В `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md`, раздел 4.1, заменить блок:

```
crates/
  zapret/   модель папки Flowseal, служба, флаги, списки, обновления, системные проверки
  probe/    сетевые проверки (curl.exe, ICMP, SOCKS5)
  tester/   прогон стратегий, рейтинг, история
  diag/     подпроект 2 (пока пустой крейт с публичным API-заглушкой)
  router/   подпроект 3 (пока пустой крейт с публичным API-заглушкой)
```

на:

```
crates/
  exec/     запуск внешних программ: таймаут, отмена, журнал, вывод в OEM/UTF-16
  probe/    сетевые проверки (curl.exe, ICMP, SOCKS5)
  zapret/   модель папки Flowseal, служба, флаги, списки, обновления, системные проверки
  tester/   прогон стратегий, рейтинг, история
```

и строку `Зависимости: \`tester → zapret, probe\`; \`diag → probe, zapret, router\`; \`src-tauri → все\`.` на:

```
Зависимости: `probe → exec`; `zapret → exec, probe` (быстрая проверка после установки, загрузки
обновлений); `tester → exec, probe, zapret`; `src-tauri → все`. Крейты `diag` и `router` появятся
вместе со своими подпроектами: пустой крейт ничего не проверяет, а добавить крейт позже ничего не ломает.
```

В разделе 13 заменить обе фразы «В подпроекте 1 — пустой крейт и скрытая вкладка.» на «В подпроекте 1 крейта и вкладки нет.». Первая стоит в пункте `diag` на одной строке. Вторая, в пункте `router`, разбита переносом: строка кончается на «В подпроекте 1 —», следующая начинается с «пустой крейт и скрытая вкладка.»; после замены перенесите строки так, чтобы они не превышали ширину соседних.

- [ ] **Шаг 8: Коммит**

```bash
git add Cargo.toml Cargo.lock package.json package-lock.json tools/make-icon.ps1 src-tauri frontend docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md
git commit -m "feat(app): add the Tauri shell, the window and the IPC bindings"
```

---

### Задача 14: Состояние приложения и команды IPC

**Files:**
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/lib.rs`, `src-tauri/src/state.rs`, `src-tauri/src/texts.rs`, `frontend/src/App.svelte` (временная проверка), `frontend/src/lib/ipc/bindings.ts` (генерируется)
- Create: `src-tauri/src/settings.rs`, `src-tauri/src/journal.rs`, `src-tauri/src/dto.rs`, `src-tauri/src/autostart.rs`, `src-tauri/src/notify.rs`, `src-tauri/src/actions.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/jobs.rs`
- Test: модульные тесты внутри `settings.rs`, `state.rs`, `journal.rs`, `autostart.rs`, `texts.rs`

**Interfaces:**
- Consumes: всё из задач 1–12 (точные имена — в блоках Interfaces этих задач), `state::Paths`, `window::show`, `texts::APP` из задачи 13
- Produces (Rust, для задачи 15):
  - `settings::{Theme { System, Light, Dark }, Settings { zapret_dir: Option<String>, socks5: Option<String>, autostart: bool, start_minimized: bool, theme: Theme, wizard_done: bool }, load, save, load_or_default, validate_socks}`
  - `state::{Paths (+ .settings(), .strategies(), .logs()), lock(&Mutex<T>), AppState}`; у `AppState`: `paths`, `settings()`, `set_settings()`, `socks5()`, `folder()`, `store()`, `testing()`, `begin_testing()`, `end_testing()`, `cancel_tests()`, `cancel_flag()`, `turn()`, `change(f)`, `change_safely(operation, f)`, `live()`, `set_live()`, `update_live(f)`, `quick()`, `set_quick(check) -> Option<QuickCheck>`, `set_ranking()`, `last_run()`, `best(n) -> Vec<String>`, `update()`, `set_update()`, `push_notice()`, `take_notices()`
  - `actions::{state(&AppHandle), notice(app, level, title, text), changed(app), apply, apply_best, restart, quick_check, check_flowseal_update, save_settings, save_custom, prepare_folder, overview, flowseal, candidates, check_folder, open_url, open_folder}`; `apply`, `apply_best`, `restart`, `quick_check` возвращают `anyhow::Result<QuickCheck>`
  - `jobs::{start(&AppHandle), start_tests(&AppHandle, Request)}`, `notify::{register(), show(app, title, body)}`, `texts::{operation(id), tests_done(&TestRun), …}`
- Produces (TypeScript, `$lib/ipc/bindings`, для задач 16–20): типы `Settings`, `Theme`, `Overview`, `FlowsealState`, `ListState`, `FolderCheck`, `Candidates`, `Notice`, `NoticeLevel`, `LiveRun`, `FolderKind` и все типы крейтов, которые возвращают команды; команды (результат `{ status: "ok", data } | { status: "error", error: string }`, кроме отмеченных «без Result»):
  - `appVersion()` и `getSettings()` без Result; `saveSettings(settings)`, `getOverview()`, `checkProxy(addr)`, `wizardCandidates()`, `checkFolder(path)`, `pickFolder()`, `openUrl(url)`, `openFolder(kind)`
  - `listStrategies()`, `getStrategy(name)`, `applyStrategy(name)`, `applyBest()`, `restartZapret()`, `cloneStrategy(source, name)`, `validateCustom(strategy)`, `saveCustom(strategy, create)`, `deleteCustom(name)` (`name` — имя записи, `record.name`)
  - `startTests(request)`, `cancelTests()` и `testLive()` без Result, `listRuns()`, `getRun(id)`, `runReport(id)`
  - `getFlowseal()`, `setGameFilter(filter)`, `setIpsetMode(mode)`, `setCheckUpdates(enabled)`, `replaceFake(kind, source)`, `addListEntry(list, entry)`, `removeListEntry(list, entry)`, `updateIpset()`, `hostsPlan()`, `applyHosts()`, `checkFlowsealUpdate()`, `systemChecks()`, `fixCheck(action)`, `clearDiscordCache()`, `removeService()`, `quickCheck()`, `takeNotices()` (без Result)
  - события: `events.testProgress` (полезная нагрузка — `Progress`), `events.testsFinished` (`{ run_id: string | null }`), `events.stateChanged`, `events.noticesChanged`

Как устроены изменения: каждая команда, которая меняет zapret, проходит через `AppState::change` — одна очередь, во время тестов отказ с понятным текстом. Смена стратегии, игровой фильтр, режим ipset, удаление службы и сохранение активной своей стратегии идут через `change_safely`: снимок в `data/state/pending.json` до первого изменения, откат при ошибке (спецификация 10). Прогон тестов держит очередь от начала до конца и делает снимок сам (`tester::run::run`). После любого изменения `actions::changed` шлёт `StateChanged`: страница перечитывает то, что показывает.

Что перезапускает службу (как `service.bat`): игровой фильтр зашит в аргументы службы — `reinstall_active`; режим ipset и фейки — `restart_if_running`; свои списки и обновление `ipset-all` — без перезапуска, winws перечитывает списки сам.

- [ ] **Шаг 1: Зависимости**

В `src-tauri/Cargo.toml` секцию `[dependencies]` заменить и добавить `[dev-dependencies]` (в задаче 13 её не было):

```toml
[dependencies]
exec.workspace = true
probe.workspace = true
tester.workspace = true
zapret.workspace = true
anyhow.workspace = true
log.workspace = true
serde.workspace = true
specta.workspace = true
specta-typescript.workspace = true
tauri.workspace = true
tauri-plugin-dialog.workspace = true
tauri-plugin-single-instance.workspace = true
tauri-specta.workspace = true
toml.workspace = true
windows = { workspace = true, features = ["Data_Xml_Dom", "UI_Notifications", "Win32_UI_WindowsAndMessaging"] }
windows-registry.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

- [ ] **Шаг 2: Падающие тесты**

Тесты лежат внизу модулей: модули приватные, интеграционные тесты их не видят. Создайте файлы с `use` и блоком тестов; реализация встанет над тестами в шаге 4.

`src-tauri/src/settings.rs`, блок тестов:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_gives_the_defaults() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(load(&dir.path().join("zapret-hub.toml")).unwrap(), Settings::default());
        assert!(Settings::default().start_minimized);
    }

    #[test]
    fn settings_survive_a_save() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zapret-hub.toml");
        let settings = Settings {
            zapret_dir: Some(r"E:\Zapret".into()),
            socks5: Some("127.0.0.1:18082".into()),
            autostart: true,
            start_minimized: false,
            theme: Theme::Dark,
            wizard_done: true,
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path).unwrap(), settings);
        assert!(std::fs::read_to_string(&path).unwrap().contains("theme = \"dark\""));
    }

    #[test]
    fn a_damaged_file_is_set_aside() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("zapret-hub.toml");
        std::fs::write(&path, "zapret_dir = [").unwrap();
        let (settings, notice) = load_or_default(&path);
        assert_eq!(settings, Settings::default());
        assert!(notice.unwrap().contains("zapret-hub.toml.broken"));
        assert!(dir.path().join("zapret-hub.toml.broken").exists());
        assert!(!path.exists());
    }

    #[test]
    fn socks_addresses_are_an_ip_and_a_port() {
        assert_eq!(validate_socks(" 127.0.0.1:2080 ").unwrap().as_deref(), Some("127.0.0.1:2080"));
        assert_eq!(validate_socks("").unwrap(), None);
        assert!(validate_socks("localhost:2080").is_err());
        assert!(validate_socks("127.0.0.1").is_err());
    }
}
```

`src-tauri/src/state.rs`, блок тестов (без папки zapret и без службы: снимок и откат проверены в задаче 10, здесь — очередь и уведомления):

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn state(dir: &std::path::Path) -> AppState {
        AppState::new(Paths { exe_dir: dir.to_path_buf() }, Settings::default())
    }

    fn notice(level: NoticeLevel, title: &str) -> Notice {
        Notice { level, title: title.into(), text: String::new() }
    }

    #[test]
    fn notices_come_errors_first_and_only_once() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        state.push_notice(notice(NoticeLevel::Info, "a"));
        state.push_notice(notice(NoticeLevel::Error, "b"));
        state.push_notice(notice(NoticeLevel::Info, "c"));
        let titles: Vec<String> = state.take_notices().into_iter().map(|n| n.title).collect();
        assert_eq!(titles, ["b", "a", "c"]);
        assert!(state.take_notices().is_empty());
    }

    #[test]
    fn changes_need_a_folder_and_wait_out_a_test_run() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        let err = state.change(|_| Ok(())).unwrap_err();
        assert!(format!("{err:#}").contains("folder is not set"), "{err:#}");
        assert!(state.begin_testing());
        assert!(!state.begin_testing(), "a second run must not start");
        let err = state.change(|_| Ok(())).unwrap_err();
        assert!(format!("{err:#}").contains("test run"), "{err:#}");
        state.end_testing();
        assert!(!state.testing());
    }
}
```

`src-tauri/src/journal.rs`, блок тестов:

```rust
#[cfg(test)]
mod tests {
    use log::Log as _;

    use super::*;

    #[test]
    fn records_go_to_the_days_file_without_foreign_chatter() {
        let dir = tempfile::tempdir().unwrap();
        let journal = Journal::new(dir.path().join("logs"));
        let record = |target: &str, level: log::Level, text: &str| {
            journal.log(&log::Record::builder().args(format_args!("{text}")).level(level).target(target).build());
        };
        record("zapret::service", log::Level::Info, "sc start zapret");
        record("wry::webview", log::Level::Info, "webview chatter");
        record("tao", log::Level::Warn, "tao trouble");
        let day = dir.path().join("logs").join(format!("{}.log", exec::LocalTime::now().date()));
        let text = std::fs::read_to_string(day).unwrap();
        assert!(text.contains("INFO  [zapret::service] sc start zapret"), "{text}");
        assert!(text.contains("WARN  [tao] tao trouble"), "{text}");
        assert!(!text.contains("chatter"), "{text}");
    }
}
```

`src-tauri/src/autostart.rs`, блок тестов:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_task_runs_this_exe_at_logon_with_the_highest_rights() {
        let exe = Path::new(r"C:\Tools & Apps\Zapret Hub\zapret-hub.exe");
        let xml = task_xml(r"PC\me", exe);
        assert!(xml.contains(r"<Command>C:\Tools &amp; Apps\Zapret Hub\zapret-hub.exe</Command>"), "{xml}");
        assert!(xml.contains(r"<WorkingDirectory>C:\Tools &amp; Apps\Zapret Hub</WorkingDirectory>"), "{xml}");
        assert!(xml.contains(r"<UserId>PC\me</UserId>"), "{xml}");
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"), "{xml}");
        assert!(xml.contains("<Arguments>--autostart</Arguments>"), "{xml}");
        assert_eq!(command_of(&xml).as_deref(), Some(r"C:\Tools & Apps\Zapret Hub\zapret-hub.exe"));
    }
}
```

В конец `src-tauri/src/texts.rs`:

```rust
#[cfg(test)]
mod tests {
    use tester::model::{Score, StrategyResult};

    use super::*;

    fn result(name: &str, launch_error: Option<&str>) -> StrategyResult {
        StrategyResult {
            name: name.into(),
            args_sha256: String::new(),
            launch_error: launch_error.map(str::to_owned),
            targets: Vec::new(),
            dpi: Vec::new(),
            score: Score { ok: 35, total: 36, median_ms: Some(120), ..Score::default() },
        }
    }

    fn run(strategies: Vec<StrategyResult>) -> TestRun {
        TestRun {
            id: "2026-10-10_05-03-34".into(),
            suite: Suite::Standard,
            flowseal_version: None,
            canceled: false,
            restore_error: None,
            strategies,
        }
    }

    #[test]
    fn the_tests_notification_names_the_best_strategy() {
        assert_eq!(
            tests_done(&run(vec![result("general (ALT11)", None)])),
            "Лучшая: general (ALT11) — не прошло 1 из 36 проверок, задержка 120 мс"
        );
        assert_eq!(
            tests_done(&run(vec![result("general", Some("exit 1"))])),
            "Ни одна стратегия не запустилась: подробности на вкладке «Тесты»"
        );
        assert_eq!(tests_done(&run(Vec::new())), "Ни одна стратегия не проверена");
    }
}
```

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p zapret-hub --lib`
Expected: FAIL при компиляции: модулей нет в `lib.rs`, функций `load`, `task_xml`, `Journal` и других нет.

- [ ] **Шаг 4: Настройки, состояние, журнал, DTO, строки**

`src-tauri/src/settings.rs`, над тестами:

```rust
//! `zapret-hub.toml` next to the exe (spec 4.3).

use std::net::SocketAddr;
use std::path::Path;

use anyhow::Context as _;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    #[default]
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
#[serde(default)]
pub struct Settings {
    pub zapret_dir: Option<String>,
    /// `127.0.0.1:2080`: checked on the overview, used when a download fails directly.
    pub socks5: Option<String>,
    pub autostart: bool,
    /// A logon start stays in the tray.
    pub start_minimized: bool,
    pub theme: Theme,
    pub wizard_done: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            zapret_dir: None,
            socks5: None,
            autostart: false,
            start_minimized: true,
            theme: Theme::System,
            wizard_done: false,
        }
    }
}

pub fn load(path: &Path) -> anyhow::Result<Settings> {
    match std::fs::read_to_string(path) {
        Ok(text) => toml::from_str(&text).with_context(|| format!("{} is damaged", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(err) => Err(err).with_context(|| format!("cannot read {}", path.display())),
    }
}

/// Through a temporary file: a crash mid-write leaves the old settings, not half of the new.
pub fn save(path: &Path, settings: &Settings) -> anyhow::Result<()> {
    let temp = path.with_extension("toml.tmp");
    std::fs::write(&temp, toml::to_string(settings)?).with_context(|| format!("cannot write {}", temp.display()))?;
    std::fs::rename(&temp, path).with_context(|| format!("cannot replace {}", path.display()))
}

/// A damaged file is set aside as `.broken` and the defaults are used: the app still starts, and
/// the second value says what happened.
pub fn load_or_default(path: &Path) -> (Settings, Option<String>) {
    let err = match load(path) {
        Ok(settings) => return (settings, None),
        Err(err) => err,
    };
    let broken = path.with_extension("toml.broken");
    let kept = match std::fs::rename(path, &broken) {
        Ok(()) => crate::texts::settings_kept(&broken),
        Err(rename) => format!("{}: {rename}", broken.display()),
    };
    (Settings::default(), Some(format!("{kept}\n\n{err:#}")))
}

/// A SOCKS5 address the way local clients listen: `IP:port`. Blank means none.
pub fn validate_socks(text: &str) -> anyhow::Result<Option<String>> {
    let text = text.trim();
    if text.is_empty() {
        return Ok(None);
    }
    let addr: SocketAddr = text.parse().map_err(|_| anyhow::anyhow!("{text} is not an address like 127.0.0.1:2080"))?;
    Ok(Some(addr.to_string()))
}
```

`src-tauri/src/dto.rs`:

```rust
//! What the app layer sends to the page besides the crates' own types, and its events.

use tester::history::RunSummary;
use tester::model::StrategyResult;
use tester::run::{Progress, Request};
use zapret::fakes::Fakes;
use zapret::game_filter::GameFilter;
use zapret::ipset::IpsetMode;
use zapret::service::{QuickCheck, SvcState};
use zapret::user_lists::UserList;

/// The «Обзор» tab, read in one go.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Overview {
    /// `None` until the wizard sets it.
    pub folder: Option<String>,
    /// Why the folder cannot be used.
    pub folder_error: Option<String>,
    pub version: Option<String>,
    /// A newer Flowseal version, once a check found one.
    pub update: Option<String>,
    pub service: SvcState,
    pub windivert: SvcState,
    pub winws_running: bool,
    pub strategy: Option<String>,
    pub quick: Option<QuickCheck>,
    pub last_run: Option<RunSummary>,
    pub testing: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct ListState {
    pub list: UserList,
    pub entries: Vec<String>,
}

/// The settings `service.bat` keeps in the folder: the «Zapret» tab.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FlowsealState {
    pub game_filter: GameFilter,
    pub ipset: IpsetMode,
    pub ipset_backup: bool,
    pub check_updates: bool,
    pub fakes: Fakes,
    pub lists: Vec<ListState>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct FolderCheck {
    pub path: String,
    /// Empty for a usable Flowseal folder.
    pub missing: Vec<String>,
    pub version: Option<String>,
}

/// What the wizard offers (spec 9.2).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Candidates {
    pub folders: Vec<FolderCheck>,
    /// `127.0.0.1:<port>` that answer a SOCKS5 greeting now.
    pub proxies: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum NoticeLevel {
    Error,
    Info,
}

/// Something the user should see even if the window was closed when it happened.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct Notice {
    pub level: NoticeLevel,
    pub title: String,
    pub text: String,
}

/// The run in progress, for a page opened in the middle of it.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize, specta::Type)]
pub struct LiveRun {
    pub request: Request,
    /// The strategy under test; `None` before the first and while restoring.
    pub current: Option<u32>,
    pub restoring: bool,
    pub results: Vec<StrategyResult>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type)]
pub enum FolderKind {
    Zapret,
    Logs,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct TestProgress(pub Progress);

/// The run is over and zapret is back; `run_id` is `None` when it failed before a result.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct TestsFinished {
    pub run_id: Option<String>,
}

/// zapret, the settings or the history changed: the page reads what it shows again.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct StateChanged;

/// `take_notices` has something.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
pub struct NoticesChanged;
```

`src-tauri/src/state.rs` целиком, над тестами:

```rust
//! Where the app keeps its files (spec 4.3) and what it holds while it runs.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};

use anyhow::Context as _;
use tester::history::RunSummary;
use tester::model::TestRun;
use zapret::custom::Store;
use zapret::folder::Folder;
use zapret::service::QuickCheck;
use zapret::snapshot::{Snapshot, pending_path};

use crate::dto::{LiveRun, Notice, NoticeLevel};
use crate::settings::Settings;

/// Everything lives next to the exe: the app is portable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Paths {
    pub exe_dir: PathBuf,
}

impl Paths {
    pub fn current() -> anyhow::Result<Self> {
        let exe = std::env::current_exe().context("cannot find the executable's path")?;
        let exe_dir = exe.parent().context("the executable has no folder")?.to_path_buf();
        Ok(Self { exe_dir })
    }

    pub fn settings(&self) -> PathBuf {
        self.exe_dir.join("zapret-hub.toml")
    }

    pub fn data(&self) -> PathBuf {
        self.exe_dir.join("data")
    }

    pub fn strategies(&self) -> PathBuf {
        self.data().join("strategies")
    }

    pub fn logs(&self) -> PathBuf {
        self.data().join("logs")
    }
}

/// A poisoned lock still holds good data: a panic elsewhere must not take the app down.
pub fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

const TESTING: &str = "a test run is going: wait for it or cancel it";

pub struct AppState {
    pub paths: Paths,
    settings: Mutex<Settings>,
    /// Held by every change to zapret and by a whole test run: one at a time (spec 10).
    queue: Mutex<()>,
    testing: AtomicBool,
    cancel: AtomicBool,
    live: Mutex<Option<LiveRun>>,
    quick: Mutex<Option<QuickCheck>>,
    /// The newest finished standard run: «Применить лучшую», the overview, the tray.
    ranking: Mutex<Option<TestRun>>,
    /// A Flowseal version newer than the folder's.
    update: Mutex<Option<String>>,
    notices: Mutex<Vec<Notice>>,
}

impl AppState {
    pub fn new(paths: Paths, settings: Settings) -> Self {
        Self {
            paths,
            settings: Mutex::new(settings),
            queue: Mutex::new(()),
            testing: AtomicBool::new(false),
            cancel: AtomicBool::new(false),
            live: Mutex::new(None),
            quick: Mutex::new(None),
            ranking: Mutex::new(None),
            update: Mutex::new(None),
            notices: Mutex::new(Vec::new()),
        }
    }

    pub fn settings(&self) -> Settings {
        lock(&self.settings).clone()
    }

    pub fn set_settings(&self, settings: Settings) {
        *lock(&self.settings) = settings;
    }

    pub fn socks5(&self) -> Option<String> {
        lock(&self.settings).socks5.clone()
    }

    pub fn folder(&self) -> anyhow::Result<Folder> {
        let dir = lock(&self.settings)
            .zapret_dir
            .clone()
            .context("the zapret folder is not set: choose it in Settings")?;
        Folder::open(dir)
    }

    pub fn store(&self) -> Store {
        Store::new(self.paths.strategies())
    }

    pub fn testing(&self) -> bool {
        self.testing.load(Ordering::SeqCst)
    }

    /// Marks a test run as going; false when one already is.
    pub fn begin_testing(&self) -> bool {
        let began = self.testing.compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst).is_ok();
        if began {
            self.cancel.store(false, Ordering::SeqCst);
        }
        began
    }

    pub fn end_testing(&self) {
        self.testing.store(false, Ordering::SeqCst);
    }

    pub fn cancel_tests(&self) {
        self.cancel.store(true, Ordering::SeqCst);
    }

    pub fn cancel_flag(&self) -> &AtomicBool {
        &self.cancel
    }

    /// The queue's turn. A test run holds it from start to end.
    pub fn turn(&self) -> MutexGuard<'_, ()> {
        lock(&self.queue)
    }

    /// A change to zapret: one at a time, never during a test run.
    pub fn change<T>(&self, f: impl FnOnce(&Folder) -> anyhow::Result<T>) -> anyhow::Result<T> {
        anyhow::ensure!(!self.testing(), TESTING);
        let _turn = self.turn();
        f(&self.folder()?)
    }

    /// A change that could leave zapret half done: the state goes to `pending.json` before the
    /// first step and is put back if any step fails (spec 10). `operation` is the id
    /// `texts::operation` names.
    pub fn change_safely<T>(
        &self,
        operation: &str,
        f: impl FnOnce(&Folder) -> anyhow::Result<T>,
    ) -> anyhow::Result<T> {
        self.change(|folder| {
            let pending = pending_path(&self.paths.data());
            let snapshot = Snapshot::take(folder, operation);
            snapshot.save(&pending)?;
            let result = f(folder);
            if let Err(err) = &result {
                log::error!("{operation} failed, restoring the previous state: {err:#}");
                if let Err(undo) = snapshot.restore(folder) {
                    // pending.json stays: the next start tries again.
                    return Err(anyhow::anyhow!("{err:#}\n\nthe previous state was not restored either: {undo:#}"));
                }
            }
            Snapshot::clear(&pending)?;
            result
        })
    }

    pub fn live(&self) -> Option<LiveRun> {
        lock(&self.live).clone()
    }

    pub fn set_live(&self, live: Option<LiveRun>) {
        *lock(&self.live) = live;
    }

    pub fn update_live(&self, f: impl FnOnce(&mut LiveRun)) {
        if let Some(live) = lock(&self.live).as_mut() {
            f(live);
        }
    }

    pub fn quick(&self) -> Option<QuickCheck> {
        lock(&self.quick).clone()
    }

    /// Returns the check it replaces.
    pub fn set_quick(&self, check: QuickCheck) -> Option<QuickCheck> {
        lock(&self.quick).replace(check)
    }

    pub fn set_ranking(&self, run: Option<TestRun>) {
        *lock(&self.ranking) = run;
    }

    pub fn last_run(&self) -> Option<RunSummary> {
        lock(&self.ranking).as_ref().map(tester::history::summary)
    }

    /// The first `n` of the ranking that started at all.
    pub fn best(&self, n: usize) -> Vec<String> {
        lock(&self.ranking)
            .as_ref()
            .map(|run| {
                run.strategies.iter().filter(|s| s.launch_error.is_none()).take(n).map(|s| s.name.clone()).collect()
            })
            .unwrap_or_default()
    }

    pub fn update(&self) -> Option<String> {
        lock(&self.update).clone()
    }

    pub fn set_update(&self, version: Option<String>) {
        *lock(&self.update) = version;
    }

    pub fn push_notice(&self, notice: Notice) {
        lock(&self.notices).push(notice);
    }

    /// Everything queued, errors first (spec 10).
    pub fn take_notices(&self) -> Vec<Notice> {
        let mut notices = std::mem::take(&mut *lock(&self.notices));
        // Stable: within a level the order they came in.
        notices.sort_by_key(|notice| notice.level != NoticeLevel::Error);
        notices
    }
}
```

`src-tauri/src/journal.rs`, над тестами:

```rust
//! `log` records into `data/logs/YYYY-MM-DD.log` (spec 10): ours from info up, the rest from warn.

use std::fs::{File, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use crate::state::lock;

/// Targets logged from info: the crates and the app (`zapret` also covers `zapret_hub_lib`).
/// Tauri's and the webview's chatter is not.
const OURS: [&str; 4] = ["exec", "probe", "zapret", "tester"];

pub struct Journal {
    dir: PathBuf,
    /// The day's file, opened on first use and again when the date changes.
    file: Mutex<Option<(String, File)>>,
}

impl Journal {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir, file: Mutex::new(None) }
    }
}

impl log::Log for Journal {
    fn enabled(&self, metadata: &log::Metadata) -> bool {
        metadata.level() <= log::Level::Warn
            || (metadata.level() <= log::Level::Info && OURS.iter().any(|ours| metadata.target().starts_with(ours)))
    }

    fn log(&self, record: &log::Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let now = exec::LocalTime::now();
        let date = now.date();
        let line = format!("{} {:<5} [{}] {}\n", now.clock(), record.level(), record.target(), record.args());
        let mut file = lock(&self.file);
        if file.as_ref().is_none_or(|(day, _)| *day != date) {
            *file = open(&self.dir, &date).map(|opened| (date, opened));
        }
        if let Some((_, opened)) = file.as_mut() {
            let _ = opened.write_all(line.as_bytes());
        }
        if cfg!(debug_assertions) {
            eprint!("{line}");
        }
    }

    fn flush(&self) {}
}

fn open(dir: &Path, date: &str) -> Option<File> {
    std::fs::create_dir_all(dir).ok()?;
    OpenOptions::new().create(true).append(true).open(dir.join(format!("{date}.log"))).ok()
}

/// First thing in `run`: everything after it is journaled.
pub fn init(dir: PathBuf) {
    if log::set_logger(Box::leak(Box::new(Journal::new(dir)))).is_ok() {
        log::set_max_level(log::LevelFilter::Info);
    }
}
```

`src-tauri/src/texts.rs` целиком, над тестами:

```rust
//! The Russian text the backend shows itself: message boxes, the tray, notifications, notices.
//! The page's text lives in `frontend/src/lib/strings.ts`.

use std::path::Path;

use tester::model::{Suite, TestRun};

pub const APP: &str = "Zapret Hub";

pub const FAILED_TO_START: &str = "Zapret Hub не запустился.";

pub const WEBVIEW2_MISSING: &str = "Для окна Zapret Hub нужен Microsoft Edge WebView2 Runtime, а на этом \
компьютере его нет.\n\nСкачайте его: https://go.microsoft.com/fwlink/p/?LinkId=2124703\n\n\
Ctrl+C копирует этот текст.";

pub const PICK_FOLDER: &str = "Папка zapret";

pub const SETTINGS_RESET: &str = "Настройки сброшены";

pub fn settings_kept(broken: &Path) -> String {
    format!("Файл настроек повреждён, он сохранён как {}.", broken.display())
}

pub const RECOVERED: &str = "Прерванная операция отменена";

pub fn recovered(operation: &str) -> String {
    format!("Прошлый запуск не довёл до конца операцию «{operation}». Служба, ipset и запущенные вручную winws возвращены как были.")
}

pub const RECOVERY_FAILED: &str = "Состояние после прерванной операции не вернулось";

/// The operation ids `pending.json` keeps, as the user knows them.
pub fn operation(id: &str) -> &str {
    match id {
        "apply" => "смена стратегии",
        "custom" => "сохранение своей стратегии",
        "game_filter" => "смена игрового фильтра",
        "ipset" => "смена режима ipset",
        "remove_service" => "удаление службы",
        "tests" => "тесты стратегий",
        other => other,
    }
}

pub const PREPARE_FAILED: &str = "Папка zapret не подготовлена";
pub const FILES_RESTORED: &str = "Файлы своих стратегий выложены заново";
pub const AUTOSTART_FAILED: &str = "Автозапуск не обновлён";
pub const HISTORY_FAILED: &str = "История тестов не прочитана";
pub const RESTORE_AFTER_TESTS: &str = "После тестов не всё вернулось на место";

pub const TESTS_DONE: &str = "Тесты завершены";
pub const TESTS_CANCELED: &str = "Тесты отменены";
pub const TESTS_FAILED: &str = "Тесты не выполнены";

pub fn tests_done(run: &TestRun) -> String {
    let Some(best) = run.strategies.first() else {
        return "Ни одна стратегия не проверена".to_owned();
    };
    if best.launch_error.is_some() {
        return "Ни одна стратегия не запустилась: подробности на вкладке «Тесты»".to_owned();
    }
    let score = &best.score;
    let tail = match run.suite {
        Suite::Standard => format!(
            "не прошло {} из {} проверок, задержка {}",
            score.not_ok(),
            score.total,
            score.median_ms.map_or_else(|| "—".to_owned(), |ms| format!("{ms} мс"))
        ),
        Suite::Dpi => format!("похоже на блокировку {}, сбоев {}", score.likely_blocked, score.fail),
    };
    format!("Лучшая: {} — {tail}", best.name)
}
```

Строки быстрой проверки, обновления и трея добавит задача 15: пока их никто не вызывает, а `clippy -D warnings` не пропускает неиспользуемый код.

- [ ] **Шаг 5: Автозапуск и уведомления**

`src-tauri/src/autostart.rs`, над тестами:

```rust
//! The Task Scheduler entry «Zapret Hub»: at logon, with the highest rights, so the elevated app
//! starts without a UAC prompt (spec 4.2).

use std::path::Path;
use std::process::Command;
use std::time::Duration;

pub const TASK: &str = "Zapret Hub";
const TIMEOUT: Duration = Duration::from_secs(30);

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

pub fn task_xml(user: &str, exe: &Path) -> String {
    let command = escape(&exe.display().to_string());
    let dir = escape(&exe.parent().map(|dir| dir.display().to_string()).unwrap_or_default());
    let user = escape(user);
    format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.2" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>Zapret Hub at logon</Description>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>false</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>4</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{command}</Command>
      <Arguments>--autostart</Arguments>
      <WorkingDirectory>{dir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    )
}

/// The exe a task's XML starts.
pub fn command_of(xml: &str) -> Option<String> {
    let start = xml.find("<Command>")? + "<Command>".len();
    let end = start + xml[start..].find("</Command>")?;
    Some(xml[start..end].replace("&lt;", "<").replace("&gt;", ">").replace("&amp;", "&"))
}

fn schtasks(args: &[&std::ffi::OsStr]) -> anyhow::Result<exec::Output> {
    let mut cmd = Command::new("schtasks.exe");
    cmd.args(args);
    Ok(exec::run(&mut cmd, TIMEOUT, None)?)
}

pub fn enable(exe: &Path) -> anyhow::Result<()> {
    let user = format!(
        "{}\\{}",
        std::env::var("USERDOMAIN").unwrap_or_default(),
        std::env::var("USERNAME").unwrap_or_default()
    );
    // schtasks reads the file as the declaration says: UTF-16 with a byte order mark.
    let file = std::env::temp_dir().join("zapret-hub-task.xml");
    let mut bytes = vec![0xFF, 0xFE];
    bytes.extend(task_xml(&user, exe).encode_utf16().flat_map(u16::to_le_bytes));
    std::fs::write(&file, bytes)?;
    let created = schtasks(&["/Create".as_ref(), "/TN".as_ref(), TASK.as_ref(), "/XML".as_ref(), file.as_os_str(), "/F".as_ref()]);
    let _ = std::fs::remove_file(&file);
    created?.check()?;
    Ok(())
}

pub fn disable() -> anyhow::Result<()> {
    let query = schtasks(&["/Query".as_ref(), "/TN".as_ref(), TASK.as_ref()])?;
    if query.success() {
        schtasks(&["/Delete".as_ref(), "/TN".as_ref(), TASK.as_ref(), "/F".as_ref()])?.check()?;
    }
    Ok(())
}

/// The exe moved since the task was made, or the task is gone: it is made again for `exe`.
pub fn refresh(exe: &Path) -> anyhow::Result<()> {
    let query = schtasks(&["/Query".as_ref(), "/TN".as_ref(), TASK.as_ref(), "/XML".as_ref()])?;
    if query.success() && command_of(&query.stdout).is_some_and(|command| Path::new(&command) == exe) {
        return Ok(());
    }
    enable(exe)
}
```

`src-tauri/src/notify.rs`:

```rust
//! Windows notifications (spec 9.3) under the AppUserModelID `ZapretHub`.

use tauri::AppHandle;
use windows::Data::Xml::Dom::XmlDocument;
use windows::UI::Notifications::{ToastNotification, ToastNotificationManager};
use windows::core::HSTRING;

const AUMID: &str = "ZapretHub";

/// An unpackaged exe has no identity for notifications: this names one. No `IconUri`: the shell
/// draws a registered icon inverted and caches it.
pub fn register() {
    let key = windows_registry::CURRENT_USER.create(format!(r"Software\Classes\AppUserModelId\{AUMID}"));
    if let Err(err) = key.and_then(|key| key.set_string("DisplayName", crate::texts::APP)) {
        log::warn!("cannot register the notification sender: {err}");
    }
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

pub fn show(app: &AppHandle, title: &str, body: &str) {
    log::info!("notification: {title}: {body}");
    let xml = format!(
        r#"<toast><visual><binding template="ToastGeneric"><text>{}</text><text>{}</text></binding></visual></toast>"#,
        escape(title),
        escape(body)
    );
    // WinRT wants an initialized apartment: the main thread has one.
    let dispatched = app.run_on_main_thread(move || {
        let shown = (|| -> windows::core::Result<()> {
            let document = XmlDocument::new()?;
            document.LoadXml(&HSTRING::from(xml))?;
            let toast = ToastNotification::CreateToastNotification(&document)?;
            ToastNotificationManager::CreateToastNotifierWithId(&HSTRING::from(AUMID))?.Show(&toast)
        })();
        if let Err(err) = shown {
            log::warn!("cannot show a notification: {err}");
        }
    });
    if let Err(err) = dispatched {
        log::warn!("cannot show a notification: {err}");
    }
}
```

- [ ] **Шаг 6: Общие действия**

`src-tauri/src/actions.rs`:

```rust
//! What commands, the tray and background jobs share.

use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use tauri::{AppHandle, Manager as _};
use tauri_specta::Event as _;
use zapret::custom::CustomStrategy;
use zapret::folder::Folder;
use zapret::service::{QuickCheck, SERVICE};
use zapret::user_lists::UserList;

use crate::dto::{
    Candidates, FlowsealState, FolderCheck, FolderKind, ListState, Notice, NoticeLevel, NoticesChanged, Overview,
    StateChanged,
};
use crate::settings::Settings;
use crate::state::AppState;
use crate::texts;

pub fn state(app: &AppHandle) -> tauri::State<'_, AppState> {
    app.state::<AppState>()
}

/// Something to show even with the window closed: it waits in the queue (spec 10).
pub fn notice(app: &AppHandle, level: NoticeLevel, title: &str, text: String) {
    log::info!("notice: {title}: {text}");
    state(app).push_notice(Notice { level, title: title.to_owned(), text });
    if let Err(err) = NoticesChanged.emit(app) {
        log::warn!("cannot announce a notice: {err}");
    }
}

/// zapret or the settings changed: the page reads them again.
pub fn changed(app: &AppHandle) {
    if let Err(err) = StateChanged.emit(app) {
        log::warn!("cannot announce a change: {err}");
    }
}

/// A change that ends in a quick check: the check feeds the tray and the overview.
fn finish_with_check(app: &AppHandle, result: anyhow::Result<QuickCheck>) -> anyhow::Result<QuickCheck> {
    if let Ok(check) = &result {
        state(app).set_quick(check.clone());
    }
    changed(app);
    result
}

/// «Применить» (spec 5.4).
pub fn apply(app: &AppHandle, name: &str) -> anyhow::Result<QuickCheck> {
    let result = state(app).change_safely("apply", |folder| zapret::service::apply(folder, name));
    finish_with_check(app, result)
}

/// «Применить лучшую»: the first of the newest standard run (spec 7.4).
pub fn apply_best(app: &AppHandle) -> anyhow::Result<QuickCheck> {
    let best = state(app).best(1).pop().ok_or_else(|| anyhow::anyhow!("no test results yet: run the standard tests first"))?;
    apply(app, &best)
}

pub fn restart(app: &AppHandle) -> anyhow::Result<QuickCheck> {
    let result = state(app).change(|folder| {
        zapret::service::restart(SERVICE)?;
        // winws needs a moment before it intercepts anything (spec 5.4).
        std::thread::sleep(Duration::from_secs(2));
        Ok(zapret::service::quick_check(folder))
    });
    finish_with_check(app, result)
}

/// Reads only, but during a test run it would check the strategy under test.
pub fn quick_check(app: &AppHandle) -> anyhow::Result<QuickCheck> {
    let state = state(app);
    anyhow::ensure!(!state.testing(), "a test run is going: wait for it or cancel it");
    let result = state.folder().map(|folder| zapret::service::quick_check(&folder));
    finish_with_check(app, result)
}

pub fn check_flowseal_update(app: &AppHandle) -> anyhow::Result<Option<String>> {
    let state = state(app);
    let newer = zapret::updates::newer_version(&state.folder()?, state.socks5().as_deref())?;
    state.set_update(newer.clone());
    changed(app);
    Ok(newer)
}

pub fn save_settings(app: &AppHandle, mut settings: Settings) -> anyhow::Result<()> {
    let state = state(app);
    let old = state.settings();
    settings.socks5 = crate::settings::validate_socks(settings.socks5.as_deref().unwrap_or_default())?;
    let folder_changed = settings.zapret_dir != old.zapret_dir;
    if folder_changed {
        anyhow::ensure!(!state.testing(), "a test run is going: wait for it or cancel it");
        if let Some(dir) = &settings.zapret_dir {
            Folder::open(dir)?;
        }
    }
    if settings.autostart != old.autostart {
        if settings.autostart {
            crate::autostart::enable(&std::env::current_exe()?)?;
        } else {
            crate::autostart::disable()?;
        }
    }
    crate::settings::save(&state.paths.settings(), &settings)?;
    state.set_settings(settings);
    if folder_changed {
        prepare_folder(app);
    }
    changed(app);
    Ok(())
}

/// The active strategy's arguments are baked into the service: saving it installs it again.
pub fn save_custom(app: &AppHandle, strategy: CustomStrategy, create: bool) -> anyhow::Result<Option<QuickCheck>> {
    let state = state(app);
    let store = state.store();
    let active = zapret::service::strategy();
    if !create && active.is_some_and(|active| active.eq_ignore_ascii_case(&strategy.file_stem())) {
        let result = state.change_safely("custom", |folder| {
            let saved = zapret::custom::save(folder, &store, &strategy, create)?;
            zapret::service::apply(folder, &saved.file_stem())
        });
        return finish_with_check(app, result).map(Some);
    }
    let result = state.change(|folder| zapret::custom::save(folder, &store, &strategy, create));
    changed(app);
    result.map(|_| None)
}

/// A folder found at start or chosen in Settings: the user lists exist, and the custom strategies'
/// files are back after a Flowseal update (spec 5.5, 6).
pub fn prepare_folder(app: &AppHandle) {
    let state = state(app);
    let Ok(folder) = state.folder() else { return };
    if let Err(err) = zapret::user_lists::ensure_defaults(&folder) {
        notice(app, NoticeLevel::Error, texts::PREPARE_FAILED, format!("{err:#}"));
    }
    match zapret::custom::materialize_all(&folder, &state.store()) {
        Ok(restored) if !restored.is_empty() => notice(app, NoticeLevel::Info, texts::FILES_RESTORED, restored.join(", ")),
        Ok(_) => {}
        Err(err) => notice(app, NoticeLevel::Error, texts::PREPARE_FAILED, format!("{err:#}")),
    }
}

pub fn overview(app: &AppHandle) -> Overview {
    let state = state(app);
    let folder = state.folder();
    Overview {
        folder: state.settings().zapret_dir,
        folder_error: folder.as_ref().err().map(|err| format!("{err:#}")),
        version: folder.as_ref().ok().and_then(Folder::version),
        update: state.update(),
        service: zapret::service::state(SERVICE),
        windivert: zapret::service::state("WinDivert"),
        winws_running: zapret::procs::is_running("winws.exe"),
        strategy: zapret::service::strategy(),
        quick: state.quick(),
        last_run: state.last_run(),
        testing: state.testing(),
    }
}

pub fn flowseal(app: &AppHandle) -> anyhow::Result<FlowsealState> {
    let folder = state(app).folder()?;
    let lists = UserList::ALL
        .iter()
        .map(|&list| Ok(ListState { list, entries: zapret::user_lists::read(&folder, list)? }))
        .collect::<anyhow::Result<Vec<_>>>()?;
    Ok(FlowsealState {
        game_filter: zapret::game_filter::read(&folder),
        ipset: zapret::ipset::mode(&folder),
        ipset_backup: zapret::ipset::has_backup(&folder),
        check_updates: zapret::flags::check_updates(&folder),
        fakes: zapret::fakes::state(&folder)?,
        lists,
    })
}

pub fn check_folder(path: &Path) -> FolderCheck {
    FolderCheck {
        path: path.display().to_string(),
        missing: zapret::folder::missing_parts(path).into_iter().map(str::to_owned).collect(),
        version: Folder::open(path).ok().and_then(|folder| folder.version()),
    }
}

/// Folders where Flowseal probably is, and SOCKS5 ports that answer now: NekoBox's own port when
/// it runs, then the default ports of known clients (spec 9.2).
pub fn candidates() -> Candidates {
    let folders = zapret::folder::candidates().iter().map(|path| check_folder(path)).collect();
    let mut ports: Vec<u16> = ["nekobox.exe", "nekoray.exe"]
        .iter()
        .flat_map(|image| zapret::procs::find(image))
        .filter_map(|proc| proc.exe.as_deref().and_then(Path::parent).and_then(probe::socks::nekobox_port))
        .collect();
    ports.extend(probe::socks::KNOWN_PORTS);
    Candidates { folders, proxies: probe::socks::listening(&ports) }
}

/// Through explorer: it hands the path or the link to the user's own, unelevated shell.
fn explorer(target: &OsStr) -> anyhow::Result<()> {
    let mut cmd = Command::new("explorer.exe");
    cmd.arg(target);
    // explorer exits with 1 even when it opened the target: the code means nothing.
    exec::run(&mut cmd, Duration::from_secs(10), None)?;
    Ok(())
}

/// Release pages and the issues `service.bat` links to: https only.
pub fn open_url(url: &str) -> anyhow::Result<()> {
    anyhow::ensure!(url.starts_with("https://"), "only https links are opened: {url}");
    explorer(OsStr::new(url))
}

pub fn open_folder(app: &AppHandle, kind: FolderKind) -> anyhow::Result<()> {
    let state = state(app);
    let path = match kind {
        FolderKind::Zapret => state.folder()?.root().to_path_buf(),
        FolderKind::Logs => {
            let logs = state.paths.logs();
            std::fs::create_dir_all(&logs)?;
            logs
        }
    };
    explorer(path.as_os_str())
}
```

- [ ] **Шаг 7: Фоновые задачи: прогон тестов и старт**

`src-tauri/src/jobs.rs`:

```rust
//! Work that outlives a command: a test run, and what happens once at start.

use tauri::AppHandle;
use tauri_specta::Event as _;
use tester::model::{Suite, TestRun};
use tester::run::{Progress, Request};
use zapret::snapshot::{Snapshot, pending_path};

use crate::actions::{self, state};
use crate::dto::{LiveRun, NoticeLevel, TestProgress, TestsFinished};
use crate::{notify, texts};

/// Starts a run in its own thread; the page follows `TestProgress` and `TestsFinished`.
pub fn start_tests(app: &AppHandle, request: Request) -> anyhow::Result<()> {
    anyhow::ensure!(!request.strategies.is_empty(), "choose at least one strategy");
    let state = state(app);
    let folder = state.folder()?;
    anyhow::ensure!(state.begin_testing(), "a test run is already going");
    state.set_live(Some(LiveRun { request: request.clone(), current: None, restoring: false, results: Vec::new() }));
    let handle = app.clone();
    std::thread::spawn(move || {
        let state = actions::state(&handle);
        let outcome = {
            // The whole run is one turn of the queue: nothing else changes zapret meanwhile.
            let _turn = state.turn();
            let socks5 = state.socks5();
            tester::run::run(&folder, &state.paths.data(), &request, socks5.as_deref(), state.cancel_flag(), &|progress| {
                on_progress(&handle, progress)
            })
        };
        finish(&handle, outcome);
    });
    actions::changed(app);
    Ok(())
}

fn on_progress(app: &AppHandle, progress: Progress) {
    state(app).update_live(|live| match &progress {
        Progress::Strategy { index, .. } => live.current = Some(*index),
        Progress::Done { result } => live.results.push(result.clone()),
        Progress::Restoring => {
            live.current = None;
            live.restoring = true;
        }
    });
    if let Err(err) = TestProgress(progress).emit(app) {
        log::warn!("cannot report test progress: {err}");
    }
}

fn finish(app: &AppHandle, outcome: anyhow::Result<TestRun>) {
    let state = state(app);
    let run_id = match outcome {
        Ok(run) => {
            if let Some(err) = &run.restore_error {
                actions::notice(app, NoticeLevel::Error, texts::RESTORE_AFTER_TESTS, err.clone());
            }
            let title = if run.canceled { texts::TESTS_CANCELED } else { texts::TESTS_DONE };
            notify::show(app, title, &texts::tests_done(&run));
            let id = run.id.clone();
            if run.suite == Suite::Standard && !run.canceled {
                state.set_ranking(Some(run));
            }
            Some(id)
        }
        Err(err) => {
            let text = format!("{err:#}");
            notify::show(app, texts::TESTS_FAILED, &text);
            actions::notice(app, NoticeLevel::Error, texts::TESTS_FAILED, text);
            None
        }
    };
    state.set_live(None);
    state.end_testing();
    if let Err(err) = (TestsFinished { run_id }).emit(app) {
        log::warn!("cannot report the end of a test run: {err}");
    }
    actions::changed(app);
}

/// Called once from `setup`; the rest of the app does not wait for it.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || startup(&app));
}

/// Before anything else changes zapret, an interrupted operation is put back (spec 10).
fn startup(app: &AppHandle) {
    recover(app);
    actions::prepare_folder(app);
    let state = state(app);
    if state.settings().autostart {
        let refreshed = std::env::current_exe()
            .map_err(anyhow::Error::from)
            .and_then(|exe| crate::autostart::refresh(&exe));
        if let Err(err) = refreshed {
            actions::notice(app, NoticeLevel::Error, texts::AUTOSTART_FAILED, format!("{err:#}"));
        }
    }
    match tester::history::latest_ranking(&tester::history::dir(&state.paths.data())) {
        Ok(run) => state.set_ranking(run),
        Err(err) => actions::notice(app, NoticeLevel::Error, texts::HISTORY_FAILED, format!("{err:#}")),
    }
    actions::changed(app);
}

fn recover(app: &AppHandle) {
    let state = state(app);
    let pending = pending_path(&state.paths.data());
    let snapshot = match Snapshot::load(&pending) {
        Ok(Some(snapshot)) => snapshot,
        Ok(None) => return,
        Err(err) => {
            // Unreadable: set aside, or every start would trip over it.
            let broken = pending.with_extension("json.broken");
            let moved = match std::fs::rename(&pending, &broken) {
                Ok(()) => broken.display().to_string(),
                Err(rename) => rename.to_string(),
            };
            actions::notice(app, NoticeLevel::Error, texts::RECOVERY_FAILED, format!("{err:#}\n\n{moved}"));
            return;
        }
    };
    log::warn!("unfinished operation {} found, restoring", snapshot.operation);
    let restored = state.folder().and_then(|folder| {
        let _turn = state.turn();
        snapshot.restore(&folder)
    });
    match restored.and_then(|()| Snapshot::clear(&pending)) {
        Ok(()) => actions::notice(
            app,
            NoticeLevel::Info,
            texts::RECOVERED,
            texts::recovered(texts::operation(&snapshot.operation)),
        ),
        // pending.json stays: the next start tries again.
        Err(err) => actions::notice(app, NoticeLevel::Error, texts::RECOVERY_FAILED, format!("{err:#}")),
    }
}
```

- [ ] **Шаг 8: Команды IPC**

`src-tauri/src/commands.rs` (тело команды — передача аргументов; изменения идут через `actions` или `AppState::change`):

```rust
//! IPC commands: argument plumbing only. The work is in `actions`, `jobs` and the crates.

use std::path::Path;

use probe::socks::ProxyStatus;
use tauri::{AppHandle, Manager as _, State};
use tauri_plugin_dialog::DialogExt as _;
use tester::catalog::{StrategyDetail, StrategyRow};
use tester::history::RunSummary;
use tester::model::TestRun;
use tester::run::Request;
use zapret::checks::{CheckAction, SystemCheck};
use zapret::custom::CustomStrategy;
use zapret::discord_cache::CacheReport;
use zapret::fakes::FakeKind;
use zapret::game_filter::GameFilter;
use zapret::hosts::HostsPlan;
use zapret::ipset::IpsetMode;
use zapret::service::{QuickCheck, SERVICE};
use zapret::user_lists::UserList;

use crate::actions::{self, state as app};
use crate::dto::{Candidates, FlowsealState, FolderCheck, FolderKind, LiveRun, Notice, Overview};
use crate::settings::Settings;
use crate::state::AppState;
use crate::{texts, window};

type Answer<T> = Result<T, String>;

/// Every command touches processes, the disk or the network: it runs off the async runtime, and
/// its error reaches the page with the whole chain of causes.
async fn blocking<T: Send + 'static>(work: impl FnOnce() -> anyhow::Result<T> + Send + 'static) -> Answer<T> {
    match tauri::async_runtime::spawn_blocking(work).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(err)) => {
            log::warn!("command failed: {err:#}");
            Err(format!("{err:#}"))
        }
        Err(err) => Err(format!("the command did not finish: {err}")),
    }
}

fn data_tests(handle: &AppHandle) -> std::path::PathBuf {
    tester::history::dir(&app(handle).paths.data())
}

#[tauri::command]
#[specta::specta]
pub fn app_version() -> String {
    env!("CARGO_PKG_VERSION").to_owned()
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings()
}

#[tauri::command]
#[specta::specta]
pub async fn save_settings(handle: AppHandle, settings: Settings) -> Answer<()> {
    blocking(move || actions::save_settings(&handle, settings)).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_overview(handle: AppHandle) -> Answer<Overview> {
    blocking(move || Ok(actions::overview(&handle))).await
}

#[tauri::command]
#[specta::specta]
pub async fn check_proxy(addr: String) -> Answer<ProxyStatus> {
    blocking(move || Ok(probe::socks::check(&addr))).await
}

#[tauri::command]
#[specta::specta]
pub async fn wizard_candidates() -> Answer<Candidates> {
    blocking(|| Ok(actions::candidates())).await
}

#[tauri::command]
#[specta::specta]
pub async fn check_folder(path: String) -> Answer<FolderCheck> {
    blocking(move || Ok(actions::check_folder(Path::new(&path)))).await
}

#[tauri::command]
#[specta::specta]
pub async fn pick_folder(handle: AppHandle) -> Answer<Option<String>> {
    blocking(move || {
        let mut dialog = handle.dialog().file().set_title(texts::PICK_FOLDER);
        if let Some(window) = handle.get_webview_window(window::MAIN) {
            dialog = dialog.set_parent(&window);
        }
        let picked = dialog.blocking_pick_folder().map(|path| path.into_path()).transpose()?;
        Ok(picked.map(|path| path.display().to_string()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn open_url(url: String) -> Answer<()> {
    blocking(move || actions::open_url(&url)).await
}

#[tauri::command]
#[specta::specta]
pub async fn open_folder(handle: AppHandle, kind: FolderKind) -> Answer<()> {
    blocking(move || actions::open_folder(&handle, kind)).await
}

#[tauri::command]
#[specta::specta]
pub async fn list_strategies(handle: AppHandle) -> Answer<Vec<StrategyRow>> {
    blocking(move || {
        let state = app(&handle);
        let active = zapret::service::strategy();
        tester::catalog::rows(&state.folder()?, &state.store(), &state.paths.data(), active.as_deref())
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn get_strategy(handle: AppHandle, name: String) -> Answer<StrategyDetail> {
    blocking(move || {
        let state = app(&handle);
        let active = zapret::service::strategy();
        tester::catalog::detail(&state.folder()?, &state.store(), &state.paths.data(), active.as_deref(), &name)
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_strategy(handle: AppHandle, name: String) -> Answer<QuickCheck> {
    blocking(move || actions::apply(&handle, &name)).await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_best(handle: AppHandle) -> Answer<QuickCheck> {
    blocking(move || actions::apply_best(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub async fn restart_zapret(handle: AppHandle) -> Answer<QuickCheck> {
    blocking(move || actions::restart(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub async fn clone_strategy(handle: AppHandle, source: String, name: String) -> Answer<CustomStrategy> {
    blocking(move || {
        let state = app(&handle);
        let cloned = state.change(|folder| zapret::custom::clone_from(folder, &state.store(), &source, &name));
        actions::changed(&handle);
        cloned
    })
    .await
}

/// The editor's «Проверить»: the parser and `winws --dry-run`, nothing saved.
#[tauri::command]
#[specta::specta]
pub async fn validate_custom(handle: AppHandle, strategy: CustomStrategy) -> Answer<()> {
    blocking(move || app(&handle).change(|folder| zapret::custom::validate(folder, &strategy).map(|_| ()))).await
}

#[tauri::command]
#[specta::specta]
pub async fn save_custom(handle: AppHandle, strategy: CustomStrategy, create: bool) -> Answer<Option<QuickCheck>> {
    blocking(move || actions::save_custom(&handle, strategy, create)).await
}

/// `name` is the record's name, not the file's.
#[tauri::command]
#[specta::specta]
pub async fn delete_custom(handle: AppHandle, name: String) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let active = zapret::service::strategy();
        let deleted = state.change(|folder| zapret::custom::delete(folder, &state.store(), &name, active.as_deref()));
        actions::changed(&handle);
        deleted
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn start_tests(handle: AppHandle, request: Request) -> Answer<()> {
    blocking(move || crate::jobs::start_tests(&handle, request)).await
}

#[tauri::command]
#[specta::specta]
pub fn cancel_tests(state: State<'_, AppState>) {
    state.cancel_tests();
}

#[tauri::command]
#[specta::specta]
pub fn test_live(state: State<'_, AppState>) -> Option<LiveRun> {
    state.live()
}

#[tauri::command]
#[specta::specta]
pub async fn list_runs(handle: AppHandle) -> Answer<Vec<RunSummary>> {
    blocking(move || tester::history::summaries(&data_tests(&handle))).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_run(handle: AppHandle, id: String) -> Answer<TestRun> {
    blocking(move || tester::history::load(&data_tests(&handle), &id)).await
}

/// «Копировать»: the run as `test zapret.ps1` writes it.
#[tauri::command]
#[specta::specta]
pub async fn run_report(handle: AppHandle, id: String) -> Answer<String> {
    blocking(move || Ok(tester::report::to_text(&tester::history::load(&data_tests(&handle), &id)?))).await
}

#[tauri::command]
#[specta::specta]
pub async fn get_flowseal(handle: AppHandle) -> Answer<FlowsealState> {
    blocking(move || actions::flowseal(&handle)).await
}

/// The ports are baked into the service's arguments: it is installed again (spec 5.5).
#[tauri::command]
#[specta::specta]
pub async fn set_game_filter(handle: AppHandle, filter: GameFilter) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change_safely("game_filter", |folder| {
            zapret::game_filter::write(folder, &filter)?;
            zapret::service::reinstall_active(folder)
        });
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_ipset_mode(handle: AppHandle, mode: IpsetMode) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change_safely("ipset", |folder| {
            zapret::ipset::set_mode(folder, mode)?;
            zapret::service::restart_if_running(SERVICE)
        });
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn set_check_updates(handle: AppHandle, enabled: bool) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|folder| Ok(zapret::flags::set_check_updates(folder, enabled)?));
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn replace_fake(handle: AppHandle, kind: FakeKind, source: String) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|folder| {
            zapret::fakes::replace(folder, kind, &source)?;
            zapret::service::restart_if_running(SERVICE)
        });
        actions::changed(&handle);
        done
    })
    .await
}

/// winws reads the lists again by itself: no restart.
#[tauri::command]
#[specta::specta]
pub async fn add_list_entry(handle: AppHandle, list: UserList, entry: String) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|folder| zapret::user_lists::add(folder, list, &entry));
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_list_entry(handle: AppHandle, list: UserList, entry: String) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|folder| zapret::user_lists::remove(folder, list, &entry));
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn update_ipset(handle: AppHandle) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let socks5 = state.socks5();
        let done = state.change(|folder| zapret::updates::update_ipset(folder, socks5.as_deref()));
        actions::changed(&handle);
        done
    })
    .await
}

/// What applying would change in hosts; nothing is written.
#[tauri::command]
#[specta::specta]
pub async fn hosts_plan(handle: AppHandle) -> Answer<HostsPlan> {
    blocking(move || zapret::updates::hosts_plan(app(&handle).socks5().as_deref())).await
}

#[tauri::command]
#[specta::specta]
pub async fn apply_hosts(handle: AppHandle) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let socks5 = state.socks5();
        state.change(|_| zapret::updates::apply_hosts(socks5.as_deref()))
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn check_flowseal_update(handle: AppHandle) -> Answer<Option<String>> {
    blocking(move || actions::check_flowseal_update(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub async fn system_checks(handle: AppHandle) -> Answer<Vec<SystemCheck>> {
    blocking(move || Ok(zapret::checks::run_all(&app(&handle).folder()?))).await
}

#[tauri::command]
#[specta::specta]
pub async fn fix_check(handle: AppHandle, action: CheckAction) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change(|_| zapret::checks::fix(action));
        actions::changed(&handle);
        done
    })
    .await
}

/// Not a change to zapret: no queue.
#[tauri::command]
#[specta::specta]
pub async fn clear_discord_cache() -> Answer<Vec<CacheReport>> {
    blocking(zapret::discord_cache::clear).await
}

#[tauri::command]
#[specta::specta]
pub async fn remove_service(handle: AppHandle) -> Answer<()> {
    blocking(move || {
        let done = app(&handle).change_safely("remove_service", |_| zapret::service::remove());
        actions::changed(&handle);
        done
    })
    .await
}

#[tauri::command]
#[specta::specta]
pub async fn quick_check(handle: AppHandle) -> Answer<QuickCheck> {
    blocking(move || actions::quick_check(&handle)).await
}

#[tauri::command]
#[specta::specta]
pub fn take_notices(state: State<'_, AppState>) -> Vec<Notice> {
    state.take_notices()
}
```

- [ ] **Шаг 9: Сборка в `lib.rs`**

`src-tauri/src/lib.rs` целиком:

```rust
//! Zapret Hub: IPC routing, the window, the tray and background jobs. The logic lives in `crates/`.

mod actions;
mod autostart;
mod commands;
mod dto;
mod jobs;
mod journal;
mod notify;
mod settings;
mod state;
mod texts;
mod window;

use specta_typescript::Typescript;
use tauri_specta::{Builder, collect_commands, collect_events};

use crate::dto::{Notice, NoticeLevel};
use crate::state::{AppState, Paths};

/// Regenerated by `cargo run -p zapret-hub --bin export-bindings` and by every debug start.
const BINDINGS_PATH: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../frontend/src/lib/ipc/bindings.ts");

fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new()
        .events(collect_events![dto::TestProgress, dto::TestsFinished, dto::StateChanged, dto::NoticesChanged])
        .commands(collect_commands![
            commands::app_version,
            commands::get_settings,
            commands::save_settings,
            commands::get_overview,
            commands::check_proxy,
            commands::wizard_candidates,
            commands::check_folder,
            commands::pick_folder,
            commands::open_url,
            commands::open_folder,
            commands::list_strategies,
            commands::get_strategy,
            commands::apply_strategy,
            commands::apply_best,
            commands::restart_zapret,
            commands::clone_strategy,
            commands::validate_custom,
            commands::save_custom,
            commands::delete_custom,
            commands::start_tests,
            commands::cancel_tests,
            commands::test_live,
            commands::list_runs,
            commands::get_run,
            commands::run_report,
            commands::get_flowseal,
            commands::set_game_filter,
            commands::set_ipset_mode,
            commands::set_check_updates,
            commands::replace_fake,
            commands::add_list_entry,
            commands::remove_list_entry,
            commands::update_ipset,
            commands::hosts_plan,
            commands::apply_hosts,
            commands::check_flowseal_update,
            commands::system_checks,
            commands::fix_check,
            commands::clear_discord_cache,
            commands::remove_service,
            commands::quick_check,
            commands::take_notices
        ])
}

pub fn export_bindings() -> anyhow::Result<()> {
    specta_builder()
        .export(Typescript::default(), BINDINGS_PATH)
        .map_err(|err| anyhow::anyhow!("failed to export IPC bindings: {err}"))
}

/// What `main` does with an error from `run`.
pub fn report(err: &anyhow::Error) {
    fatal(&format!("{}\n\n{err:#}", texts::FAILED_TO_START));
}

/// The one native message box: there is no window to show the error in.
fn fatal(text: &str) {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
    use windows::core::HSTRING;
    log::error!("{text}");
    // SAFETY: a plain Win32 call with valid, NUL-terminated wide strings.
    let _ = unsafe { MessageBoxW(None, &HSTRING::from(text), &HSTRING::from(texts::APP), MB_OK | MB_ICONERROR) };
}

const WEBVIEW2_CLIENT: &str = r"Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";

/// `pv` of the WebView2 Runtime, per machine (WOW6432Node on 64-bit Windows) or per user. An
/// uninstall leaves an empty value or `0.0.0.0` behind.
fn webview2_installed() -> bool {
    [
        windows_registry::LOCAL_MACHINE.open(format!(r"SOFTWARE\WOW6432Node\{WEBVIEW2_CLIENT}")),
        windows_registry::CURRENT_USER.open(format!(r"Software\{WEBVIEW2_CLIENT}")),
    ]
    .into_iter()
    .flatten()
    .filter_map(|key| key.get_string("pv").ok())
    .any(|version| !version.trim().is_empty() && version.trim() != "0.0.0.0")
}

pub fn run() -> anyhow::Result<()> {
    // Without the runtime no window can exist: the user is told here (spec 12).
    if !webview2_installed() {
        fatal(texts::WEBVIEW2_MISSING);
        std::process::exit(1);
    }
    let paths = Paths::current()?;
    journal::init(paths.logs());
    log::info!("Zapret Hub {} started from {}", env!("CARGO_PKG_VERSION"), paths.exe_dir.display());
    let (settings, damaged) = settings::load_or_default(&paths.settings());
    let state = AppState::new(paths, settings);
    if let Some(text) = damaged {
        state.push_notice(Notice { level: NoticeLevel::Error, title: texts::SETTINGS_RESET.to_owned(), text });
    }
    let builder = specta_builder();
    #[cfg(debug_assertions)]
    if let Err(err) = export_bindings() {
        log::warn!("{err:#}");
    }
    tauri::Builder::default()
        // First: a second copy hands over to this one before anything else starts.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| window::show(app)))
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);
            notify::register();
            window::show(app.handle());
            jobs::start(app.handle());
            Ok(())
        })
        .run(tauri::generate_context!())?;
    Ok(())
}
```

- [ ] **Шаг 10: Тесты, clippy, привязки**

Run: `cargo test -p zapret-hub --lib`
Expected: PASS, 9 тестов.

Run: `cargo clippy -p zapret-hub --all-targets -- -D warnings`
Expected: без замечаний. Если clippy просит другой порядок импортов или упрощение — правьте код, а не подавляйте.

Run: `cargo run -p zapret-hub --bin export-bindings`, затем `npm run check`
Expected: в `bindings.ts` есть `getOverview`, `startTests`, `takeNotices`, `testProgress`, `stateChanged`; `svelte-check` без ошибок.

- [ ] **Шаг 11: Временная проверка в окне**

`frontend/src/App.svelte` (задача 17 заменит его):

```svelte
<script lang="ts">
  import { commands, type Overview } from "$lib/ipc/bindings";

  let overview = $state<Overview | null>(null);
  let error = $state("");
  void commands.getOverview().then((answer) => {
    if (answer.status === "ok") overview = answer.data;
    else error = answer.error;
  });
</script>

<main>
  <h1>Zapret Hub</h1>
  {#if error}<p class="bad">{error}</p>{/if}
  <pre>{JSON.stringify(overview, null, 2)}</pre>
</main>
```

Run: `npm run check`
Expected: 0 ошибок.

Вручную, из повышенного терминала: `npm run tauri dev`. Ожидается JSON обзора: `service` — состояние службы zapret, `strategy` — имя из реестра. Рядом с `target\debug\zapret-hub.exe` появились `data\logs\<дата>.log` (строка `Zapret Hub 0.1.0 started from …`) и, после первого изменения, `zapret-hub.toml`. Пока папка не задана, `folder_error` — «the zapret folder is not set: choose it in Settings».

- [ ] **Шаг 12: Коммит**

```bash
git add src-tauri frontend/src Cargo.lock
git commit -m "feat(app): keep settings and state, and expose zapret and tests over IPC"
```

---

### Задача 15: Трей, фоновые проверки, запуск свёрнутым

**Files:**
- Create: `src-tauri/src/tray.rs`
- Modify: `src-tauri/src/lib.rs`, `src-tauri/src/jobs.rs`, `src-tauri/src/actions.rs`, `src-tauri/src/texts.rs`
- Test: модульные тесты в `src-tauri/src/tray.rs`

**Interfaces:**
- Consumes: `AppState::{quick, best, testing, cancel_tests, update, folder}`, `actions::{state, changed, notice, apply, restart, quick_check, check_flowseal_update}`, `notify::show`, `window::show`, `texts::APP`, `zapret::service::{state, strategy, SERVICE, SvcState, QuickCheck}`, `zapret::flags::check_updates`
- Produces: `tray::{Light { Green, Yellow, Gray }, light(SvcState, Option<&QuickCheck>) -> Light, pixels(Light) -> Vec<u8>, create(&AppHandle), refresh(&AppHandle)}`; `texts::{quick_result, tooltip, strategy, QUICK_OK, QUICK_FAILED, UPDATE, update, …}`; `actions::changed` теперь обновляет и трей. Страница ничего нового не получает.

- [ ] **Шаг 1: Падающие тесты**

`src-tauri/src/tray.rs`, блок тестов:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn check(ok: bool) -> QuickCheck {
        QuickCheck { ok, service: SvcState::Running, winws_running: true, checks: Vec::new() }
    }

    #[test]
    fn the_light_follows_the_service_and_the_last_check() {
        assert_eq!(light(SvcState::Running, Some(&check(true))), Light::Green);
        assert_eq!(light(SvcState::Running, Some(&check(false))), Light::Yellow);
        assert_eq!(light(SvcState::Running, None), Light::Yellow);
        assert_eq!(light(SvcState::Stopped, Some(&check(true))), Light::Gray);
        assert_eq!(light(SvcState::NotInstalled, None), Light::Gray);
    }

    #[test]
    fn the_icon_is_a_disc_on_transparency() {
        let rgba = pixels(Light::Green);
        assert_eq!(rgba.len(), 32 * 32 * 4);
        let at = |x: usize, y: usize| rgba[(y * 32 + x) * 4..][..4].to_vec();
        assert_eq!(at(16, 16), [0x2E, 0xB8, 0x5C, 255]);
        assert_eq!(at(0, 0)[3], 0);
        assert_eq!(at(31, 31)[3], 0);
        assert_eq!(pixels(Light::Gray)[(16 * 32 + 16) * 4..][..3], [0x8A, 0x8F, 0x98]);
    }
}
```

Добавьте `mod tray;` в `lib.rs`.

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret-hub --lib tray`
Expected: FAIL при компиляции: `light`, `pixels`, `Light` не найдены.

- [ ] **Шаг 3: Трей**

`src-tauri/src/tray.rs`, над тестами:

```rust
//! The tray icon (spec 9.3): its color says whether zapret works, its menu switches strategies.

use std::time::{Duration, Instant};

use tauri::image::Image;
use tauri::menu::{IsMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Wry};
use zapret::service::{QuickCheck, SERVICE, SvcState};

use crate::dto::NoticeLevel;
use crate::{actions, notify, texts, window};

const TRAY: &str = "main";
const SIZE: usize = 32;
const RADIUS: f32 = 14.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Light {
    Green,
    Yellow,
    Gray,
}

/// Green: the service runs and the last quick check passed. Yellow: it runs, but the check failed
/// or has not run yet. Gray: stopped or not installed.
pub fn light(service: SvcState, quick: Option<&QuickCheck>) -> Light {
    match (service, quick) {
        (SvcState::Running, Some(check)) if check.ok => Light::Green,
        (SvcState::Running, _) => Light::Yellow,
        _ => Light::Gray,
    }
}

/// A 32×32 RGBA disc with a soft edge: drawn here, so there are no image files to ship.
pub fn pixels(light: Light) -> Vec<u8> {
    let [r, g, b] = match light {
        Light::Green => [0x2E, 0xB8, 0x5C],
        Light::Yellow => [0xF2, 0xB7, 0x05],
        Light::Gray => [0x8A, 0x8F, 0x98],
    };
    let center = SIZE as f32 / 2.0;
    let mut out = Vec::with_capacity(SIZE * SIZE * 4);
    for y in 0..SIZE {
        for x in 0..SIZE {
            let (dx, dy) = (x as f32 + 0.5 - center, y as f32 + 0.5 - center);
            let coverage = (RADIUS + 0.5 - (dx * dx + dy * dy).sqrt()).clamp(0.0, 1.0);
            out.extend([r, g, b, (coverage * 255.0).round() as u8]);
        }
    }
    out
}

fn icon(light: Light) -> Image<'static> {
    Image::new_owned(pixels(light), SIZE as u32, SIZE as u32)
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    TrayIconBuilder::with_id(TRAY)
        .icon(icon(Light::Gray))
        .tooltip(texts::APP)
        .menu(&menu(app, None, &[])?)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| on_menu(app, &event.id().0))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                window::show(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

/// After every `actions::changed`. Cheap: the SCM and the registry only.
pub fn refresh(app: &AppHandle) {
    let Some(tray) = app.tray_by_id(TRAY) else { return };
    let state = actions::state(app);
    let strategy = zapret::service::strategy();
    let light = light(zapret::service::state(SERVICE), state.quick().as_ref());
    let updated = menu(app, strategy.as_deref(), &state.best(5))
        .and_then(|menu| tray.set_menu(Some(menu)))
        .and_then(|()| tray.set_icon(Some(icon(light))))
        .and_then(|()| tray.set_tooltip(Some(texts::tooltip(light, strategy.as_deref()))));
    if let Err(err) = updated {
        log::warn!("cannot update the tray: {err}");
    }
}

/// «Открыть»; «Стратегия: <имя>» → «Переключить на» and the top five; «Перезапустить zapret»;
/// «Быстрая проверка»; «Выход» (spec 9.3).
fn menu(app: &AppHandle, strategy: Option<&str>, best: &[String]) -> tauri::Result<Menu<Wry>> {
    let switch = MenuItem::with_id(app, "switch", texts::SWITCH_TO, false, None::<&str>)?;
    let empty = MenuItem::with_id(app, "empty", texts::NO_RANKING, false, None::<&str>)?;
    let line = PredefinedMenuItem::separator(app)?;
    let names = best
        .iter()
        .map(|name| MenuItem::with_id(app, format!("apply:{name}"), name, true, None::<&str>))
        .collect::<tauri::Result<Vec<_>>>()?;
    let mut inner: Vec<&dyn IsMenuItem<Wry>> = vec![&switch, &line];
    if names.is_empty() {
        inner.push(&empty);
    }
    inner.extend(names.iter().map(|item| item as &dyn IsMenuItem<Wry>));
    let current = Submenu::with_items(app, texts::strategy(strategy), true, &inner)?;
    let open = MenuItem::with_id(app, "open", texts::OPEN, true, None::<&str>)?;
    let restart = MenuItem::with_id(app, "restart", texts::RESTART, true, None::<&str>)?;
    let quick = MenuItem::with_id(app, "quick", texts::QUICK_CHECK, true, None::<&str>)?;
    let gap = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", texts::QUIT, true, None::<&str>)?;
    let items: [&dyn IsMenuItem<Wry>; 6] = [&open, &current, &restart, &quick, &gap, &quit];
    Menu::with_items(app, &items)
}

fn on_menu(app: &AppHandle, id: &str) {
    match id {
        "open" => window::show(app),
        "quit" => quit(app),
        "restart" => background(app, actions::restart),
        "quick" => background(app, actions::quick_check),
        _ => {
            if let Some(name) = id.strip_prefix("apply:") {
                let name = name.to_owned();
                background(app, move |app| actions::apply(app, &name));
            }
        }
    }
}

/// Menu events come on the main thread: the work goes to another one, and its outcome to a
/// notification, as the window is usually closed.
fn background(app: &AppHandle, work: impl FnOnce(&AppHandle) -> anyhow::Result<QuickCheck> + Send + 'static) {
    let app = app.clone();
    std::thread::spawn(move || match work(&app) {
        Ok(check) => {
            let title = if check.ok { texts::QUICK_OK } else { texts::QUICK_FAILED };
            notify::show(&app, title, &texts::quick_result(&check));
        }
        Err(err) => {
            let text = format!("{err:#}");
            notify::show(&app, texts::TRAY_FAILED, &text);
            actions::notice(&app, NoticeLevel::Error, texts::TRAY_FAILED, text);
        }
    });
}

/// «Выход». A test run is canceled first and puts zapret back: quitting must not leave the
/// service stopped.
fn quit(app: &AppHandle) {
    let state = actions::state(app);
    if !state.testing() {
        app.exit(0);
        return;
    }
    state.cancel_tests();
    let app = app.clone();
    std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(60);
        while actions::state(&app).testing() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(200));
        }
        app.exit(0);
    });
}
```

В `src-tauri/src/texts.rs` в импорты — `use probe::curl::CheckStatus;` и `use zapret::service::{QuickCheck, SvcState};`, перед блоком тестов:

```rust
pub const QUICK_OK: &str = "Быстрая проверка пройдена";
pub const QUICK_FAILED: &str = "Быстрая проверка не прошла";

pub fn quick_result(check: &QuickCheck) -> String {
    if check.service != SvcState::Running {
        return "Служба zapret не запущена".to_owned();
    }
    if !check.winws_running {
        return "Служба запущена, но winws.exe не работает".to_owned();
    }
    check
        .checks
        .iter()
        .map(|item| match (item.status, item.time_ms) {
            (CheckStatus::Ok, Some(ms)) => format!("{} — {ms} мс", item.name),
            (CheckStatus::Ok, None) => format!("{} — OK", item.name),
            (CheckStatus::Ssl, _) => format!("{} — ошибка TLS", item.name),
            (CheckStatus::Unsup, _) => format!("{} — сброс соединения", item.name),
            (CheckStatus::Error, _) => format!("{} — нет ответа", item.name),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub const UPDATE: &str = "Вышла новая версия Flowseal";

pub fn update(version: &str) -> String {
    format!("Версия {version}. Ссылка на релиз — на вкладке «Обзор».")
}

pub const OPEN: &str = "Открыть";
pub const SWITCH_TO: &str = "Переключить на";
pub const NO_RANKING: &str = "Нет результатов тестов";
pub const RESTART: &str = "Перезапустить zapret";
pub const QUICK_CHECK: &str = "Быстрая проверка";
pub const QUIT: &str = "Выход";
pub const TRAY_FAILED: &str = "Действие из трея не выполнено";

pub fn strategy(name: Option<&str>) -> String {
    format!("Стратегия: {}", name.unwrap_or("не выбрана"))
}

/// Windows cuts a tray tooltip at 127 characters: two short lines.
pub fn tooltip(light: crate::tray::Light, strategy: Option<&str>) -> String {
    use crate::tray::Light;
    let status = match light {
        Light::Green => "работает",
        Light::Yellow => "работает, проверка не пройдена",
        Light::Gray => "служба не запущена",
    };
    match strategy {
        Some(name) if light != Light::Gray => format!("{APP}: {status}\n{name}"),
        _ => format!("{APP}: {status}"),
    }
}
```

В `src-tauri/src/actions.rs` функция `changed` становится:

```rust
/// zapret or the settings changed: the page reads them again, the tray redraws.
pub fn changed(app: &AppHandle) {
    if let Err(err) = StateChanged.emit(app) {
        log::warn!("cannot announce a change: {err}");
    }
    crate::tray::refresh(app);
}
```

- [ ] **Шаг 4: Периодические проверки**

В `src-tauri/src/jobs.rs` заменить `start` и добавить две функции; в импорты — `use std::time::{Duration, Instant};` и `use zapret::service::SvcState;`:

```rust
const QUICK_EVERY: Duration = Duration::from_secs(30 * 60);
const VERSION_EVERY: Duration = Duration::from_secs(24 * 60 * 60);

/// Called once from `setup`: the start, then a quick check every 30 minutes and the Flowseal
/// version once a day (spec 5.6, 9.3).
pub fn start(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        startup(&app);
        let mut version_checked: Option<Instant> = None;
        loop {
            if version_checked.is_none_or(|at| at.elapsed() >= VERSION_EVERY) {
                check_version(&app);
                version_checked = Some(Instant::now());
            }
            background_check(&app);
            std::thread::sleep(QUICK_EVERY);
        }
    });
}

/// A notification only when a running service stops passing (spec 9.3).
fn background_check(app: &AppHandle) {
    let before = state(app).quick();
    // No folder yet, or a test run: nothing to check.
    let Ok(check) = actions::quick_check(app) else { return };
    let was_passing = before.is_none_or(|before| before.ok);
    if check.service == SvcState::Running && !check.ok && was_passing {
        notify::show(app, texts::QUICK_FAILED, &texts::quick_result(&check));
    }
}

/// When Flowseal's own flag asks for it; one notification per new version.
fn check_version(app: &AppHandle) {
    let state = state(app);
    let Ok(folder) = state.folder() else { return };
    if !zapret::flags::check_updates(&folder) {
        return;
    }
    let known = state.update();
    match actions::check_flowseal_update(app) {
        Ok(Some(version)) if known.as_deref() != Some(version.as_str()) => {
            notify::show(app, texts::UPDATE, &texts::update(&version));
        }
        Ok(_) => {}
        Err(err) => log::warn!("cannot check the Flowseal version: {err:#}"),
    }
}
```

- [ ] **Шаг 5: Трей и запуск свёрнутым в `lib.rs`**

В `run()` перед `let builder = specta_builder();`:

```rust
    // The Task Scheduler entry passes it (autostart.rs).
    let autostarted = std::env::args().any(|arg| arg == "--autostart");
```

Добавить `use tauri::Manager as _;`, а хвост `run()` от `.setup(` заменить на:

```rust
        .setup(move |app| {
            builder.mount_events(app);
            notify::register();
            tray::create(app.handle())?;
            let settings = app.state::<AppState>().settings();
            // A logon start stays in the tray once the wizard is done (spec 9.2, 9.3).
            if !(autostarted && settings.start_minimized && settings.wizard_done) {
                window::show(app.handle());
            }
            jobs::start(app.handle());
            Ok(())
        })
        .build(tauri::generate_context!())?
        .run(|_app, event| {
            // The close button leaves the app in the tray; only «Выход» ends it (spec 4.2).
            if let tauri::RunEvent::ExitRequested { code: None, api, .. } = event {
                api.prevent_exit();
            }
        });
    Ok(())
}
```

- [ ] **Шаг 6: Тесты и clippy**

Run: `cargo test -p zapret-hub --lib`
Expected: PASS, 11 тестов.

Run: `cargo clippy -p zapret-hub --all-targets -- -D warnings`
Expected: без замечаний.

Вручную, из повышенного терминала, `npm run tauri dev`:
- в трее кружок: зелёный, если служба zapret работает и проверка прошла (через несколько секунд после старта), серый — если служба остановлена (`sc stop zapret` в другом окне, затем «Быстрая проверка» в меню);
- подсказка над иконкой — состояние и имя стратегии;
- крестик закрывает окно, процесс остаётся, иконка на месте; щелчок левой кнопкой открывает окно заново;
- «Быстрая проверка» показывает уведомление Windows с двумя целями и временем; если уведомления нет, проверьте журнал (`cannot show a notification`) и `HKCU\Software\Classes\AppUserModelId\ZapretHub`;
- «Выход» завершает процесс, служба zapret продолжает работать (`sc query zapret` → `RUNNING`).

- [ ] **Шаг 7: Коммит**

```bash
git add src-tauri
git commit -m "feat(app): add the tray, periodic quick checks and minimized start"
```

---

### Задача 16: Основа интерфейса — очередь уведомлений, подтверждения, строки, сортировка

**Files:**
- Create: `frontend/src/lib/api.ts`, `frontend/src/lib/notices.svelte.ts`, `frontend/src/lib/confirm.svelte.ts`, `frontend/src/lib/app.svelte.ts`, `frontend/src/lib/strings.ts`, `frontend/src/lib/format.ts`, `frontend/src/lib/sort.ts`, `frontend/src/components/Notices.svelte`, `frontend/src/components/ConfirmDialog.svelte`
- Modify: `frontend/src/app.css`
- Test: `frontend/src/lib/notices.test.ts`, `frontend/src/lib/sort.test.ts`, `frontend/src/lib/format.test.ts`

**Interfaces:**
- Consumes: `commands`, `events` и типы из `$lib/ipc/bindings` (задача 14)
- Produces (для задач 17–20):
  - `api.ts`: `type Answer<T>`, `call<T>(title, answer: Promise<Answer<T>>): Promise<T | undefined>` — при ошибке кладёт уведомление с заголовком `title` и возвращает `undefined`
  - `notices.svelte.ts`: `class NoticeQueue { items; current; push(notice); dismiss() }`, `notices`
  - `confirm.svelte.ts`: `interface Question { title; text; yes; danger? }`, `confirmation.ask(question): Promise<boolean>`, `confirmation.question`, `confirmation.answer(yes)`
  - `app.svelte.ts`: `type Tab = "overview" | "strategies" | "tests" | "zapret" | "settings"`, `app { tab, settings, overview, revision, live, finished, busy }`, `start()`, `loadSettings()`, `refreshOverview()`, `refreshLive()`, `change<T>(title, run: () => Promise<Answer<T>>)`, `applyTheme(theme)`
  - `strings.ts`: `S` — все строки интерфейса, `LINKS.release(version)`, `plural(n, forms)`
  - `format.ts`: `ms`, `runDate`, `hubStem`, `blockSummary`, `quickSummary`, `estimateMinutes`, `statusClass`, `dpiClass`, `levelClass`
  - `sort.ts`: `type Direction = "asc" | "desc"`, `sortBy<T>(rows, value, dir): T[]`
  - Компоненты `Notices.svelte`, `ConfirmDialog.svelte` без свойств; общие классы в `app.css`: `.card`, `.cards`, `.buttons`, `.row`, `.grow`, `.wide`, `.badge`, `.choice`, `.hint`, `button.link`, `button.danger`, таблицы, `pre`

Как страница узнаёт об изменениях: на `StateChanged` увеличивается `app.revision` и перечитывается обзор; вкладки в `$effect` читают `app.revision` и загружают свои данные заново. На `TestProgress` страница перечитывает `testLive()` целиком: так не нужно сводить события с уже прочитанным состоянием. На `NoticesChanged` забирает `takeNotices()` в общую очередь.

- [ ] **Шаг 1: Падающие тесты**

`frontend/src/lib/notices.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { NoticeQueue } from "./notices.svelte";

describe("NoticeQueue", () => {
  it("shows errors before messages and drops nothing", () => {
    const queue = new NoticeQueue();
    queue.push({ level: "Info", title: "a", text: "" });
    queue.push({ level: "Error", title: "b", text: "" });
    queue.push({ level: "Info", title: "c", text: "" });
    queue.push({ level: "Error", title: "d", text: "" });
    const seen: string[] = [];
    while (queue.current) {
      seen.push(queue.current.title);
      queue.dismiss();
    }
    expect(seen).toEqual(["b", "d", "a", "c"]);
  });
});
```

`frontend/src/lib/sort.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { sortBy } from "./sort";

describe("sortBy", () => {
  const rows = [
    { name: "a", value: 3 },
    { name: "b", value: null },
    { name: "c", value: 1 },
    { name: "d", value: 3 },
  ];

  it("puts empty values last either way and keeps ties in order", () => {
    expect(sortBy(rows, (row) => row.value, "asc").map((row) => row.name)).toEqual(["c", "a", "d", "b"]);
    expect(sortBy(rows, (row) => row.value, "desc").map((row) => row.name)).toEqual(["a", "d", "c", "b"]);
  });

  it("compares numbers inside names as numbers", () => {
    const names = ["general (ALT10)", "general (ALT2)", "general"].map((name) => ({ name }));
    expect(sortBy(names, (row) => row.name, "asc").map((row) => row.name)).toEqual([
      "general",
      "general (ALT2)",
      "general (ALT10)",
    ]);
  });
});
```

`frontend/src/lib/format.test.ts`:

```ts
import { describe, expect, it } from "vitest";
import { blockSummary, runDate } from "./format";

describe("blockSummary", () => {
  it("names the protocol, the ports and what the block is limited to", () => {
    expect(
      blockSummary('--filter-udp=443 --hostlist="%LISTS%list-general.txt" --hostlist-exclude="%LISTS%list-exclude.txt" --dpi-desync=fake'),
    ).toBe("UDP 443 · list-general.txt");
    expect(blockSummary("--filter-udp=19294-19344,50000-50100 --filter-l7=discord,stun --dpi-desync=fake")).toBe(
      "UDP 19294-19344,50000-50100 · l7 discord,stun",
    );
    expect(blockSummary("--filter-tcp=443 --hostlist-domains=discord.media --dpi-desync=multisplit")).toBe(
      "TCP 443 · discord.media",
    );
    expect(blockSummary("--dpi-desync=fake")).toBe("все пакеты");
  });
});

describe("runDate", () => {
  it("turns a run id into a date and a time", () => {
    expect(runDate("2026-10-10_05-03-34")).toBe("10.10.2026 05:03");
    expect(runDate("odd")).toBe("odd");
  });
});
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `npm test`
Expected: FAIL: модулей `./notices.svelte`, `./sort`, `./format` нет.

- [ ] **Шаг 3: Очередь, вызовы, подтверждения, состояние**

`frontend/src/lib/notices.svelte.ts`:

```ts
import type { Notice } from "./ipc/bindings";

/** One queue for every error and message (spec 10): errors first, nothing dropped, closing shows the next. */
export class NoticeQueue {
  items = $state<Notice[]>([]);

  get current(): Notice | undefined {
    return this.items[0];
  }

  push(notice: Notice): void {
    if (notice.level !== "Error") {
      this.items.push(notice);
      return;
    }
    const firstMessage = this.items.findIndex((item) => item.level !== "Error");
    this.items.splice(firstMessage === -1 ? this.items.length : firstMessage, 0, notice);
  }

  dismiss(): void {
    this.items.shift();
  }
}

export const notices = new NoticeQueue();
```

`frontend/src/lib/api.ts`:

```ts
import { notices } from "./notices.svelte";

/** What tauri-specta makes of a command that returns `Result<T, String>`. */
export type Answer<T> = { status: "ok"; data: T } | { status: "error"; error: string };

/** The command's data, or `undefined` once its error is in the notice queue under `title`. */
export async function call<T>(title: string, answer: Promise<Answer<T>>): Promise<T | undefined> {
  try {
    const result = await answer;
    if (result.status === "ok") return result.data;
    notices.push({ level: "Error", title, text: result.error });
  } catch (err) {
    notices.push({ level: "Error", title, text: String(err) });
  }
  return undefined;
}
```

`frontend/src/lib/confirm.svelte.ts`:

```ts
export interface Question {
  title: string;
  text: string;
  /** The label of the button that agrees. */
  yes: string;
  danger?: boolean;
}

/** One modal question at a time; a new one answers the previous with «no». */
class Confirmation {
  question = $state<Question | null>(null);
  #resolve: ((yes: boolean) => void) | null = null;

  ask(question: Question): Promise<boolean> {
    this.#resolve?.(false);
    this.question = question;
    return new Promise((resolve) => (this.#resolve = resolve));
  }

  answer(yes: boolean): void {
    this.#resolve?.(yes);
    this.#resolve = null;
    this.question = null;
  }
}

export const confirmation = new Confirmation();
```

`frontend/src/lib/app.svelte.ts`:

```ts
import { commands, events, type LiveRun, type Overview, type Settings, type Theme } from "./ipc/bindings";
import { call, type Answer } from "./api";
import { notices } from "./notices.svelte";
import { S } from "./strings";

export type Tab = "overview" | "strategies" | "tests" | "zapret" | "settings";

class App {
  tab = $state<Tab>("overview");
  settings = $state<Settings | null>(null);
  overview = $state<Overview | null>(null);
  /** Bumped by every `StateChanged`: tabs read what they show again when it moves. */
  revision = $state(0);
  live = $state<LiveRun | null>(null);
  /** The run that just finished, for the Tests tab to open. */
  finished = $state<string | null>(null);
  pending = $state(0);

  /** A command the user waits on is running: the buttons that change zapret stay off. */
  get busy(): boolean {
    return this.pending > 0;
  }
}

export const app = new App();

const darkScheme = matchMedia("(prefers-color-scheme: dark)");

export function applyTheme(theme: Theme): void {
  const dark = theme === "dark" || (theme === "system" && darkScheme.matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
}

darkScheme.addEventListener("change", () => applyTheme(app.settings?.theme ?? "system"));

/** `call` for a command the user waits on. */
export async function change<T>(title: string, run: () => Promise<Answer<T>>): Promise<T | undefined> {
  app.pending++;
  try {
    return await call(title, run());
  } finally {
    app.pending--;
  }
}

export async function loadSettings(): Promise<void> {
  app.settings = await commands.getSettings();
  // `#[serde(default)]` makes every Settings field optional in the generated types.
  applyTheme(app.settings.theme ?? "system");
}

export async function refreshOverview(): Promise<void> {
  const overview = await call(S.failed.overview, commands.getOverview());
  if (overview) app.overview = overview;
}

let finishes = 0;

/** A `testLive` answer that crossed `TestsFinished` on the way is stale: it is dropped. */
export async function refreshLive(): Promise<void> {
  const seen = finishes;
  const live = await commands.testLive();
  if (seen === finishes) app.live = live;
}

async function pullNotices(): Promise<void> {
  for (const notice of await commands.takeNotices()) notices.push(notice);
}

/** Once, from `App.svelte`: listeners first, so nothing that happens meanwhile is missed. */
export async function start(): Promise<void> {
  await events.stateChanged.listen(() => {
    app.revision++;
    void refreshOverview();
  });
  await events.noticesChanged.listen(() => void pullNotices());
  await events.testProgress.listen(() => void refreshLive());
  await events.testsFinished.listen(({ payload }) => {
    finishes++;
    app.live = null;
    app.finished = payload.run_id;
  });
  await loadSettings();
  await Promise.all([pullNotices(), refreshOverview(), refreshLive()]);
}
```

- [ ] **Шаг 4: Строки, форматирование, сортировка**

`frontend/src/lib/strings.ts`:

```ts
// Every string of the page (spec 9.1): Russian now, a translation replaces this file.
import type {
  CacheOutcome,
  CheckAction,
  CheckId,
  CheckLevel,
  CheckStatus,
  DpiStatus,
  FakeKind,
  GameFilterMode,
  IpsetMode,
  Score,
  SvcState,
  Theme,
  UserList,
} from "./ipc/bindings";

/** `plural(5, ["стратегия", "стратегии", "стратегий"])` → «стратегий». */
export function plural(n: number, [one, few, many]: [string, string, string]): string {
  const tens = n % 100;
  const units = n % 10;
  if (tens >= 11 && tens <= 14) return many;
  if (units === 1) return one;
  if (units >= 2 && units <= 4) return few;
  return many;
}

const strategies = (n: number) => `${n} ${plural(n, ["стратегия", "стратегии", "стратегий"])}`;

export const LINKS = {
  release: (version: string) => `https://github.com/Flowseal/zapret-discord-youtube/releases/tag/${version}`,
};

export const S = {
  app: "Zapret Hub",
  common: {
    cancel: "Отмена",
    close: "Закрыть",
    copy: "Копировать",
    save: "Сохранить",
    apply: "Применить",
    check: "Проверить",
    checking: "Проверяю…",
    loading: "Загрузка…",
  },
  tabs: {
    overview: "Обзор",
    strategies: "Стратегии",
    tests: "Тесты",
    zapret: "Zapret",
    settings: "Настройки",
  },
  notices: {
    more: (n: number) => `ещё ${n}`,
  },
  /** Titles of the notices a failed command leaves; the text is the error itself. */
  failed: {
    overview: "Обзор не обновился",
    settings: "Настройки не сохранены",
    proxy: "Прокси не проверен",
    folder: "Папка не проверена",
    pick: "Папка не выбрана",
    open: "Не открылось",
    strategies: "Список стратегий не прочитан",
    strategy: "Стратегия не прочитана",
    apply: "Стратегия не применена",
    restart: "zapret не перезапущен",
    quick: "Быстрая проверка не выполнена",
    clone: "Стратегия не склонирована",
    validate: "Проверка не пройдена",
    saveCustom: "Стратегия не сохранена",
    deleteCustom: "Стратегия не удалена",
    startTests: "Тесты не запущены",
    runs: "История тестов не прочитана",
    run: "Прогон не прочитан",
    report: "Отчёт не собран",
    flowseal: "Настройки Flowseal не прочитаны",
    gameFilter: "Игровой фильтр не изменён",
    ipset: "Режим ipset не изменён",
    checkUpdates: "Автопроверка не изменена",
    fake: "Фейк не заменён",
    list: "Список не изменён",
    updateIpset: "ipset-all не обновлён",
    hosts: "hosts не обновлён",
    version: "Версия Flowseal не проверена",
    checks: "Проверки не выполнены",
    fix: "Не исправлено",
    cache: "Кэш Discord не очищен",
    removeService: "Служба не удалена",
    wizard: "Настройки не сохранены",
  },
  svc: {
    NotInstalled: "не установлена",
    Stopped: "остановлена",
    StartPending: "запускается",
    StopPending: "останавливается",
    Running: "работает",
    Other: "в непонятном состоянии",
  } satisfies Record<SvcState, string>,
  /** The tokens `test zapret.ps1` prints. */
  status: { Ok: "OK", Ssl: "SSL", Unsup: "UNSUP", Error: "ERROR" } satisfies Record<CheckStatus, string>,
  dpiStatus: { Ok: "OK", Fail: "FAIL", Unsupported: "UNSUP", LikelyBlocked: "БЛОК" } satisfies Record<DpiStatus, string>,
  overview: {
    service: "Служба zapret",
    strategy: "Стратегия",
    noStrategy: "не выбрана",
    notRunning: "служба zapret не запущена",
    winwsMissing: "служба запущена, но winws.exe не работает",
    stopPending:
      "Служба застряла в остановке: скорее всего, WinDivert держит другой обход. Загляните в «Системные проверки» на вкладке Zapret.",
    quick: "Быстрая проверка",
    quickNever: "ещё не выполнялась",
    quickOk: "пройдена",
    quickFailed: "не пройдена",
    restart: "Перезапустить zapret",
    flowseal: "Flowseal",
    version: (version: string | null) => (version ? `Версия ${version}` : "Версия не найдена"),
    updateAvailable: (version: string) => `Вышла версия ${version}`,
    release: "Страница релиза",
    checkUpdate: "Проверить обновления",
    upToDate: "Обновлений нет",
    proxy: "Прокси SOCKS5",
    proxyNotSet: "Не задан. Адрес задаётся в настройках.",
    proxyOk: (latency: string) => `отвечает, ${latency}`,
    proxyFailed: "не отвечает",
    lastTest: "Последний тест",
    noTests: "Тестов ещё не было",
    best: "Лучшая",
    errors: (bad: number, total: number, latency: string) => `не прошло ${bad} из ${total}, задержка ${latency}`,
    applyBest: "Применить лучшую",
    runTests: "Запустить тесты",
    testing: "Идут тесты…",
    folderProblem: "Папка zapret недоступна",
    chooseFolder: "Выбрать папку",
  },
  settings: {
    folder: "Папка zapret",
    folderHint: "Например, E:\\Zapret",
    pick: "Выбрать…",
    missing: (parts: string[]) => `не хватает: ${parts.join(", ")}`,
    folderOk: (version: string | null) => (version ? `Flowseal ${version}` : "папка подходит"),
    proxy: "Прокси SOCKS5",
    proxyAddress: "Адрес",
    proxyHint: "Нужен для проверки на обзоре и для загрузок, если напрямую не вышло. Пусто — без прокси.",
    start: "Запуск",
    autostart: "Запускать при входе в Windows",
    minimized: "При автозапуске оставаться в трее",
    theme: "Тема",
    themes: { system: "как в системе", light: "светлая", dark: "тёмная" } satisfies Record<Theme, string>,
    saved: "Сохранено",
    files: "Файлы",
    openLogs: "Открыть журнал",
    openZapret: "Открыть папку zapret",
    version: (version: string) => `Zapret Hub ${version}`,
  },
  wizard: {
    title: "Первый запуск",
    step: (n: number, total: number) => `Шаг ${n} из ${total}`,
    folderTitle: "Где лежит zapret?",
    folderText: "Папка Flowseal zapret-discord-youtube: в ней service.bat и папки bin и lists.",
    searching: "Ищу…",
    pickOther: "Выбрать другую…",
    later: "Настроить позже",
    next: "Далее",
    back: "Назад",
    proxyTitle: "Прокси SOCKS5",
    proxyText: "Необязательно. Нужен для проверки на обзоре и для загрузок, если напрямую не вышло.",
    noProxies: "Прокси, которые сейчас отвечают на 127.0.0.1, не найдены.",
    noProxy: "Без прокси",
    autostartTitle: "Автозапуск",
    done: "Готово",
  },
  strategies: {
    search: "Поиск",
    name: "Стратегия",
    result: "Последний тест",
    active: "активная",
    custom: "своя",
    stale: "устарел",
    notStrategy: "не стратегия",
    untested: "не проверялась",
    launchError: "не запустилась",
    score: (bad: number, total: number, latency: string) => `ошибок ${bad}/${total} · ${latency}`,
    dpiScore: (blocked: number, fail: number) => `блок ${blocked} · сбоев ${fail}`,
    pick: "Выберите стратегию в списке.",
    clonedFrom: (name: string) => `Копия «${name}»`,
    edit: "Изменить",
    delete: "Удалить",
    deleteTitle: "Удалить стратегию?",
    deleteText: (name: string) => `Файл «${name}.bat» и запись о нём будут удалены.`,
    cloneName: "Имя копии",
    clone: "Клонировать",
    filter: "Фильтр перехвата",
    block: (n: number) => `Блок ${n}`,
    finalArgs: "Final args — как их передаёт service.bat",
    lastRun: (date: string) => `Последний тест: ${date}`,
  },
  editor: {
    hint: "Аргументы winws как в .bat: %BIN%, %LISTS% и %GameFilter% подставляются при установке. Перенос строки внутри блока — пробел.",
    filter: "Фильтр перехвата (--wf-…)",
    up: "Выше",
    down: "Ниже",
    duplicate: "Дублировать",
    remove: "Удалить",
    add: "Добавить блок",
    validate: "Проверить",
    valid: "Проверка пройдена: разбор и winws --dry-run без ошибок.",
    appliedOk: "Сохранено и установлено заново: быстрая проверка пройдена.",
    appliedFailed: "Сохранено и установлено заново, но быстрая проверка не прошла.",
    allTraffic: "все пакеты",
  },
  results: {
    rank: "#",
    name: "Стратегия",
    errors: "Ошибки",
    latency: "HTTPS",
    ping: "Пинг ICMP",
    discord: "Discord",
    youtube: "YouTube",
    dbd: "DbD/Steam",
    blocked: "Блок",
    fail: "Сбои",
    ok: "OK",
    launch: "не запустилась",
    target: "Цель",
    host: "Хост",
    extra: "(доп.)",
    kinds: { Http: "HTTP", Tls12: "TLS 1.2", Tls13: "TLS 1.3" },
    breakdown: (score: Score) => `ERROR ${score.errors}, SSL ${score.ssl}, UNSUP ${score.unsup}`,
  },
  tests: {
    newRun: "Новый прогон",
    standard: "Стандартный",
    dpi: "DPI 16–20",
    standardHint:
      "Цели из targets.txt: HTTP, TLS 1.2, TLS 1.3 и пинг. Сначала стратегии с меньшим числом ошибок, при равенстве — с меньшей задержкой HTTPS.",
    dpiHint: "Набор hyperion-cs: блокировка после 16–20 КБ. На время прогона ipset переключается в «любой IP».",
    all: "Все",
    none: "Ничего",
    start: "Запустить",
    estimate: (n: number, minutes: number) => `${strategies(n)}, около ${minutes} мин; zapret на это время остановится`,
    preparing: "Подготовка…",
    progress: (index: number, total: number, name: string) => `Стратегия ${index} из ${total}: ${name}`,
    restoring: "Возвращаю службу, ipset и winws как было…",
    runTitle: (date: string, suite: string) => `Прогон ${date} · ${suite}`,
    flowseal: (version: string | null) => `Flowseal ${version ?? "?"}`,
    canceled: "отменён",
    applyBest: (name: string) => `Применить лучшую: ${name}`,
    copied: "Скопировано",
    history: "История",
    noRuns: "Прогонов ещё не было",
    runCount: strategies,
    runScore: (bad: number, total: number, latency: string) => `ошибок ${bad}/${total}, ${latency}`,
  },
  zapret: {
    service: "Служба",
    serviceHint: "Как «Remove Services» в service.bat: служба zapret, процессы winws, затем WinDivert.",
    remove: "Удалить службу",
    removeTitle: "Удалить службу zapret?",
    removeText: "Обход перестанет работать, пока стратегия не будет применена снова.",
    gameFilter: "Игровой фильтр",
    gameModes: { disabled: "выключен", all: "TCP и UDP", tcp: "только TCP", udp: "только UDP" } satisfies Record<
      GameFilterMode,
      string
    >,
    tcpRange: "Порты TCP",
    udpRange: "Порты UDP",
    gameHint: "Порты зашиты в аргументы службы: активная стратегия будет установлена заново.",
    ipset: "Режим ipset",
    ipsetModes: { loaded: "загруженный список", none: "выключен", any: "любой IP" } satisfies Record<IpsetMode, string>,
    ipsetHint: "После смены служба перезапускается.",
    noBackup: "Сохранённого списка нет: сначала обновите ipset-all.",
    updates: "Обновления",
    checkUpdates: "Проверять новую версию Flowseal при запуске и раз в сутки",
    updateIpset: "Обновить ipset-all",
    ipsetUpdated: "Список ipset-all обновлён.",
    updateHosts: "Обновить hosts",
    hostsCurrent: "hosts уже актуален.",
    hostsTitle: "Записать в hosts?",
    hostsApply: "Записать",
    hostsUpdated: "hosts обновлён.",
    fakesTitle: "Активные фейки",
    fakes: { Discord: "Discord (UDP)", Game: "Игры (UDP)" } satisfies Record<FakeKind, string>,
    fakeUnknown: "свой файл",
    listsTitle: "Свои списки",
    lists: {
      General: {
        title: "Свои домены",
        file: "list-general-user.txt",
        hint: "Обходятся вместе со списком Flowseal.",
        placeholder: "example.com",
      },
      Exclude: {
        title: "Исключения",
        file: "list-exclude-user.txt",
        hint: "Домены, которые обход не трогает.",
        placeholder: "example.com",
      },
      IpsetExclude: {
        title: "Исключения IP",
        file: "ipset-exclude-user.txt",
        hint: "Адреса и сети, которые обход не трогает.",
        placeholder: "192.168.0.0/16",
      },
    } satisfies Record<UserList, { title: string; file: string; hint: string; placeholder: string }>,
    add: "Добавить",
    removeEntry: "удалить",
    cache: "Кэш Discord",
    cacheButton: "Очистить кэш Discord",
    cacheTitle: "Очистить кэш Discord?",
    cacheText: "Discord (Stable, PTB, Canary, Development) будет закрыт, папки Cache, Code Cache и GPUCache удалены.",
    cacheClear: "Очистить",
    cacheNone: "Discord не найден.",
    cacheClosed: "был закрыт",
    cacheOutcome: { Deleted: "удалена", Missing: "не было", Failed: "не удалена" } satisfies Record<CacheOutcome, string>,
  },
  checks: {
    title: "Системные проверки",
    run: "Запустить проверки",
    issue: "Обсуждение на GitHub",
    level: { Ok: "в порядке", Warn: "внимание", Problem: "проблема" } satisfies Record<CheckLevel, string>,
    names: {
      Bfe: "Base Filtering Engine",
      Proxy: "Системный прокси",
      TcpTimestamps: "TCP timestamps",
      Adguard: "AdGuard",
      Killer: "Службы Killer",
      IntelConnectivity: "Intel Connectivity Network Service",
      CheckPoint: "Check Point",
      SmartByte: "SmartByte",
      CyrillicPath: "Кириллица в пути к zapret",
      OneDrive: "zapret в папке OneDrive",
      WinDivertSys: "Файл WinDivert64.sys",
      Vpn: "Службы VPN",
      SecureDns: "Безопасный DNS",
      HostsYoutube: "YouTube в hosts",
      WinDivertOrphan: "WinDivert без winws",
      Conflicts: "Другие обходы блокировок",
    } satisfies Record<CheckId, string>,
    hints: {
      Bfe: "Служба не запущена, а без неё zapret не работает.",
      Proxy: "Включён системный прокси. Убедитесь, что он рабочий, или выключите его, если не пользуетесь.",
      TcpTimestamps: "TCP timestamps выключены, а стратегиям они нужны.",
      Adguard: "AdGuard может мешать Discord.",
      Killer: "Службы Killer конфликтуют с zapret.",
      IntelConnectivity: "Эта служба конфликтует с zapret.",
      CheckPoint: "Check Point конфликтует с zapret: попробуйте его удалить.",
      SmartByte: "SmartByte конфликтует с zapret: удалите его или отключите в services.msc.",
      CyrillicPath: "Если обход не работает, перенесите zapret в папку без кириллицы, например C:\\zapret.",
      OneDrive: "Если обход не работает, перенесите zapret из OneDrive, например в C:\\zapret.",
      WinDivertSys: "В папке bin нет WinDivert64.sys: без него winws не запустится.",
      Vpn: "VPN может конфликтовать с zapret: убедитесь, что все VPN выключены.",
      SecureDns: "Настройте безопасный DNS в браузере или, в Windows 11, в параметрах сети.",
      HostsYoutube: "В hosts есть записи youtube.com или youtu.be: они могут мешать YouTube.",
      WinDivertOrphan: "winws не запущен, а служба WinDivert работает: её держит другой обход или остаток прошлого запуска.",
      Conflicts: "Найдены службы других обходов: они делят WinDivert с zapret.",
    } satisfies Record<CheckId, string>,
    /** The issues `service.bat` links to. */
    links: {
      Adguard: "https://github.com/Flowseal/zapret-discord-youtube/issues/417",
      Killer: "https://github.com/Flowseal/zapret-discord-youtube/issues/2512#issuecomment-2821119513",
      IntelConnectivity: "https://github.com/ValdikSS/GoodbyeDPI/issues/541#issuecomment-2661670982",
    } as Partial<Record<CheckId, string>>,
    actions: {
      EnableTcpTimestamps: "Включить",
      RemoveWinDivert: "Удалить WinDivert",
      RemoveConflicts: "Удалить службы",
    } satisfies Record<CheckAction, string>,
    confirm: (action: CheckAction, detail: string) =>
      action === "RemoveConflicts"
        ? `Службы ${detail} будут остановлены и удалены, затем WinDivert.`
        : "Служба WinDivert будет остановлена и удалена. Если её держит другой обход, он перестанет работать.",
  },
};
```

`frontend/src/lib/format.ts`:

```ts
import type { CheckLevel, CheckStatus, DpiStatus, QuickCheck, Suite } from "./ipc/bindings";
import { S } from "./strings";

export function ms(value: number | null | undefined): string {
  return value == null ? "—" : `${value} мс`;
}

/** `2026-10-10_05-03-34` → `10.10.2026 05:03`. */
export function runDate(id: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})_(\d{2})-(\d{2})/.exec(id);
  return match ? `${match[3]}.${match[2]}.${match[1]} ${match[4]}:${match[5]}` : id;
}

/** The file stem of a custom strategy, as `CustomStrategy::file_stem` makes it. */
export function hubStem(name: string): string {
  return `general (HUB ${name})`;
}

/** A block's card header (spec 6): protocol and ports, l7, the list or domains it is limited to. */
export function blockSummary(block: string): string {
  const pick = (flag: string): string | undefined => {
    const match = new RegExp(`--${flag}=(?:"([^"]*)"|(\\S+))`).exec(block);
    return match?.[1] ?? match?.[2];
  };
  const file = (path: string) => path.replace(/^%\w+%/, "").split(/[\\/]/).pop() ?? path;
  const parts: string[] = [];
  const tcp = pick("filter-tcp");
  const udp = pick("filter-udp");
  const l7 = pick("filter-l7");
  const hostlist = pick("hostlist");
  const ipset = pick("ipset");
  const domains = pick("hostlist-domains");
  if (tcp) parts.push(`TCP ${tcp}`);
  if (udp) parts.push(`UDP ${udp}`);
  if (l7) parts.push(`l7 ${l7}`);
  if (hostlist) parts.push(file(hostlist));
  if (ipset) parts.push(file(ipset));
  if (domains) parts.push(domains);
  return parts.length > 0 ? parts.join(" · ") : S.editor.allTraffic;
}

export function quickSummary(check: QuickCheck): string {
  if (check.service !== "Running") return S.overview.notRunning;
  if (!check.winws_running) return S.overview.winwsMissing;
  return check.checks
    .map((item) => `${item.name} — ${item.status === "Ok" ? ms(item.time_ms) : S.status[item.status]}`)
    .join("\n");
}

/** About 15 s a strategy for the standard suite (spec 14), a little more for DPI. */
export function estimateMinutes(count: number, suite: Suite): number {
  return Math.max(1, Math.round((count * (suite === "Standard" ? 15 : 20)) / 60));
}

export const statusClass: Record<CheckStatus, string> = { Ok: "ok", Ssl: "bad", Unsup: "warn", Error: "bad" };
export const dpiClass: Record<DpiStatus, string> = { Ok: "ok", Fail: "warn", Unsupported: "muted", LikelyBlocked: "bad" };
export const levelClass: Record<CheckLevel, string> = { Ok: "ok", Warn: "warn", Problem: "bad" };
```

`frontend/src/lib/sort.ts`:

```ts
export type Direction = "asc" | "desc";

/**
 * A sorted copy. Empty values go last in either direction; ties keep the input order, which for
 * a finished run is the ranking. Strings compare with numbers as numbers (`ALT2` before `ALT10`).
 */
export function sortBy<T>(
  rows: readonly T[],
  value: (row: T) => number | string | null | undefined,
  dir: Direction,
): T[] {
  const sign = dir === "asc" ? 1 : -1;
  return rows
    .map((row, index) => ({ row, index, key: value(row) }))
    .sort((a, b) => {
      if (a.key == null || b.key == null) {
        if (a.key == null && b.key == null) return a.index - b.index;
        return a.key == null ? 1 : -1;
      }
      const order =
        typeof a.key === "number" && typeof b.key === "number"
          ? a.key - b.key
          : String(a.key).localeCompare(String(b.key), "ru", { numeric: true });
      return order !== 0 ? sign * order : a.index - b.index;
    })
    .map((item) => item.row);
}
```

- [ ] **Шаг 5: Компоненты уведомлений и подтверждения, общие стили**

`frontend/src/components/Notices.svelte`:

```svelte
<script lang="ts">
  import { notices } from "$lib/notices.svelte";
  import { S } from "$lib/strings";

  async function copy(title: string, text: string): Promise<void> {
    await navigator.clipboard.writeText(text ? `${title}\n\n${text}` : title);
  }
</script>

{#if notices.current}
  {@const notice = notices.current}
  <aside class="notice" class:error={notice.level === "Error"} role="alert">
    <div class="head">
      <strong>{notice.title}</strong>
      {#if notices.items.length > 1}<span class="muted">{S.notices.more(notices.items.length - 1)}</span>{/if}
    </div>
    {#if notice.text}<pre>{notice.text}</pre>{/if}
    <div class="buttons">
      <button onclick={() => copy(notice.title, notice.text)}>{S.common.copy}</button>
      <button class="primary" onclick={() => notices.dismiss()}>{S.common.close}</button>
    </div>
  </aside>
{/if}

<style>
  .notice {
    position: fixed;
    right: 16px;
    bottom: 16px;
    width: min(520px, calc(100vw - 32px));
    padding: 12px 14px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-left: 4px solid var(--accent);
    border-radius: 8px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.25);
    z-index: 20;
  }

  .notice.error {
    border-left-color: var(--bad);
  }

  .head {
    display: flex;
    justify-content: space-between;
    gap: 12px;
  }
</style>
```

`frontend/src/components/ConfirmDialog.svelte`:

```svelte
<script lang="ts">
  import { confirmation } from "$lib/confirm.svelte";
  import { S } from "$lib/strings";

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && confirmation.question) confirmation.answer(false);
  }
</script>

<svelte:window {onkeydown} />

{#if confirmation.question}
  {@const question = confirmation.question}
  <div class="backdrop">
    <div class="dialog" role="alertdialog" aria-modal="true" aria-labelledby="confirm-title" tabindex="-1">
      <h2 id="confirm-title">{question.title}</h2>
      <pre>{question.text}</pre>
      <div class="buttons">
        <button onclick={() => confirmation.answer(false)}>{S.common.cancel}</button>
        <button class={question.danger ? "danger" : "primary"} onclick={() => confirmation.answer(true)}>
          {question.yes}
        </button>
      </div>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.4);
    z-index: 30;
  }

  .dialog {
    width: min(560px, calc(100vw - 32px));
    padding: 16px 18px;
    background: var(--panel);
    border: 1px solid var(--border);
    border-radius: 10px;
  }
</style>
```

В конец `frontend/src/app.css` (классы общие для всех вкладок: Svelte не видит классы, заданные выражением, и счёл бы их стили неиспользуемыми):

```css
h1,
h2,
h3 {
  margin: 0 0 8px;
  font-weight: 600;
}

h1 {
  font-size: 20px;
}

h2 {
  font-size: 15px;
}

h3 {
  font-size: 14px;
  margin-top: 12px;
}

p {
  margin: 4px 0;
}

code,
pre {
  font: 12px/1.4 "Cascadia Mono", Consolas, monospace;
}

pre {
  margin: 6px 0;
  padding: 8px 10px;
  max-height: 280px;
  overflow: auto;
  white-space: pre-wrap;
  word-break: break-word;
  background: var(--bg);
  border: 1px solid var(--border);
  border-radius: 6px;
}

.cards {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
  gap: 14px;
}

.card {
  background: var(--panel);
  border: 1px solid var(--border);
  border-radius: 10px;
  padding: 14px 16px;
}

.buttons {
  display: flex;
  flex-wrap: wrap;
  align-items: center;
  gap: 8px;
  margin-top: 10px;
}

.row {
  display: flex;
  align-items: center;
  gap: 8px;
}

.grow {
  flex: 1;
}

.wide {
  width: 100%;
}

.hint {
  margin: 4px 0;
}

.badge {
  display: inline-block;
  margin-left: 6px;
  padding: 0 6px;
  border: 1px solid currentColor;
  border-radius: 4px;
  font-size: 12px;
  font-weight: normal;
}

.choice {
  display: flex;
  align-items: baseline;
  gap: 8px;
  margin: 6px 0;
}

label.inline {
  display: inline-flex;
  align-items: center;
  gap: 6px;
  margin: 4px 14px 4px 0;
}

button.link {
  padding: 0;
  border: none;
  background: none;
  color: var(--accent);
  text-align: left;
}

button.link:hover:not(:disabled) {
  background: none;
  text-decoration: underline;
}

button.danger {
  border-color: var(--bad);
  color: var(--bad);
}

table {
  width: 100%;
  border-collapse: collapse;
}

th,
td {
  padding: 4px 8px;
  border-bottom: 1px solid var(--border);
  text-align: left;
  white-space: nowrap;
}

th {
  font-weight: 600;
  color: var(--muted);
}

tr.selected {
  background: var(--hover);
}

textarea {
  width: 100%;
  resize: vertical;
  font: 12px/1.4 "Cascadia Mono", Consolas, monospace;
}
```

- [ ] **Шаг 6: Тесты и проверка типов**

Run: `npm test`
Expected: PASS, 5 тестов.

Run: `npm run check`
Expected: `svelte-check found 0 errors and 0 warnings`.

- [ ] **Шаг 7: Коммит**

```bash
git add frontend/src
git commit -m "feat(ui): add the notice queue, confirmations, strings and table sorting"
```

---

### Задача 17: Оболочка, обзор, настройки, мастер первого запуска

**Files:**
- Create: `frontend/src/tabs/Overview.svelte`, `frontend/src/tabs/Settings.svelte`, `frontend/src/Wizard.svelte`
- Modify: `frontend/src/App.svelte` (целиком)

**Interfaces:**
- Consumes: всё из задачи 16; команды `getOverview`, `checkProxy`, `restartZapret`, `quickCheck`, `applyBest`, `checkFlowsealUpdate`, `openUrl`, `saveSettings`, `pickFolder`, `checkFolder`, `wizardCandidates`, `openFolder`, `appVersion`
- Produces: `App.svelte` со списком вкладок `TABS`; задачи 18–20 вставляют в него свои вкладки по образцу:

```ts
  { id: "strategies", label: S.tabs.strategies, view: Strategies },
```

- [ ] **Шаг 1: Оболочка**

`frontend/src/App.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import ConfirmDialog from "./components/ConfirmDialog.svelte";
  import Notices from "./components/Notices.svelte";
  import Overview from "./tabs/Overview.svelte";
  import Settings from "./tabs/Settings.svelte";
  import Wizard from "./Wizard.svelte";
  import { app, start, type Tab } from "$lib/app.svelte";
  import { S } from "$lib/strings";

  const TABS = [
    { id: "overview", label: S.tabs.overview, view: Overview },
    // Tasks 18–20 add Strategies, Tests and Zapret here, in that order.
    { id: "settings", label: S.tabs.settings, view: Settings },
  ] satisfies { id: Tab; label: string; view: unknown }[];

  const current = $derived(TABS.find((tab) => tab.id === app.tab) ?? TABS[0]!);

  onMount(() => void start());
</script>

{#if app.settings && !app.settings.wizard_done}
  <Wizard />
{:else if app.settings}
  <div class="shell">
    <nav>
      <span class="brand">{S.app}</span>
      {#each TABS as tab (tab.id)}
        <button class="tab" class:active={tab.id === current.id} onclick={() => (app.tab = tab.id)}>{tab.label}</button>
      {/each}
      {#if app.live}<span class="testing warn">{S.overview.testing}</span>{/if}
    </nav>
    <main>
      <current.view />
    </main>
  </div>
{/if}

<Notices />
<ConfirmDialog />

<style>
  .shell {
    display: flex;
    flex-direction: column;
    height: 100vh;
  }

  nav {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 8px 16px;
    background: var(--panel);
    border-bottom: 1px solid var(--border);
  }

  .brand {
    margin-right: 16px;
    font-weight: 600;
  }

  .tab {
    border: none;
    background: none;
  }

  .tab.active {
    background: var(--hover);
    font-weight: 600;
  }

  .testing {
    margin-left: auto;
  }

  main {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 14px;
    padding: 16px;
    overflow: auto;
  }
</style>
```

- [ ] **Шаг 2: Обзор**

`frontend/src/tabs/Overview.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type ProxyStatus } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshOverview } from "$lib/app.svelte";
  import { ms, runDate } from "$lib/format";
  import { LINKS, S } from "$lib/strings";

  let proxy = $state<ProxyStatus | null>(null);
  let proxyBusy = $state(false);
  let upToDate = $state(false);

  async function checkProxy(): Promise<void> {
    const addr = app.settings?.socks5;
    if (!addr) return;
    proxyBusy = true;
    proxy = (await call(S.failed.proxy, commands.checkProxy(addr))) ?? null;
    proxyBusy = false;
  }

  async function checkUpdate(): Promise<void> {
    upToDate = (await change(S.failed.version, () => commands.checkFlowsealUpdate())) === null;
  }

  onMount(() => {
    void refreshOverview();
    void checkProxy();
  });
</script>

{#if app.overview}
  {@const o = app.overview}
  {@const locked = app.busy || o.testing}
  {#if o.folder_error}
    <section class="card">
      <h2 class="bad">{S.overview.folderProblem}</h2>
      <pre>{o.folder_error}</pre>
      <div class="buttons"><button onclick={() => (app.tab = "settings")}>{S.overview.chooseFolder}</button></div>
    </section>
  {/if}
  <div class="cards">
    <section class="card">
      <h2>{S.overview.service}</h2>
      <p class={o.service === "Running" ? "ok" : "bad"}>{S.svc[o.service]}</p>
      <p>{S.overview.strategy}: <strong>{o.strategy ?? S.overview.noStrategy}</strong></p>
      {#if o.service === "Running" && !o.winws_running}<p class="warn">{S.overview.winwsMissing}</p>{/if}
      {#if o.service === "StopPending" || o.windivert === "StopPending"}<p class="warn">{S.overview.stopPending}</p>{/if}
      <p>
        {S.overview.quick}:
        {#if o.quick}
          <span class={o.quick.ok ? "ok" : "bad"}>{o.quick.ok ? S.overview.quickOk : S.overview.quickFailed}</span>
        {:else}
          <span class="muted">{S.overview.quickNever}</span>
        {/if}
      </p>
      {#if o.quick}
        {#each o.quick.checks as item (item.name)}
          <p class="muted">{item.name} — {item.status === "Ok" ? ms(item.time_ms) : S.status[item.status]}</p>
        {/each}
      {/if}
      <div class="buttons">
        <button disabled={locked} onclick={() => change(S.failed.restart, () => commands.restartZapret())}>
          {S.overview.restart}
        </button>
        <button disabled={locked} onclick={() => change(S.failed.quick, () => commands.quickCheck())}>{S.overview.quick}</button>
      </div>
    </section>

    <section class="card">
      <h2>{S.overview.flowseal}</h2>
      <p>{S.overview.version(o.version)}</p>
      {#if o.update}
        {@const version = o.update}
        <p class="warn">{S.overview.updateAvailable(version)}</p>
        <button class="link" onclick={() => call(S.failed.open, commands.openUrl(LINKS.release(version)))}>
          {S.overview.release}
        </button>
      {:else if upToDate}
        <p class="ok">{S.overview.upToDate}</p>
      {/if}
      <div class="buttons"><button disabled={app.busy} onclick={checkUpdate}>{S.overview.checkUpdate}</button></div>
    </section>

    <section class="card">
      <h2>{S.overview.proxy}</h2>
      {#if !app.settings?.socks5}
        <p class="muted">{S.overview.proxyNotSet}</p>
      {:else}
        <p>{app.settings.socks5}</p>
        {#if proxyBusy}
          <p class="muted">{S.common.checking}</p>
        {:else if proxy}
          <p class={proxy.ok ? "ok" : "bad"}>{proxy.ok ? S.overview.proxyOk(ms(proxy.latency_ms)) : S.overview.proxyFailed}</p>
          {#if proxy.error}<pre>{proxy.error}</pre>{/if}
        {/if}
        <div class="buttons"><button disabled={proxyBusy} onclick={checkProxy}>{S.common.check}</button></div>
      {/if}
    </section>

    <section class="card">
      <h2>{S.overview.lastTest}</h2>
      {#if o.last_run?.best && o.last_run.best_score}
        {@const score = o.last_run.best_score}
        <p>{S.overview.best}: <strong>{o.last_run.best}</strong></p>
        <p>{S.overview.errors(score.total - score.ok, score.total, ms(score.median_ms))}</p>
        <p class="muted">{runDate(o.last_run.id)}</p>
      {:else}
        <p class="muted">{S.overview.noTests}</p>
      {/if}
      <div class="buttons">
        <button
          class="primary"
          disabled={locked || !o.last_run?.best}
          onclick={() => change(S.failed.apply, () => commands.applyBest())}
        >
          {S.overview.applyBest}
        </button>
        <button disabled={o.testing} onclick={() => (app.tab = "tests")}>{S.overview.runTests}</button>
      </div>
    </section>
  </div>
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}
```

- [ ] **Шаг 3: Настройки**

`frontend/src/tabs/Settings.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type FolderCheck, type ProxyStatus, type Settings, type Theme } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, loadSettings } from "$lib/app.svelte";
  import { ms } from "$lib/format";
  import { S } from "$lib/strings";

  const THEMES: Theme[] = ["system", "light", "dark"];

  // A copy to edit, saved as a whole.
  let draft = $state<Settings | null>(null);
  let folder = $state<FolderCheck | null>(null);
  let proxy = $state<ProxyStatus | null>(null);
  let saved = $state(false);
  let version = $state("");

  async function checkFolder(): Promise<void> {
    const path = draft?.zapret_dir?.trim();
    folder = path ? ((await call(S.failed.folder, commands.checkFolder(path))) ?? null) : null;
  }

  async function pick(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path && draft) {
      draft.zapret_dir = path;
      await checkFolder();
    }
  }

  async function checkProxy(): Promise<void> {
    proxy = null;
    const addr = draft?.socks5?.trim();
    if (addr) proxy = (await call(S.failed.proxy, commands.checkProxy(addr))) ?? null;
  }

  async function save(): Promise<void> {
    if (!draft) return;
    const settings: Settings = {
      ...draft,
      zapret_dir: draft.zapret_dir?.trim() || null,
      socks5: draft.socks5?.trim() || null,
    };
    saved = false;
    if ((await change(S.failed.settings, () => commands.saveSettings(settings))) !== undefined) {
      await loadSettings();
      saved = true;
    }
  }

  onMount(() => {
    if (app.settings) draft = { ...app.settings };
    void checkFolder();
    void commands.appVersion().then((v) => (version = v));
  });
</script>

{#if draft}
  <section class="card">
    <h2>{S.settings.folder}</h2>
    <div class="row">
      <input class="grow" bind:value={draft.zapret_dir} onchange={checkFolder} placeholder={S.settings.folderHint} />
      <button onclick={pick}>{S.settings.pick}</button>
    </div>
    {#if folder}
      <p class={folder.missing.length > 0 ? "bad" : "ok"}>
        {folder.missing.length > 0 ? S.settings.missing(folder.missing) : S.settings.folderOk(folder.version)}
      </p>
    {/if}
  </section>

  <section class="card">
    <h2>{S.settings.proxy}</h2>
    <p class="muted">{S.settings.proxyHint}</p>
    <div class="row">
      <input class="grow" bind:value={draft.socks5} placeholder="127.0.0.1:2080" aria-label={S.settings.proxyAddress} />
      <button onclick={checkProxy}>{S.common.check}</button>
    </div>
    {#if proxy}
      <p class={proxy.ok ? "ok" : "bad"}>{proxy.ok ? S.overview.proxyOk(ms(proxy.latency_ms)) : S.overview.proxyFailed}</p>
      {#if proxy.error}<pre>{proxy.error}</pre>{/if}
    {/if}
  </section>

  <section class="card">
    <h2>{S.settings.start}</h2>
    <label class="inline"><input type="checkbox" bind:checked={draft.autostart} /> {S.settings.autostart}</label>
    <label class="inline"><input type="checkbox" bind:checked={draft.start_minimized} /> {S.settings.minimized}</label>
    <div class="row">
      <label for="theme">{S.settings.theme}</label>
      <select id="theme" bind:value={draft.theme}>
        {#each THEMES as theme (theme)}<option value={theme}>{S.settings.themes[theme]}</option>{/each}
      </select>
    </div>
  </section>

  <!-- One button for the three cards above: they are saved together. -->
  <div class="buttons">
    <button class="primary" disabled={app.busy} onclick={save}>{S.common.save}</button>
    {#if saved}<span class="ok">{S.settings.saved}</span>{/if}
  </div>

  <section class="card">
    <h2>{S.settings.files}</h2>
    <div class="buttons">
      <button onclick={() => call(S.failed.open, commands.openFolder("Logs"))}>{S.settings.openLogs}</button>
      <button disabled={!app.settings?.zapret_dir} onclick={() => call(S.failed.open, commands.openFolder("Zapret"))}>
        {S.settings.openZapret}
      </button>
    </div>
    {#if version}<p class="muted">{S.settings.version(version)}</p>{/if}
  </section>
{/if}
```

- [ ] **Шаг 4: Мастер первого запуска**

`frontend/src/Wizard.svelte` (спецификация 9.2: папка, SOCKS5, автозапуск):

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type Candidates, type FolderCheck } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, loadSettings } from "$lib/app.svelte";
  import { S } from "$lib/strings";

  let step = $state(1);
  let candidates = $state<Candidates | null>(null);
  let folder = $state<FolderCheck | null>(null);
  let socks5 = $state("");
  let autostart = $state(true);
  let minimized = $state(true);

  onMount(async () => {
    candidates = (await call(S.failed.wizard, commands.wizardCandidates())) ?? { folders: [], proxies: [] };
    folder = candidates.folders.find((found) => found.missing.length === 0) ?? null;
    socks5 = candidates.proxies[0] ?? "";
  });

  async function pick(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path) folder = (await call(S.failed.folder, commands.checkFolder(path))) ?? null;
  }

  /** «Настроить позже» keeps the defaults but does not ask again. */
  async function finish(later: boolean): Promise<void> {
    if (!app.settings) return;
    const settings = later
      ? { ...app.settings, wizard_done: true }
      : {
          ...app.settings,
          zapret_dir: folder?.path ?? null,
          socks5: socks5.trim() || null,
          autostart,
          start_minimized: minimized,
          wizard_done: true,
        };
    if ((await change(S.failed.wizard, () => commands.saveSettings(settings))) !== undefined) await loadSettings();
  }
</script>

<div class="wizard card">
  <h1>{S.wizard.title}</h1>
  <p class="muted">{S.wizard.step(step, 3)}</p>

  {#if step === 1}
    <h2>{S.wizard.folderTitle}</h2>
    <p>{S.wizard.folderText}</p>
    {#if !candidates}<p class="muted">{S.wizard.searching}</p>{/if}
    {#each candidates?.folders ?? [] as found}
      <label class="choice">
        <input
          type="radio"
          name="folder"
          checked={folder?.path === found.path}
          disabled={found.missing.length > 0}
          onchange={() => (folder = found)}
        />
        <span>{found.path}</span>
        <span class={found.missing.length > 0 ? "bad" : "muted"}>
          {found.missing.length > 0 ? S.settings.missing(found.missing) : S.settings.folderOk(found.version)}
        </span>
      </label>
    {/each}
    {#if folder && !candidates?.folders.some((found) => found.path === folder?.path)}
      <p>
        {folder.path} —
        <span class={folder.missing.length > 0 ? "bad" : "ok"}>
          {folder.missing.length > 0 ? S.settings.missing(folder.missing) : S.settings.folderOk(folder.version)}
        </span>
      </p>
    {/if}
    <div class="buttons">
      <button onclick={pick}>{S.wizard.pickOther}</button>
      <span class="grow"></span>
      <button class="link" onclick={() => finish(true)}>{S.wizard.later}</button>
      <button class="primary" disabled={!folder || folder.missing.length > 0} onclick={() => (step = 2)}>
        {S.wizard.next}
      </button>
    </div>
  {:else if step === 2}
    <h2>{S.wizard.proxyTitle}</h2>
    <p>{S.wizard.proxyText}</p>
    {#each candidates?.proxies ?? [] as addr (addr)}
      <label class="choice">
        <input type="radio" name="proxy" checked={socks5 === addr} onchange={() => (socks5 = addr)} />
        {addr}
      </label>
    {/each}
    {#if candidates && candidates.proxies.length === 0}<p class="muted">{S.wizard.noProxies}</p>{/if}
    <div class="row">
      <input class="grow" bind:value={socks5} placeholder="127.0.0.1:2080" aria-label={S.settings.proxyAddress} />
    </div>
    <div class="buttons">
      <button onclick={() => (step = 1)}>{S.wizard.back}</button>
      <span class="grow"></span>
      <button
        onclick={() => {
          socks5 = "";
          step = 3;
        }}>{S.wizard.noProxy}</button
      >
      <button class="primary" onclick={() => (step = 3)}>{S.wizard.next}</button>
    </div>
  {:else}
    <h2>{S.wizard.autostartTitle}</h2>
    <label class="inline"><input type="checkbox" bind:checked={autostart} /> {S.settings.autostart}</label>
    <label class="inline"><input type="checkbox" bind:checked={minimized} /> {S.settings.minimized}</label>
    <div class="buttons">
      <button onclick={() => (step = 2)}>{S.wizard.back}</button>
      <span class="grow"></span>
      <button class="primary" disabled={app.busy} onclick={() => finish(false)}>{S.wizard.done}</button>
    </div>
  {/if}
</div>

<style>
  .wizard {
    max-width: 640px;
    margin: 48px auto;
  }
</style>
```

- [ ] **Шаг 5: Проверка**

Run: `npm run check`, затем `npm test`
Expected: 0 ошибок и 0 предупреждений; тесты задачи 16 проходят.

Вручную, из повышенного терминала, `npm run tauri dev` (старый `target\debug\zapret-hub.toml` удалить, чтобы увидеть мастер):
- мастер находит папку zapret из службы и порт NekoBox, если тот запущен; «Готово» сохраняет настройки, при включённом автозапуске в Планировщике появляется задача «Zapret Hub» (`schtasks /Query /TN "Zapret Hub"`);
- обзор показывает службу, стратегию, версию Flowseal, прокси; «Быстрая проверка» заполняет строку проверки, иконка трея становится зелёной;
- «Перезапустить zapret» перезапускает службу (в журнале `sc`/`net` с кодами выхода);
- в настройках смена темы применяется после «Сохранить»;
- неверный адрес прокси (`localhost:1`) при сохранении даёт уведомление с текстом ошибки, а не падение.

- [ ] **Шаг 6: Коммит**

```bash
git add frontend/src
git commit -m "feat(ui): add the overview, the settings and the first-run wizard"
```

---

### Задача 18: Вкладка «Стратегии» и редактор своих стратегий

**Files:**
- Create: `frontend/src/components/TargetChecks.svelte`, `frontend/src/tabs/Strategies.svelte`, `frontend/src/tabs/StrategyEditor.svelte`
- Modify: `frontend/src/App.svelte`

**Interfaces:**
- Consumes: `listStrategies`, `getStrategy`, `applyStrategy`, `cloneStrategy`, `validateCustom`, `saveCustom`, `deleteCustom`; `hubStem`, `blockSummary`, `quickSummary`, `statusClass`, `dpiClass`, `confirmation`, `change`
- Produces: `TargetChecks.svelte` со свойством `result: StrategyResult` (задача 19 показывает им подробности строки результата)

Своя стратегия редактируется только по записи: клонирование сразу создаёт запись и файл, редактор открывается на них. Имя записи не меняется: «переименовать» — это клонировать и удалить.

- [ ] **Шаг 1: Проверки по целям**

`frontend/src/components/TargetChecks.svelte`:

```svelte
<script lang="ts">
  import type { StrategyResult } from "$lib/ipc/bindings";
  import { dpiClass, ms, statusClass } from "$lib/format";
  import { S } from "$lib/strings";

  let { result }: { result: StrategyResult } = $props();

  const KINDS = ["Http", "Tls12", "Tls13"] as const;
</script>

{#if result.launch_error !== null}
  <pre class="bad">{result.launch_error}</pre>
{:else if result.dpi.length > 0}
  <table>
    <thead>
      <tr>
        <th>{S.results.target}</th>
        <th>{S.results.host}</th>
        {#each KINDS as kind (kind)}<th>{S.results.kinds[kind]}</th>{/each}
      </tr>
    </thead>
    <tbody>
      {#each result.dpi as target (target.id)}
        <tr>
          <td>{target.id} <span class="muted">{target.provider} {target.country}</span></td>
          <td>{target.host}</td>
          {#each KINDS as kind (kind)}
            {@const check = target.checks.find((c) => c.kind === kind)}
            <td class={check ? dpiClass[check.status] : "muted"}>{check ? S.dpiStatus[check.status] : "—"}</td>
          {/each}
        </tr>
      {/each}
    </tbody>
  </table>
{:else}
  <table>
    <thead>
      <tr>
        <th>{S.results.target}</th>
        {#each KINDS as kind (kind)}<th>{S.results.kinds[kind]}</th>{/each}
        <th>{S.results.ping}</th>
      </tr>
    </thead>
    <tbody>
      {#each result.targets as target}
        <tr>
          <td>{target.name} {#if target.extra}<span class="muted">{S.results.extra}</span>{/if}</td>
          {#each KINDS as kind (kind)}
            {@const check = target.checks.find((c) => c.kind === kind)}
            <td class={check ? statusClass[check.status] : "muted"} title={check?.stderr ?? ""}>
              {#if check}{check.status === "Ok" ? ms(check.time_ms) : S.status[check.status]}{:else}—{/if}
            </td>
          {/each}
          <td class="muted">{ms(target.ping_ms)}</td>
        </tr>
      {/each}
    </tbody>
  </table>
{/if}
```

- [ ] **Шаг 2: Редактор**

`frontend/src/tabs/StrategyEditor.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type CustomStrategy } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { blockSummary, quickSummary } from "$lib/format";
  import { S } from "$lib/strings";

  interface Props {
    /** The file stem, `general (HUB <name>)`. */
    stem: string;
    onClose: () => void;
  }

  let { stem, onClose }: Props = $props();

  let draft = $state<CustomStrategy | null>(null);
  let status = $state("");

  onMount(async () => {
    draft = (await call(S.failed.strategy, commands.getStrategy(stem)))?.record ?? null;
  });

  function move(index: number, by: number): void {
    if (!draft) return;
    const to = index + by;
    if (to < 0 || to >= draft.blocks.length) return;
    const [block] = draft.blocks.splice(index, 1);
    draft.blocks.splice(to, 0, block ?? "");
  }

  function duplicate(index: number): void {
    if (draft) draft.blocks.splice(index + 1, 0, draft.blocks[index] ?? "");
  }

  function removeBlock(index: number): void {
    if (draft) draft.blocks.splice(index, 1);
  }

  async function validate(): Promise<void> {
    if (!draft) return;
    const strategy = draft;
    status = "";
    if ((await change(S.failed.validate, () => commands.validateCustom(strategy))) !== undefined) status = S.editor.valid;
  }

  async function save(): Promise<void> {
    if (!draft) return;
    const strategy = draft;
    status = "";
    const check = await change(S.failed.saveCustom, () => commands.saveCustom(strategy, false));
    if (check === undefined) return;
    // The active strategy was installed again: its quick check is worth reading before closing.
    if (check) status = `${check.ok ? S.editor.appliedOk : S.editor.appliedFailed}\n${quickSummary(check)}`;
    else onClose();
  }
</script>

{#if draft}
  <h2>{stem}</h2>
  <p class="muted">{S.editor.hint}</p>
  <label for="wf">{S.editor.filter}</label>
  <input id="wf" class="wide" bind:value={draft.wf} />
  {#each draft.blocks as block, i (i)}
    <div class="block">
      <div class="row">
        <strong>{S.strategies.block(i + 1)}</strong>
        <span class="muted">{blockSummary(block)}</span>
        <span class="grow"></span>
        <button title={S.editor.up} aria-label={S.editor.up} disabled={i === 0} onclick={() => move(i, -1)}>↑</button>
        <button
          title={S.editor.down}
          aria-label={S.editor.down}
          disabled={i === draft.blocks.length - 1}
          onclick={() => move(i, 1)}>↓</button
        >
        <button onclick={() => duplicate(i)}>{S.editor.duplicate}</button>
        <button class="danger" disabled={draft.blocks.length === 1} onclick={() => removeBlock(i)}>{S.editor.remove}</button>
      </div>
      <textarea rows="3" bind:value={draft.blocks[i]} aria-label={S.strategies.block(i + 1)}></textarea>
    </div>
  {/each}
  <div class="buttons">
    <button onclick={() => draft?.blocks.push("")}>{S.editor.add}</button>
    <span class="grow"></span>
    <button disabled={app.busy} onclick={validate}>{S.editor.validate}</button>
    <button class="primary" disabled={app.busy} onclick={save}>{S.common.save}</button>
    <button onclick={onClose}>{S.common.close}</button>
  </div>
  {#if status}<pre>{status}</pre>{/if}
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}

<style>
  .block {
    margin: 10px 0;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
</style>
```

- [ ] **Шаг 3: Вкладка**

`frontend/src/tabs/Strategies.svelte`:

```svelte
<script lang="ts">
  import { commands, type StrategyDetail, type StrategyRow } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { blockSummary, hubStem, ms, runDate } from "$lib/format";
  import { S } from "$lib/strings";
  import TargetChecks from "../components/TargetChecks.svelte";
  import StrategyEditor from "./StrategyEditor.svelte";

  let rows = $state<StrategyRow[]>([]);
  let query = $state("");
  let detail = $state<StrategyDetail | null>(null);
  /** The stem of the custom strategy under edit. */
  let editing = $state<string | null>(null);
  let cloneName = $state("");

  const visible = $derived(rows.filter((row) => row.name.toLowerCase().includes(query.trim().toLowerCase())));
  const locked = $derived(app.busy || (app.overview?.testing ?? false));

  async function open(name: string): Promise<void> {
    detail = (await call(S.failed.strategy, commands.getStrategy(name))) ?? null;
  }

  async function load(): Promise<void> {
    rows = (await call(S.failed.strategies, commands.listStrategies())) ?? [];
    const name = detail?.row.name;
    if (name) await open(name);
  }

  $effect(() => {
    void app.revision;
    void load();
  });

  async function clone(source: string): Promise<void> {
    const name = cloneName.trim();
    const created = await change(S.failed.clone, () => commands.cloneStrategy(source, name));
    if (!created) return;
    cloneName = "";
    await open(hubStem(created.name));
    editing = hubStem(created.name);
  }

  async function remove(record: string, stem: string): Promise<void> {
    const sure = await confirmation.ask({
      title: S.strategies.deleteTitle,
      text: S.strategies.deleteText(stem),
      yes: S.strategies.delete,
      danger: true,
    });
    if (sure && (await change(S.failed.deleteCustom, () => commands.deleteCustom(record))) !== undefined) detail = null;
  }
</script>

<div class="split">
  <section class="card scroll">
    <input class="wide" placeholder={S.strategies.search} bind:value={query} aria-label={S.strategies.search} />
    <table>
      <thead>
        <tr><th>{S.strategies.name}</th><th>{S.strategies.result}</th></tr>
      </thead>
      <tbody>
        {#each visible as row (row.name)}
          <tr class:selected={detail?.row.name === row.name}>
            <td>
              <button
                class="link"
                onclick={() => {
                  editing = null;
                  void open(row.name);
                }}>{row.name}</button
              >
              {#if row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
              {#if row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
            </td>
            <td>{@render summary(row)}</td>
          </tr>
        {/each}
      </tbody>
    </table>
  </section>

  <section class="card scroll">
    {#if editing}
      <StrategyEditor
        stem={editing}
        onClose={() => {
          editing = null;
          void load();
        }}
      />
    {:else if detail}
      {@const d = detail}
      <h2>
        {d.row.name}
        {#if d.row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
        {#if d.row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
        {#if d.row.last?.stale}<span class="badge warn">{S.strategies.stale}</span>{/if}
      </h2>
      {#if d.record}<p class="muted">{S.strategies.clonedFrom(d.record.cloned_from)}</p>{/if}
      <div class="buttons">
        <button
          class="primary"
          disabled={locked || d.row.error !== null}
          onclick={() => change(S.failed.apply, () => commands.applyStrategy(d.row.name))}>{S.common.apply}</button
        >
        {#if d.record}
          {@const record = d.record}
          <button disabled={locked} onclick={() => (editing = d.row.name)}>{S.strategies.edit}</button>
          <button class="danger" disabled={locked || d.row.active} onclick={() => remove(record.name, d.row.name)}>
            {S.strategies.delete}
          </button>
        {/if}
      </div>
      <div class="buttons">
        <input placeholder={S.strategies.cloneName} bind:value={cloneName} aria-label={S.strategies.cloneName} />
        <button disabled={locked || !cloneName.trim() || d.row.error !== null} onclick={() => clone(d.row.name)}>
          {S.strategies.clone}
        </button>
      </div>
      {#if d.row.error}<pre class="bad">{d.row.error}</pre>{/if}
      {#if d.wf}
        <h3>{S.strategies.filter}</h3>
        <pre>{d.wf}</pre>
      {/if}
      {#each d.blocks as block, i (i)}
        <h3>{S.strategies.block(i + 1)} <span class="muted">· {blockSummary(block)}</span></h3>
        <pre>{block}</pre>
      {/each}
      {#if d.final_args}
        <details>
          <summary>{S.strategies.finalArgs}</summary>
          <pre>{d.final_args}</pre>
        </details>
      {/if}
      {#if d.result}
        <h3>{S.strategies.lastRun(runDate(d.row.last?.run_id ?? ""))}</h3>
        <TargetChecks result={d.result} />
      {/if}
    {:else}
      <p class="muted">{S.strategies.pick}</p>
    {/if}
  </section>
</div>

{#snippet summary(row: StrategyRow)}
  {#if row.error}
    <span class="bad">{S.strategies.notStrategy}</span>
  {:else if !row.last}
    <span class="muted">{S.strategies.untested}</span>
  {:else if row.last.launch_error}
    <span class="bad">{S.strategies.launchError}</span>
  {:else}
    {@const score = row.last.score}
    <span class={score.total - score.ok === 0 ? "ok" : "warn"}>
      {row.last.suite === "Standard"
        ? S.strategies.score(score.total - score.ok, score.total, ms(score.median_ms))
        : S.strategies.dpiScore(score.likely_blocked, score.fail)}
    </span>
    {#if row.last.stale}<span class="badge warn">{S.strategies.stale}</span>{/if}
  {/if}
{/snippet}

<style>
  .split {
    display: grid;
    grid-template-columns: minmax(340px, 2fr) 3fr;
    gap: 14px;
    min-height: 0;
    flex: 1;
  }

  .scroll {
    overflow: auto;
  }
</style>
```

В `frontend/src/App.svelte` добавить `import Strategies from "./tabs/Strategies.svelte";` и в `TABS` сразу после строки `overview`:

```ts
    { id: "strategies", label: S.tabs.strategies, view: Strategies },
```

- [ ] **Шаг 4: Проверка**

Run: `npm run check`
Expected: 0 ошибок и 0 предупреждений.

Вручную, из повышенного терминала, `npm run tauri dev`:
- список в порядке меню `service.bat`, поиск по «ALT» оставляет ALT-стратегии; активная помечена;
- «Применить» на другой стратегии меняет службу (`sc qc zapret` показывает новые аргументы), в обзоре — новая стратегия и результат быстрой проверки;
- «Клонировать» с именем `test` создаёт `general (HUB test).bat` рядом с оригиналами и открывает редактор; переставить блоки, «Проверить» → «Проверка пройдена»; дописать в блок `^` → «Проверить» показывает ошибку разбора целиком;
- клон виден в меню `service.bat` («Install Service»), «Удалить» убирает и файл, и `data\strategies\test.toml`; активную своя стратегию удалить нельзя (кнопка выключена).

- [ ] **Шаг 5: Коммит**

```bash
git add frontend/src
git commit -m "feat(ui): add the strategies tab and the custom strategy editor"
```

---

### Задача 19: Вкладка «Тесты»: запуск, живой прогресс, результаты, история

**Files:**
- Create: `frontend/src/components/ResultsTable.svelte`, `frontend/src/tabs/Tests.svelte`
- Modify: `frontend/src/App.svelte`

**Interfaces:**
- Consumes: `listStrategies`, `startTests`, `cancelTests`, `listRuns`, `getRun`, `runReport`, `applyStrategy`; `app.live`, `app.finished`, `refreshLive`, `sortBy`, `TargetChecks`
- Produces: `ResultsTable.svelte` со свойствами `{ suite: Suite; results: StrategyResult[]; onApply?: (name) => void; disabled?: boolean }`

Таблица законченного прогона приходит отсортированной по рейтингу (задача 11): столбец «#» — это место в рейтинге, щелчок по заголовку сортирует по любому столбцу. Живая таблица показывает результаты в порядке прихода. «Применить лучшую» на вкладке ставит первую строку показанного прогона — спецификация 7.4; на обзоре — первую строку последнего стандартного прогона.

- [ ] **Шаг 1: Таблица результатов**

`frontend/src/components/ResultsTable.svelte`:

```svelte
<script lang="ts">
  import type { StrategyResult, Suite } from "$lib/ipc/bindings";
  import { ms } from "$lib/format";
  import { sortBy, type Direction } from "$lib/sort";
  import { S } from "$lib/strings";
  import TargetChecks from "./TargetChecks.svelte";

  interface Props {
    suite: Suite;
    /** Best first for a finished run; in arrival order for a live one. */
    results: StrategyResult[];
    onApply?: (name: string) => void;
    disabled?: boolean;
  }

  let { suite, results, onApply, disabled = false }: Props = $props();

  type Key = "rank" | "name" | "errors" | "latency" | "ping" | "discord" | "youtube" | "dbd" | "blocked" | "fail" | "ok";
  interface Item {
    result: StrategyResult;
    rank: number;
  }

  const STANDARD: Key[] = ["rank", "name", "errors", "latency", "ping", "discord", "youtube", "dbd"];
  const DPI: Key[] = ["rank", "name", "blocked", "fail", "ok"];
  // A strategy that did not start sorts after every one that did.
  const WORST = Number.MAX_SAFE_INTEGER;

  let key = $state<Key>("rank");
  let dir = $state<Direction>("asc");
  let open = $state<string | null>(null);

  const columns = $derived(suite === "Standard" ? STANDARD : DPI);
  const failed = (part: { ok: number; total: number }) => part.total - part.ok;

  function value({ result, rank }: Item): number | string | null {
    const s = result.score;
    const broken = result.launch_error !== null;
    switch (key) {
      case "rank":
        return rank;
      case "name":
        return result.name;
      case "errors":
        return broken ? WORST : failed(s);
      case "latency":
        return broken ? null : s.median_ms;
      case "ping":
        return s.ping_median_ms;
      case "discord":
        return s.discord.total > 0 ? failed(s.discord) : null;
      case "youtube":
        return s.youtube.total > 0 ? failed(s.youtube) : null;
      case "dbd":
        return s.dbd.median_ms;
      case "blocked":
        return broken ? WORST : s.likely_blocked;
      case "fail":
        return broken ? WORST : s.fail;
      case "ok":
        return broken ? null : -s.ok;
    }
  }

  const items = $derived(sortBy(results.map((result, index) => ({ result, rank: index + 1 })), value, dir));

  function sortOn(column: Key): void {
    if (key === column) {
      dir = dir === "asc" ? "desc" : "asc";
    } else {
      key = column;
      dir = "asc";
    }
  }
</script>

<table>
  <thead>
    <tr>
      {#each columns as column (column)}
        <th>
          <button class="head" onclick={() => sortOn(column)}>
            {S.results[column]}{key === column ? (dir === "asc" ? " ↑" : " ↓") : ""}
          </button>
        </th>
      {/each}
      {#if onApply}<th></th>{/if}
    </tr>
  </thead>
  <tbody>
    {#each items as { result, rank } (result.name)}
      <tr class:selected={open === result.name}>
        {#each columns as column (column)}
          <td>{@render cell(column, result, rank)}</td>
        {/each}
        {#if onApply}
          <td><button {disabled} onclick={() => onApply?.(result.name)}>{S.common.apply}</button></td>
        {/if}
      </tr>
      {#if open === result.name}
        <tr>
          <td colspan={columns.length + (onApply ? 1 : 0)}><TargetChecks {result} /></td>
        </tr>
      {/if}
    {/each}
  </tbody>
</table>

{#snippet cell(column: Key, result: StrategyResult, rank: number)}
  {@const s = result.score}
  {#if column === "rank"}
    {rank}
  {:else if column === "name"}
    <button class="link" onclick={() => (open = open === result.name ? null : result.name)}>{result.name}</button>
  {:else if result.launch_error !== null}
    {#if column === "errors" || column === "blocked"}<span class="bad">{S.results.launch}</span>{/if}
  {:else if column === "errors"}
    <span class={failed(s) === 0 ? "ok" : "bad"} title={S.results.breakdown(s)}>{failed(s)} / {s.total}</span>
  {:else if column === "latency"}
    {ms(s.median_ms)}
  {:else if column === "ping"}
    {ms(s.ping_median_ms)} <span class="muted">{s.ping_ok}/{s.ping_total}</span>
  {:else if column === "discord"}
    {@render profile(s.discord.ok, s.discord.total)}
  {:else if column === "youtube"}
    {@render profile(s.youtube.ok, s.youtube.total)}
  {:else if column === "dbd"}
    {ms(s.dbd.median_ms)}
    {#if failed(s.dbd) > 0}<span class="bad">−{failed(s.dbd)}</span>{/if}
  {:else if column === "blocked"}
    <span class={s.likely_blocked > 0 ? "bad" : "ok"}>{s.likely_blocked}</span>
  {:else if column === "fail"}
    {s.fail}
  {:else}
    {s.ok} / {s.total}
  {/if}
{/snippet}

{#snippet profile(ok: number, total: number)}
  {#if total > 0}<span class={ok === total ? "ok" : "warn"}>{ok}/{total}</span>{:else}—{/if}
{/snippet}

<style>
  .head {
    padding: 0;
    border: none;
    background: none;
    color: inherit;
    font-weight: 600;
  }
</style>
```

- [ ] **Шаг 2: Вкладка**

`frontend/src/tabs/Tests.svelte`:

```svelte
<script lang="ts">
  import { commands, type RunSummary, type Score, type StrategyRow, type Suite, type TestRun } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change, refreshLive } from "$lib/app.svelte";
  import { estimateMinutes, ms, runDate } from "$lib/format";
  import { S } from "$lib/strings";
  import ResultsTable from "../components/ResultsTable.svelte";

  let rows = $state<StrategyRow[]>([]);
  let suite = $state<Suite>("Standard");
  let chosen = $state<string[]>([]);
  let runs = $state<RunSummary[]>([]);
  let shown = $state<TestRun | null>(null);
  let copied = $state(false);
  // The selection is set once; later reloads keep what the user chose.
  let picked = false;

  const usable = (list: StrategyRow[]) => list.filter((row) => row.error === null).map((row) => row.name);
  const suiteName = (value: Suite) => (value === "Standard" ? S.tests.standard : S.tests.dpi);

  async function openRun(id: string): Promise<void> {
    shown = (await call(S.failed.run, commands.getRun(id))) ?? null;
    copied = false;
  }

  async function load(): Promise<void> {
    rows = (await call(S.failed.strategies, commands.listStrategies())) ?? [];
    runs = (await call(S.failed.runs, commands.listRuns())) ?? [];
    if (!picked && rows.length > 0) {
      chosen = usable(rows);
      picked = true;
    }
    if (!shown && runs[0]) await openRun(runs[0].id);
  }

  $effect(() => {
    void app.revision;
    void load();
  });

  $effect(() => {
    const id = app.finished;
    if (id) {
      app.finished = null;
      void openRun(id);
    }
  });

  async function start(): Promise<void> {
    // In the menu's order, whatever order they were ticked in.
    const strategies = rows.filter((row) => chosen.includes(row.name)).map((row) => row.name);
    if ((await change(S.failed.startTests, () => commands.startTests({ suite, strategies }))) !== undefined) {
      await refreshLive();
    }
  }

  async function copy(id: string): Promise<void> {
    const text = await call(S.failed.report, commands.runReport(id));
    if (text === undefined) return;
    await navigator.clipboard.writeText(text);
    copied = true;
  }

  function apply(name: string): void {
    void change(S.failed.apply, () => commands.applyStrategy(name));
  }
</script>

{#if app.live}
  {@const live = app.live}
  {@const total = live.request.strategies.length}
  <section class="card">
    <h2>{suiteName(live.request.suite)}</h2>
    {#if live.restoring}
      <p>{S.tests.restoring}</p>
    {:else if live.current !== null}
      <p>{S.tests.progress(live.current + 1, total, live.request.strategies[live.current] ?? "")}</p>
    {:else}
      <p>{S.tests.preparing}</p>
    {/if}
    <progress class="wide" max={total} value={live.results.length}></progress>
    <div class="buttons">
      <button class="danger" disabled={live.restoring} onclick={() => commands.cancelTests()}>{S.common.cancel}</button>
    </div>
  </section>
  <section class="card">
    <ResultsTable suite={live.request.suite} results={live.results} />
  </section>
{:else}
  <section class="card">
    <h2>{S.tests.newRun}</h2>
    <div class="row">
      <label class="inline"><input type="radio" bind:group={suite} value="Standard" /> {S.tests.standard}</label>
      <label class="inline"><input type="radio" bind:group={suite} value="Dpi" /> {S.tests.dpi}</label>
    </div>
    <p class="muted">{suite === "Standard" ? S.tests.standardHint : S.tests.dpiHint}</p>
    <div class="buttons">
      <button onclick={() => (chosen = usable(rows))}>{S.tests.all}</button>
      <button onclick={() => (chosen = [])}>{S.tests.none}</button>
    </div>
    <div class="choices">
      {#each rows as row (row.name)}
        <label class="inline">
          <input type="checkbox" bind:group={chosen} value={row.name} disabled={row.error !== null} />
          {row.name}
        </label>
      {/each}
    </div>
    <div class="buttons">
      <button class="primary" disabled={app.busy || chosen.length === 0} onclick={start}>{S.tests.start}</button>
      <span class="muted">{S.tests.estimate(chosen.length, estimateMinutes(chosen.length, suite))}</span>
    </div>
  </section>

  {#if shown}
    {@const run = shown}
    {@const first = run.strategies[0]}
    <section class="card">
      <h2>{S.tests.runTitle(runDate(run.id), suiteName(run.suite))}</h2>
      <p class="muted">
        {S.tests.flowseal(run.flowseal_version)}{#if run.canceled}{" · "}{S.tests.canceled}{/if}
      </p>
      {#if run.restore_error}<pre class="bad">{run.restore_error}</pre>{/if}
      <div class="buttons">
        {#if first && first.launch_error === null}
          <button class="primary" disabled={app.busy} onclick={() => apply(first.name)}>{S.tests.applyBest(first.name)}</button>
        {/if}
        <button onclick={() => copy(run.id)}>{S.common.copy}</button>
        {#if copied}<span class="ok">{S.tests.copied}</span>{/if}
      </div>
      <ResultsTable suite={run.suite} results={run.strategies} onApply={apply} disabled={app.busy} />
    </section>
  {/if}
{/if}

<section class="card">
  <h2>{S.tests.history}</h2>
  {#if runs.length === 0}<p class="muted">{S.tests.noRuns}</p>{/if}
  <table>
    <tbody>
      {#each runs as summary (summary.id)}
        <tr class:selected={shown?.id === summary.id}>
          <td><button class="link" onclick={() => openRun(summary.id)}>{runDate(summary.id)}</button></td>
          <td>{suiteName(summary.suite)}{#if summary.canceled}{" · "}{S.tests.canceled}{/if}</td>
          <td>{S.tests.runCount(summary.strategies)}</td>
          <td>{summary.best ?? "—"}</td>
          <td>{#if summary.best_score}{@render score(summary.suite, summary.best_score)}{/if}</td>
        </tr>
      {/each}
    </tbody>
  </table>
</section>

{#snippet score(kind: Suite, value: Score)}
  {kind === "Standard"
    ? S.tests.runScore(value.total - value.ok, value.total, ms(value.median_ms))
    : S.strategies.dpiScore(value.likely_blocked, value.fail)}
{/snippet}

<style>
  .choices {
    columns: 3 220px;
    margin-top: 8px;
  }
</style>
```

В `frontend/src/App.svelte` добавить `import Tests from "./tabs/Tests.svelte";` и в `TABS` сразу после строки `strategies`:

```ts
    { id: "tests", label: S.tabs.tests, view: Tests },
```

- [ ] **Шаг 3: Проверка**

Run: `npm run check`
Expected: 0 ошибок и 0 предупреждений.

Вручную, из повышенного терминала, `npm run tauri dev`:
- выбрать 3 стратегии, «Запустить»: строка «Стратегия N из 3», полоса, таблица заполняется по мере прогона; в шапке «Идут тесты…»; кнопки «Применить» на других вкладках выключены;
- закрыть окно крестиком посреди прогона и открыть из трея: прогресс на месте;
- после конца — уведомление Windows «Тесты завершены» с лучшей стратегией; прогон открыт, «#» — место в рейтинге, щелчок по «HTTPS» сортирует по задержке, по имени стратегии раскрываются проверки по целям;
- «Копировать» кладёт в буфер текст в формате `test zapret.ps1`; служба zapret после прогона снова работает;
- «Отменить» посреди прогона: строка «Возвращаю…», затем прогон с пометкой «отменён», служба работает.

- [ ] **Шаг 4: Коммит**

```bash
git add frontend/src
git commit -m "feat(ui): add the tests tab with live progress, ranking and history"
```

---

### Задача 20: Вкладка «Zapret»: настройки Flowseal, обновления, проверки, кэш Discord

**Files:**
- Create: `frontend/src/components/ListEditor.svelte`, `frontend/src/tabs/Zapret.svelte`
- Modify: `frontend/src/App.svelte`

**Interfaces:**
- Consumes: `getFlowseal`, `setGameFilter`, `setIpsetMode`, `setCheckUpdates`, `replaceFake`, `addListEntry`, `removeListEntry`, `updateIpset`, `hostsPlan`, `applyHosts`, `systemChecks`, `fixCheck`, `clearDiscordCache`, `removeService`, `openUrl`; `confirmation`, `change`, `levelClass`
- Produces: `ListEditor.svelte` со свойствами `{ list: UserList; entries: string[]; disabled: boolean }`

- [ ] **Шаг 1: Редактор своего списка**

`frontend/src/components/ListEditor.svelte`:

```svelte
<script lang="ts">
  import { commands, type UserList } from "$lib/ipc/bindings";
  import { change } from "$lib/app.svelte";
  import { S } from "$lib/strings";

  interface Props {
    list: UserList;
    entries: string[];
    disabled: boolean;
  }

  let { list, entries, disabled }: Props = $props();
  let entry = $state("");

  const text = $derived(S.zapret.lists[list]);

  async function add(event: SubmitEvent): Promise<void> {
    event.preventDefault();
    const value = entry.trim();
    if (value && (await change(S.failed.list, () => commands.addListEntry(list, value))) !== undefined) entry = "";
  }
</script>

<div>
  <h3>{text.title} <span class="muted">{text.file}</span></h3>
  <p class="muted">{text.hint}</p>
  <form class="row" onsubmit={add}>
    <input class="grow" placeholder={text.placeholder} bind:value={entry} {disabled} aria-label={text.title} />
    <button type="submit" disabled={disabled || !entry.trim()}>{S.zapret.add}</button>
  </form>
  <ul>
    {#each entries as value}
      <li>
        <code>{value}</code>
        <button class="link" {disabled} onclick={() => change(S.failed.list, () => commands.removeListEntry(list, value))}>
          {S.zapret.removeEntry}
        </button>
      </li>
    {/each}
  </ul>
</div>
```

- [ ] **Шаг 2: Вкладка**

`frontend/src/tabs/Zapret.svelte`:

```svelte
<script lang="ts">
  import {
    commands,
    type CacheReport,
    type CheckAction,
    type FakeKind,
    type FlowsealState,
    type GameFilter,
    type GameFilterMode,
    type IpsetMode,
    type SystemCheck,
  } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { levelClass } from "$lib/format";
  import { S } from "$lib/strings";
  import ListEditor from "../components/ListEditor.svelte";

  const MODES: GameFilterMode[] = ["disabled", "all", "tcp", "udp"];
  const IPSET: IpsetMode[] = ["loaded", "none", "any"];
  const FAKES: FakeKind[] = ["Discord", "Game"];
  const RISKY: CheckAction[] = ["RemoveWinDivert", "RemoveConflicts"];

  let flowseal = $state<FlowsealState | null>(null);
  let filter = $state<GameFilter | null>(null);
  let checks = $state<SystemCheck[] | null>(null);
  let cache = $state<CacheReport[] | null>(null);
  let status = $state("");

  const locked = $derived(app.busy || (app.overview?.testing ?? false));

  async function load(): Promise<void> {
    flowseal = (await call(S.failed.flowseal, commands.getFlowseal())) ?? null;
    if (flowseal) filter = { ...flowseal.game_filter };
  }

  $effect(() => {
    void app.revision;
    void load();
  });

  async function applyFilter(): Promise<void> {
    if (!filter) return;
    const chosen = filter;
    await change(S.failed.gameFilter, () => commands.setGameFilter(chosen));
  }

  async function updateIpset(): Promise<void> {
    status = "";
    if ((await change(S.failed.updateIpset, () => commands.updateIpset())) !== undefined) status = S.zapret.ipsetUpdated;
  }

  /** Shows what changes before anything is written (spec 5.6). */
  async function updateHosts(): Promise<void> {
    status = "";
    const plan = await change(S.failed.hosts, () => commands.hostsPlan());
    if (!plan) return;
    if (!plan.needs_update) {
      status = S.zapret.hostsCurrent;
      return;
    }
    const diff = [...plan.added.map((line) => `+ ${line}`), ...plan.removed.map((line) => `− ${line}`)].join("\n");
    const sure = await confirmation.ask({ title: S.zapret.hostsTitle, text: diff, yes: S.zapret.hostsApply });
    if (sure && (await change(S.failed.hosts, () => commands.applyHosts())) !== undefined) status = S.zapret.hostsUpdated;
  }

  async function runChecks(): Promise<void> {
    checks = (await change(S.failed.checks, () => commands.systemChecks())) ?? null;
  }

  async function fix(check: SystemCheck, action: CheckAction): Promise<void> {
    if (RISKY.includes(action)) {
      const sure = await confirmation.ask({
        title: S.checks.actions[action],
        text: S.checks.confirm(action, check.detail),
        yes: S.checks.actions[action],
        danger: true,
      });
      if (!sure) return;
    }
    if ((await change(S.failed.fix, () => commands.fixCheck(action))) !== undefined) await runChecks();
  }

  async function clearCache(): Promise<void> {
    const sure = await confirmation.ask({ title: S.zapret.cacheTitle, text: S.zapret.cacheText, yes: S.zapret.cacheClear });
    if (sure) cache = (await change(S.failed.cache, () => commands.clearDiscordCache())) ?? null;
  }

  async function removeService(): Promise<void> {
    const sure = await confirmation.ask({
      title: S.zapret.removeTitle,
      text: `${S.zapret.serviceHint}\n\n${S.zapret.removeText}`,
      yes: S.zapret.remove,
      danger: true,
    });
    if (sure) await change(S.failed.removeService, () => commands.removeService());
  }
</script>

{#if flowseal && filter}
  {@const f = flowseal}
  <div class="cards">
    <section class="card">
      <h2>{S.zapret.gameFilter}</h2>
      <select bind:value={filter.mode} disabled={locked} aria-label={S.zapret.gameFilter}>
        {#each MODES as mode (mode)}<option value={mode}>{S.zapret.gameModes[mode]}</option>{/each}
      </select>
      {#if filter.mode !== "disabled"}
        <div class="row">
          <label for="tcp">{S.zapret.tcpRange}</label>
          <input id="tcp" bind:value={filter.tcp} disabled={locked} />
          <label for="udp">{S.zapret.udpRange}</label>
          <input id="udp" bind:value={filter.udp} disabled={locked} />
        </div>
      {/if}
      <p class="muted">{S.zapret.gameHint}</p>
      <div class="buttons"><button class="primary" disabled={locked} onclick={applyFilter}>{S.common.apply}</button></div>
    </section>

    <section class="card">
      <h2>{S.zapret.ipset}</h2>
      {#each IPSET as mode (mode)}
        <label class="choice">
          <input
            type="radio"
            name="ipset"
            checked={f.ipset === mode}
            disabled={locked || (mode === "loaded" && f.ipset !== "loaded" && !f.ipset_backup)}
            onchange={() => change(S.failed.ipset, () => commands.setIpsetMode(mode))}
          />
          {S.zapret.ipsetModes[mode]}
        </label>
      {/each}
      {#if f.ipset !== "loaded" && !f.ipset_backup}<p class="warn">{S.zapret.noBackup}</p>{/if}
      <p class="muted">{S.zapret.ipsetHint}</p>
    </section>

    <section class="card">
      <h2>{S.zapret.updates}</h2>
      <label class="inline">
        <input
          type="checkbox"
          checked={f.check_updates}
          disabled={locked}
          onchange={(event) => change(S.failed.checkUpdates, () => commands.setCheckUpdates(event.currentTarget.checked))}
        />
        {S.zapret.checkUpdates}
      </label>
      <div class="buttons">
        <button disabled={locked} onclick={updateIpset}>{S.zapret.updateIpset}</button>
        <button disabled={locked} onclick={updateHosts}>{S.zapret.updateHosts}</button>
      </div>
      {#if status}<p class="ok">{status}</p>{/if}
    </section>

    <section class="card">
      <h2>{S.zapret.fakesTitle}</h2>
      {#each FAKES as kind (kind)}
        {@const current = kind === "Discord" ? f.fakes.discord : f.fakes.game}
        <div class="row">
          <label for={`fake-${kind}`}>{S.zapret.fakes[kind]}</label>
          <select
            id={`fake-${kind}`}
            value={current ?? ""}
            disabled={locked}
            onchange={(event) => change(S.failed.fake, () => commands.replaceFake(kind, event.currentTarget.value))}
          >
            {#if !current}<option value="">{S.zapret.fakeUnknown}</option>{/if}
            {#each f.fakes.files as file (file)}<option value={file}>{file}</option>{/each}
          </select>
        </div>
      {/each}
    </section>

    <section class="card">
      <h2>{S.zapret.service}</h2>
      <p class="muted">{S.zapret.serviceHint}</p>
      <div class="buttons"><button class="danger" disabled={locked} onclick={removeService}>{S.zapret.remove}</button></div>
    </section>

    <section class="card">
      <h2>{S.zapret.cache}</h2>
      <div class="buttons"><button disabled={app.busy} onclick={clearCache}>{S.zapret.cacheButton}</button></div>
      {#if cache}
        {#if cache.length === 0}<p class="muted">{S.zapret.cacheNone}</p>{/if}
        {#each cache as report (report.install)}
          <h3>{report.install} {#if report.closed}<span class="muted">{S.zapret.cacheClosed}</span>{/if}</h3>
          {#each report.dirs as dir (dir.path)}
            <p class={dir.outcome === "Failed" ? "bad" : "muted"}>
              {dir.path} — {S.zapret.cacheOutcome[dir.outcome]}{dir.error ? `: ${dir.error}` : ""}
            </p>
          {/each}
        {/each}
      {/if}
    </section>
  </div>

  <section class="card">
    <h2>{S.zapret.listsTitle}</h2>
    <div class="cards">
      {#each f.lists as item (item.list)}
        <ListEditor list={item.list} entries={item.entries} disabled={locked} />
      {/each}
    </div>
  </section>
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}

<section class="card">
  <h2>{S.checks.title}</h2>
  <div class="buttons"><button disabled={app.busy} onclick={runChecks}>{S.checks.run}</button></div>
  {#if checks}
    {#each checks as check (check.id)}
      <div class="check">
        <span class={levelClass[check.level]}>●</span>
        <strong>{S.checks.names[check.id]}</strong>
        <span class="muted">— {S.checks.level[check.level]}</span>
        {#if check.level !== "Ok"}
          <p class="hint">{S.checks.hints[check.id]}</p>
          {#if check.detail}<pre>{check.detail}</pre>{/if}
          <div class="buttons">
            {#if S.checks.links[check.id]}
              {@const url = S.checks.links[check.id] ?? ""}
              <button class="link" onclick={() => call(S.failed.open, commands.openUrl(url))}>{S.checks.issue}</button>
            {/if}
            {#if check.action}
              {@const action = check.action}
              <button disabled={locked} onclick={() => fix(check, action)}>{S.checks.actions[action]}</button>
            {/if}
          </div>
        {/if}
      </div>
    {/each}
  {/if}
</section>

<style>
  .check {
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
  }
</style>
```

В `frontend/src/App.svelte` добавить `import Zapret from "./tabs/Zapret.svelte";`, в `TABS` сразу после строки `tests`:

```ts
    { id: "zapret", label: S.tabs.zapret, view: Zapret },
```

и удалить строку-комментарий `// Tasks 18–20 add Strategies, Tests and Zapret here, in that order.`

- [ ] **Шаг 3: Проверка**

Run: `npm run check`, затем `npm test`
Expected: 0 ошибок и 0 предупреждений; 5 тестов проходят.

Вручную, из повышенного терминала, `npm run tauri dev`, сверяя с `service.bat`:
- игровой фильтр «только TCP» → `utils\game_filter.enabled` содержит `mode=tcp`, `sc qc zapret` показывает порты в аргументах; «выключен» → там `mode=disabled`, диапазоны сохранены;
- ipset: «любой IP» → `lists\ipset-all.txt` пуст, список лежит в `ipset-all.txt.backup`; «выключен» → в файле `203.0.113.113/32`; «загруженный список» возвращает список из `.backup`; после каждой смены служба перезапущена;
- «Обновить hosts» показывает строки с `+`/`−`, «Записать» добавляет блок между `# zapret-hub begin` и `# zapret-hub end`, повтор говорит «hosts уже актуален»;
- свой домен добавляется в `lists\list-general-user.txt`, `exa mple` даёт уведомление с ошибкой формата;
- «Запустить проверки» показывает 16 строк; у AdGuard (если стоит) — ссылка на обсуждение; «Очистить кэш Discord» закрывает Discord и перечисляет папки.

- [ ] **Шаг 4: Коммит**

```bash
git add frontend/src
git commit -m "feat(ui): add the zapret tab with Flowseal settings, checks and lists"
```

---

### Задача 21: Выпуск — README, CHANGELOG, архив, финальная проверка

**Files:**
- Create: `README.md`, `CHANGELOG.md`, `tools/package.ps1`

**Interfaces:**
- Consumes: всё приложение (задачи 1–20); версия из `[workspace.package]` корневого `Cargo.toml`
- Produces: `dist\zapret-hub-<версия>.zip` с `zapret-hub.exe` и `README.md` (спецификация 12); `/dist` уже в `.gitignore` (задача 1)

- [ ] **Шаг 1: README**

`README.md`:

````markdown
# Zapret Hub

Приложение в трее для [Flowseal zapret-discord-youtube](https://github.com/Flowseal/zapret-discord-youtube):
выбирает и переключает стратегии, тестирует их и ставит лучшую, делает всё, что умеет `service.bat`.

## Что умеет

- **Обзор:** служба, стратегия, быстрая проверка, версия Flowseal, прокси SOCKS5, лучшая стратегия последнего теста.
- **Стратегии:** все `general*.bat` в порядке меню `service.bat`, аргументы по блокам, применение в один клик.
  Свои стратегии — копия оригинальной с редактором блоков и проверкой `winws --dry-run`.
- **Тесты:** стандартный набор `test zapret.ps1` (HTTP, TLS 1.2, TLS 1.3, пинг) и набор DPI 16–20. Живой прогресс,
  рейтинг, история, копирование результатов в формате `test zapret.ps1`.
- **Zapret:** игровой фильтр, ipset, фейки, свои списки, обновление ipset и hosts, системные проверки `service.bat`,
  очистка кэша Discord, удаление службы.
- **Трей:** цвет иконки показывает состояние, меню переключает на стратегии из первой пятёрки рейтинга, быстрая
  проверка раз в 30 минут, уведомления Windows.

## Как выбирается лучшая стратегия

Выше та, у которой меньше неудачных проверок; при равенстве — та, у которой меньше медианная задержка HTTPS.
ICMP-пинг показан отдельным столбцом и на порядок не влияет. Стратегия, которая не запустилась, — в конце.

## Что нужно

- Windows 10 1803 или новее, либо Windows 11, x64. `curl.exe` входит в Windows начиная с 1803.
- Права администратора: приложение управляет службой zapret и WinDivert.
- WebView2 Runtime. В Windows 10 и 11 он обычно уже есть; если нет, приложение покажет ссылку на загрузку.
- Папка Flowseal zapret-discord-youtube (проверено на 1.10.3). Приложение её не скачивает.

## Установка и запуск

1. Распакуйте архив в отдельную папку, например `C:\Tools\ZapretHub`. Настройки и данные лежат рядом с exe.
2. Запустите `zapret-hub.exe` и подтвердите запрос UAC.
3. Мастер первого запуска найдёт папку zapret по установленной службе или запущенному `winws.exe` (или даст
   выбрать её вручную), предложит прокси SOCKS5, если тот сейчас отвечает, и спросит про автозапуск.

Крестик прячет окно в трей, выход — «Выход» в меню трея. Повторный запуск exe открывает уже работающую копию.

## Где данные

Всё рядом с exe:

- `zapret-hub.toml` — настройки;
- `data\strategies\` — свои стратегии;
- `data\tests\` — результаты тестов, `data\cache\` — набор DPI;
- `data\logs\ГГГГ-ММ-ДД.log` — журнал: каждая команда (`sc`, `net`, `curl.exe`, `winws.exe` и другие) с кодом выхода
  и полным выводом;
- `data\state\pending.json` — снимок на время изменяющей операции. Если приложение закрылось посреди неё, при
  следующем запуске оно вернёт службу, ipset и winws как было и скажет об этом;
- `data\webview\` — данные окна.

## Что приложение меняет в системе

- Службу `zapret` и имя стратегии в реестре — так же, как `service.bat`. `service.bat` и `test zapret.ps1`
  продолжают работать.
- Файлы в папке zapret: флаги и списки в форматах `service.bat`; свои стратегии — файлы
  `general (HUB <имя>).bat`, их видно и в меню `service.bat`.
- `hosts` — только по кнопке «Обновить hosts» и после подтверждения: блок между строками `# zapret-hub begin`
  и `# zapret-hub end`.
- Задачу Планировщика «Zapret Hub», если включён автозапуск.
- Ключ `HKCU\Software\Classes\AppUserModelId\ZapretHub` — имя приложения в уведомлениях.

## Тесты стратегий

На время прогона служба zapret останавливается, а winws по очереди запускается с каждой выбранной стратегией;
после прогона служба, ipset и winws возвращаются как были. Стандартный набор — около 15 секунд на стратегию,
все стратегии Flowseal — 8–10 минут. Прогон можно отменить; закрытие окна его не прерывает.

## Антивирусы

Приложение запускает `winws.exe`, управляет службами и может править `hosts`, поэтому антивирус может счесть
его подозрительным. Своего WinDivert в приложении нет: драйвер берётся из папки zapret. Если exe попал в
карантин, добавьте папку приложения в исключения.

## Если что-то не работает

Текст любой ошибки копируется кнопкой «Копировать» в уведомлении; подробности — в журнале `data\logs`.
Системные проверки на вкладке Zapret повторяют диагностику `service.bat`.

## Удаление

1. Выключите автозапуск в настройках (или `schtasks /Delete /TN "Zapret Hub" /F`).
2. Выйдите из приложения через трей и удалите его папку.
3. По желанию удалите ключ уведомлений: `reg delete HKCU\Software\Classes\AppUserModelId\ZapretHub /f`.

Служба zapret и папка Flowseal остаются. Службу удаляет кнопка на вкладке Zapret или `service.bat`.

## Сборка из исходников

Нужны Rust (версию задаёт `rust-toolchain.toml`), Node.js 24 LTS и Microsoft C++ Build Tools.

```
npm install
npm run tauri build
```

Готовый exe — `target\release\zapret-hub.exe`. `tools\package.ps1` собирает его и кладёт в
`dist\zapret-hub-<версия>.zip` вместе с README. `npm run tauri dev` запускается из терминала с правами
администратора; сборка, тесты и `npm run check` — из обычного.
````

- [ ] **Шаг 2: CHANGELOG**

`CHANGELOG.md`:

```markdown
# Изменения

Версии по [semver](https://semver.org/lang/ru/).

## 0.1.0

Первая версия.

- Управление Flowseal zapret-discord-youtube вместо `service.bat`: служба, стратегии, игровой фильтр, ipset,
  фейки, свои списки, обновление ipset и hosts, системные проверки, очистка кэша Discord.
- Свои стратегии: копия оригинальной, редактор блоков, проверка `winws --dry-run`.
- Тесты стратегий: стандартный набор `test zapret.ps1` и DPI 16–20, рейтинг, история, копирование результатов.
- Трей с быстрой проверкой раз в 30 минут, уведомления Windows, автозапуск через Планировщик.
- Снимок состояния перед каждой изменяющей операцией: откат при ошибке и восстановление после аварийного
  завершения.
```

- [ ] **Шаг 3: Скрипт упаковки**

`tools/package.ps1`:

```powershell
# Builds the release exe and packs it with the README into dist\zapret-hub-<version>.zip.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Push-Location $root
try {
    # The first `version = "..."` line of the root Cargo.toml is [workspace.package]'s.
    $version = (Select-String -Path Cargo.toml -Pattern '^version = "(.+)"$' | Select-Object -First 1).Matches[0].Groups[1].Value
    npm run tauri build
    if ($LASTEXITCODE -ne 0) { throw "npm run tauri build exited with $LASTEXITCODE" }

    $stage = Join-Path $root "dist\zapret-hub-$version"
    $zip = "$stage.zip"
    Remove-Item -Recurse -Force $stage, $zip -ErrorAction SilentlyContinue
    New-Item -ItemType Directory -Force $stage | Out-Null
    Copy-Item target\release\zapret-hub.exe, README.md -Destination $stage
    Compress-Archive -Path "$stage\*" -DestinationPath $zip
    Write-Output "packed $zip"
} finally {
    Pop-Location
}
```

- [ ] **Шаг 4: Автоматические проверки целиком**

В обычном терминале, на ненагруженной машине, по очереди:

Run: `cargo fmt --all -- --check`
Expected: код 0, вывода нет.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

Run: `cargo test --workspace`
Expected: все тесты проходят; тесты, которым нужны права администратора, служба или `E:\Zapret`, перечислены как `ignored`.

Run: `npm run check`
Expected: `svelte-check found 0 errors and 0 warnings`.

Run: `npm test`
Expected: 5 тестов проходят.

- [ ] **Шаг 5: Архив**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File tools/package.ps1`
Expected: последняя строка `packed …\dist\zapret-hub-0.1.0.zip`; в архиве `zapret-hub.exe` и `README.md`.

Run (PowerShell): `Select-String -Path target\release\zapret-hub.exe -Pattern "requireAdministrator" -SimpleMatch -Quiet`
Expected: `True`.

- [ ] **Шаг 6: Тесты с правами администратора**

Из повышенного терминала (zapret несколько раз остановится и вернётся; запомнить вывод `sc qc zapret` до запуска):

Run: `cargo test --workspace -- --ignored --nocapture --test-threads=1`
Expected: все проходят; после прогона `sc qc zapret` показывает ту же стратегию, что до него, служба работает.

- [ ] **Шаг 7: Ручной чек-лист (спецификация 11)**

Распаковать архив из шага 5 в пустую папку (например, `C:\Temp\zh-check`) и запустить exe оттуда. Каждый пункт сверяется с `service.bat` и журналом `data\logs`:

1. Запрос UAC; мастер первого запуска; рядом с exe появились `zapret-hub.toml` и `data\logs\`.
2. Удаление службы на вкладке Zapret → `sc query zapret` отвечает «1060»; применение стратегии на вкладке «Стратегии» → служба установлена и работает, «Check Status» в `service.bat` показывает ту же стратегию.
3. Смена стратегии из трея («Стратегия: … → Переключить на»): иконка и обзор показывают новую стратегию и результат быстрой проверки.
4. Полный прогон стандартного набора по всем стратегиям: прогресс, уведомление в конце, таблица в порядке рейтинга, «Применить лучшую» ставит первую строку.
5. Отмена посреди прогона: прогон сохранён с пометкой «отменён», служба работает с прежней стратегией.
6. Посреди прогона из повышенного терминала `taskkill /F /IM zapret-hub.exe`, затем снова запустить exe: уведомление о прерванной операции, служба, ipset и ручные winws как до прогона, `data\state\pending.json` нет.
7. Прогон набора DPI: во время прогона `lists\ipset-all.txt` пуст; после — `Get-FileHash lists\ipset-all.txt` совпадает с хешем до прогона.
8. Автозапуск: включить в настройках, выйти, `schtasks /Run /TN "Zapret Hub"` → приложение в трее без окна; выключить → `schtasks /Query /TN "Zapret Hub"` задачи не находит.
9. Крестик прячет окно в трей, повторный запуск exe поднимает окно, «Выход» завершает процесс.

Expected: все пункты выполняются как описано. Расхождение — ошибка: исправить в задаче, к которой относится код, и пройти пункт заново.

- [ ] **Шаг 8: Коммит**

```bash
git add README.md CHANGELOG.md tools/package.ps1
git commit -m "build(app): add the release zip, the readme and the changelog"
```
