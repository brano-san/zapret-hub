# Zapret Hub — zapret2 как второй движок: план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Цель:** стратегии bol-van/zapret2 (`winws2.exe`, стратегии на Lua) рядом со стратегиями Flowseal: тот же тестер и рейтинг, служба `zapret2` в один клик, переключение движков без ручной работы, свои стратегии в текстовом редакторе — из окна и через `zapret-hub-cli`.

**Архитектура:** новый пакет `crates/zapret2` (зависит от `zapret` и `exec`, без `tauri`): папка движка, версия и совместимость Lua, встроенные и свои стратегии, подстановки, статическая проверка, проверка запуском winws2, служба `zapret2`. Общего трейта движка нет (спецификация 3.1). Стратегия zapret2 везде называется `zapret2:<имя>`, `zapret::service::active()` даёт пару (служба, процесс) установленного движка, а по префиксу ветвятся три места: `tester::run::launch`, `tester::catalog::detail`, `actions::apply`. Крейт `zapret` знает второй движок только по именам `zapret2` и `winws2.exe`: исключительность служб, один снимок на оба движка, быстрая и системные проверки. Приложение получает настройку `zapret2_dir`, четыре команды окна (`zapret2_info`, `validate_zapret2`, `save_zapret2`, `delete_zapret2`) и поля обзора; CLI — подкоманды `zapret2 info|validate|save|delete`; папку zapret2, как и папку Flowseal, меняет только окно (спецификация 10); окно — карточку папки, карточку на обзоре, текстовый редактор и метки.

**Стек:** новых внешних зависимостей нет. Rust 1.99.0 (edition 2024: `if … && let` в `save_settings`); `windows-registry` 0.6.1 — имя стратегии на ключе любой службы; specta 2.0.0-rc.25 — поле с `#[serde(default)]` становится `?:` в TypeScript; clap 4.6.7; tauri 2.12 (2.12.3 в `Cargo.lock` ветки `feat/cli`); Svelte 5.57.1, svelte-check 4.7.6, vitest 5.0.3.

**Спецификация:** `docs/superpowers/specs/2026-10-11-zapret-hub-zapret2-design.md` (далее «спецификация»); её основа — `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md` («core») и `docs/superpowers/specs/2026-10-11-zapret-hub-cli-design.md` («cli»). Исполнитель читает спецификацию вместе с планом.

**База:** `4183ad1` — вершина ветки `feat/cli` (подпроект CLI: выпуск 0.2.0 и исправления финального ревью, после которых канал CLI не меняет `zapret_dir`); ветка `feat/zapret2` начинается от неё. Якоря плана сверены с `4183ad1` предварительной проверкой; из двух вариантов задачи 9 действует второй, второй вариант `setting` задачи 8 не нужен.

## Общие ограничения

- Только Windows 10/11 x64. Исполнитель работает в обычном терминале. Ни один шаг не требует прав администратора, не запускает приложение и не меняет систему: никаких `sc`, `net`, `taskkill`, `netsh`, `reg`, `schtasks`.
- Движка zapret2 у исполнителя нет, и он его не скачивает. Всё, что запускает `winws2.exe` (`--version`, `--dry-run`, `--intercept=0`, служба, тестовый прогон), — тесты `#[ignore]` с переменной `ZAPRET2_DIR` или ручная проверка пользователя после слияния. Тесты без прав подсовывают пустой `winws2.exe`: его запуск даёт os error 193, и по этому видно, дошло ли дело до запуска.
- Имя стратегии zapret2 — `zapret2:<имя>`; без префикса (`mine`) — только аргумент `save_zapret2`/`delete_zapret2` и CLI `zapret2 save|delete`. Свои стратегии — `data\zapret2\<имя>.txt`, встроенные (`general`, `general-sni`) зашиты через `include_str!`.
- Командная строка zapret2 = `--chdir="<корень>" ` + текст без строк-комментариев (`#` в начале строки) с подстановками `%LUA%`, `%FILES%`, `%LISTS%`, `%BIN%`, `%GameFilter%`, `%GameFilterTCP%`, `%GameFilterUDP%`. Тест, проверка и служба берут одну и ту же строку; её SHA-256 — ключ свежести результата.
- Проверка — по порядку до первой ошибки: статическая; `<строка> --wf-dup-check=0 --dry-run`; `<строка> --intercept=0`. Аргументы передаются через `raw_arg` как есть.
- Служба `zapret2`, автозапуск, `ImagePath` = `"<winws2.exe>" <строка>`; имя стратегии — значение `zapret-discord-youtube` на её ключе. Установлена одна служба из двух: установка любой удаляет другую.
- Вывод `winws2` и тексты ошибок не усекаются.
- Код, комментарии, тексты ошибок и коммиты — на английском; `docs/`, README, интерфейс — на русском. Коммит — одна строка Conventional Commits (scope `zapret`, `zapret2`, `tester`, `app`, `cli`, `ui`, `build`), без тела и без трейлеров.
- Тесты приложения — `cargo test -p zapret-hub --lib`: у бинарника `zapret-hub` манифест `requireAdministrator` и `test = false`.
- Rust-код в плане уже прошёл rustfmt. Перед каждым коммитом с Rust-кодом всё равно `cargo fmt --all` и `cargo clippy --workspace --all-targets -- -D warnings` (без замечаний).
- После изменения DTO или команд — `cargo run -p zapret-hub --bin export-bindings` и `frontend/src/lib/ipc/bindings.ts` в том же коммите.
- В коммит идут только файлы задачи: `git add` с явными путями, без `git add -A` и `git add .`.
- Числа тестов в «Expected» — для дерева `4183ad1` с кодом плана: приложение 16 до задачи 7 и 18 с неё, cli 8/17/11 до задачи 8 и 8/18/11 с неё. Если число расходится, сверяется прирост.
- Плейсхолдеров в плане нет: файлы, которые план велит создать или заменить, даны целиком, правки — парами «было → стало» или «после строки … добавить».

## На что смотреть при ревью

1. **Пути с пробелами** (`C:\Program Files\…`, папка Flowseal с пробелом). Каждая подстановка остаётся одним аргументом в кавычках, `--chdir` не съедает кавычку, а `\"` перед кавычкой отклоняется до запуска winws2. Тесты: `the_command_line_fills_every_placeholder`, `a_backslash_before_a_quote_is_refused` (задача 4), `a_trailing_backslash_is_not_part_of_the_root` (задача 3).
2. **Данные прошлой версии.** `pending.json` 0.2.0 без полей второго движка, прогоны без `zapret2_version`, настройки без `zapret2_dir` читаются, и zapret2 выключен. Тесты: `a_pending_json_from_before_zapret2_still_loads` (задача 2), `a_run_saved_before_zapret2_still_loads` (задача 6), `zapret2_is_off_until_its_folder_is_set` (задача 7).
3. **Пустая, битая, кириллическая или несовместимая папка zapret2 не мешает Flowseal.** Строки zapret2 пропадают, Flowseal работает как раньше, причина видна в «Настройках» и на обзоре, прогон со стратегиями zapret2 отказывает до снимка. Тесты: `every_missing_file_is_named`, `a_cyrillic_path_is_refused`, `info_is_empty_without_a_folder_and_names_what_is_wrong` (задача 3), `zapret2_rows_follow_flowseals_with_their_checks_and_results`, `zapret2_strategies_need_a_working_engine_before_the_run_starts` (задача 6).
4. **Стратегия, которая молча ничего не делает.** winws2, вышедший после строки готовности (Lua не стартовал, код 87) или напечатавший `LUA ERROR`, — ошибка запуска с полным выводом, а не стратегия с нулевым счётом выше рабочих. Тесты: `winws2_that_died_or_printed_a_lua_error_did_not_run_the_strategy`, `a_mixed_run_ranks_both_engines_by_the_same_rules` (задача 6), `an_engine_that_exits_is_a_launch_error_naming_its_image` (задача 1).
5. **Файлы из Блокнота и PowerShell 5.1.** BOM снимается, CRLF допустим, `#var` внутри строки остаётся. Тесты: BOM в `built_in_ones_come_first_then_the_users_in_natural_order`, CRLF и `#var` в `comment_lines_go_and_zapret2_hashes_stay` (задача 4), `zapret2_save_and_validate_send_the_text_of_a_file` (задача 8).

---

## Структура файлов

```
Cargo.toml                          + zapret2 в [workspace.dependencies]; версия 0.3.0
crates/zapret/
  src/service.rs                    SERVICE2, WINWS2, active(), strategy_of, set_strategy_of, create(name, …);
                                    установка Flowseal удаляет zapret2, remove() — обе службы
  src/procs.rs                      Winws::start(exe, cwd, tail) для любого движка
  src/checks.rs                     conflicts(): zapret2 рядом с zapret; «WinDivert без winws» видит winws2
  src/snapshot.rs                   снимок и откат обоих движков
  src/batparse.rs, src/custom.rs    replace_ci и quoted_tokens открыты для zapret2
crates/zapret2/                     новый пакет
  src/lib.rs                        Engine (папка, exe, lua, версия с кешем), parse_banner, required_compat, Info, info()
  src/strategies.rs                 встроенные и свои, claim/write/delete, args, tail, check, split
  src/service.rs                    validate (static → --dry-run → --intercept=0), install, apply, reinstall_active, save
  src/builtin/general.txt           встроенная zapret2:general
  src/builtin/general-sni.txt       встроенная zapret2:general-sni
  tests/engine.rs, tests/strategies.rs, tests/service.rs, tests/common/mod.rs
  tests/fixtures/functions.txt      функции Lua zapret2 на a1adf7b
  tests/fixtures/flowseal-1.10.3.txt  файлы lists\ и bin\ Flowseal 1.10.3
crates/tester/
  src/model.rs                      TestRun.zapret2_version
  src/run.rs                        launch() по префиксу, LUA_SETTLE, lua_failure(), обе службы остановлены
  src/catalog.rs                    строки и карточка zapret2, StrategyDetail.source
crates/cli/
  src/lib.rs                        Request::{Zapret2Info, ValidateZapret2, SaveZapret2, DeleteZapret2}
  src/main.rs                       zapret2 info|validate|save|delete, read_text()
src-tauri/
  Cargo.toml                        + zapret2
  src/settings.rs                   Settings.zapret2_dir
  src/state.rs                      engine2(), engine2_required()
  src/actions.rs                    apply по префиксу, restart активной, save_settings, overview, save_zapret2
  src/commands.rs                   zapret2_info, validate_zapret2, save_zapret2, delete_zapret2; game filter, ipset, fake
  src/dto.rs                        Overview.service_name, Overview.zapret2
  src/jobs.rs, src/tray.rs, src/texts.rs, src/lib.rs, src/cli.rs
frontend/src/
  lib/ipc/bindings.ts               (генерируется)
  lib/format.ts, lib/format.test.ts ZAPRET2, zapret2Name()
  lib/strings.ts                    строки zapret2
  tabs/Zapret2Editor.svelte         новый: текстовый редактор стратегии zapret2
  tabs/Strategies.svelte            метки, кнопки своих zapret2, копия в редактор
  tabs/Overview.svelte, tabs/Settings.svelte, tabs/Tests.svelte
docs/cli.md                         команды zapret2, папка zapret2 — только в окне, пункт 12 живой проверки
docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md   поправка к разделу 13
docs/superpowers/specs/2026-10-11-zapret-hub-cli-design.md    поправка к разделу 4
README.md, CHANGELOG.md             раздел «zapret2», запись 0.3.0
package.json, frontend/package.json, package-lock.json   версия 0.3.0
```

Зависимости: `zapret2 → zapret, exec`; `tester → zapret2`; `src-tauri → zapret2`. `cli` новых зависимостей не получает.

## Что проверено сборкой

Копия дерева `8d0173f` (тот же tree `7d8e05d`) с кодом всех пяти задач плана CLI, на ней — десять задач этого плана, каждая отдельным коммитом; своя папка `target`, обычный терминал.
- На каждой задаче: `cargo fmt --all -- --check` без изменений, `cargo clippy --workspace --all-targets -- -D warnings` без замечаний, `cargo test --workspace --exclude zapret-hub` и `cargo test -p zapret-hub --lib` проходят. Тестов (прошло / `ignored`): задача 1 — 138 / 11, 2 — 139 / 11, 3 — 147 / 12, 4 — 158 / 12, 5 — 161 / 15, 6 — 166 / 16, 7 — 166 / 16, 8 — 167 / 16, 9 — 167 / 16, 10 — 167 / 16; тестов приложения 14 до задачи 6 и 15 с задачи 7.
- Шаг «Убедиться, что тесты падают» каждой задачи 1–9 проверен отдельно: тесты задачи поверх дерева предыдущей дают ровно те ошибки компиляции (или `TypeError` в задаче 9), что записаны в плане.
- `cargo run -p zapret-hub --bin export-bindings` на задачах 6, 7, 8 и 10 даёт ровно `bindings.ts` плана. `svelte-check` — 0 ошибок и 0 предупреждений на каждой задаче; vitest — 10 тестов до задачи 9, 11 с неё; `vite build` на задаче 10 проходит.
- Каждый блок кода задач (168) и каждая правка или якорь в тексте (139; четыре якоря сокращены многоточием и сверены вручную) сверены скриптом с файлами копии до и после своей задачи, второй вариант задач 8 и 9 — с веткой `feat/cli`.
- Историческая проверка: ветка `feat/cli` на `1258411` (задачи 1–4 CLI и исправления ревью): код задач 1–9 наложен через `git apply -3`. Два конфликта — `setting` в `crates/cli/src/main.rs` и перечитывание настроек в `Settings.svelte` — разрешены, как сказано во втором варианте задач 8 и 9; после этого fmt и clippy чисты, 167 тестов и 16 `ignored`, 16 тестов приложения (на ветке их на один больше, чем в плане CLI), `bindings.ts` меняется так же, как на копии, `svelte-check` 0/0, vitest 14, `vite build` проходит. Остальные правки в файлах CLI легли без ручной работы. `docs/cli.md` на ветке ещё нет (задача 5 CLI): его якоря сверены только с планом CLI.
- `Cargo.lock` копии собран без сети, и восемь сторонних пакетов в нём на патч-версию ниже, чем на `feat/cli` (tauri 2.12.1 вместо 2.12.3). Итоговое дерево проверено и с `Cargo.lock` ветки `feat/cli`: clippy чист, все тесты проходят.
- Внешние API сверены с версиями из `Cargo.lock`: std `CommandExt::raw_arg`, `ExitStatusExt::from_raw` (в тесте); `windows-registry` 0.6.1 `LOCAL_MACHINE.open/create`, `get_string`, `set_string`; specta 2.0.0-rc.25 (`#[serde(default)]` → `?:`, встроенный `QuickCheck` в ответе `saveZapret2`); clap 4.6.7 (`ValueEnum` + `rename_all`, `Error::render` в тесте); tauri 2.12 (команды и `async_runtime::block_on`, как в коде CLI); Svelte 5.57.1 (`$state`, `$props`, `$derived`, `{@const}`).
- По исходникам zapret2 на `a1adf7b` (клон только читался, ничего из него не запускалось): формат баннера `--version` и `lua_compat_ver`, `NFQWS2_COMPAT_VER_REQUIRED=6`, строка `command line parameters verified` и код выхода `--dry-run`, код 87 у `--intercept=0` при ошибке Lua, строка готовности `windivert initialized. capture is started.`, список функций в `functions.txt`, имена блобов, протоколов `--filter-l7` и типов `--payload`, аргументы `fake`, `multidisorder`, `fakedsplit`.
- Не запускалось: ничего, что запускает `winws2.exe` (его нет), тесты `ignored`, приложение и CLI вживую, релизная сборка `tools/package.ps1`.

---

### Задача 1: `zapret` знает zapret2 по имени

**Files:**
- Modify: `crates/zapret/src/service.rs`, `crates/zapret/src/procs.rs`, `crates/zapret/src/checks.rs`
- Modify (вызовы): `crates/zapret/src/custom.rs`, `crates/zapret/src/snapshot.rs` (задача 2 перепишет эти места), `crates/tester/src/run.rs`
- Test: `crates/zapret/tests/service.rs`, `crates/zapret/tests/checks.rs`

**Interfaces:**
- Consumes: `zapret::service::{state, status, config, stop_and_delete, start, SvcState}`, `zapret::procs::{kill_all, is_running}` — без изменений.
- Produces (крейт `zapret`):
  - `service::SERVICE2: &str = "zapret2"`, `service::WINWS2: &str = "winws2.exe"`;
  - `service::active() -> (&'static str, &'static str)` — `("zapret2", "winws2.exe")`, пока служба `zapret2` установлена, иначе `("zapret", "winws.exe")`;
  - `service::strategy_of(name: &str) -> Option<String>`; `service::strategy()` теперь `strategy_of(active().0)`;
  - `service::set_strategy_of(service: &str, name: &str) -> anyhow::Result<()>` вместо `set_strategy(name)`;
  - `service::create(name: &str, bin_path: &str, auto_start: bool) -> anyhow::Result<()>` — новый первый аргумент;
  - `service::install` (Flowseal) удаляет службу `zapret2` и завершает `winws2.exe`; `service::remove` удаляет обе службы и оба процесса; `service::quick_check` смотрит на `active()`;
  - `procs::Winws::start(exe: &Path, cwd: &Path, tail: &str) -> std::io::Result<Winws>` вместо `start(folder, tail)`;
  - `checks::conflicts(installed: impl Fn(&str) -> bool) -> Vec<&'static str>` — установленные службы для «Удалить службы»: `CONFLICTING` и `zapret2`, если рядом установлена `zapret`.

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret/tests/service.rs`, начало файла (импорты) заменить на:

```rust
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

use zapret::procs::Winws;
use zapret::service::{SvcState, config, parse_timestamps, state};
```

там же после теста `reads_a_service_config_from_the_registry` добавить:

```rust
#[test]
fn a_service_without_a_recorded_strategy_has_none() {
    assert_eq!(
        zapret::service::strategy_of("zapret-hub-no-such-service"),
        None
    );
}

fn cmd_exe() -> PathBuf {
    PathBuf::from(std::env::var_os("ComSpec").expect("ComSpec is set"))
}

#[test]
fn a_started_engine_is_ready_when_capture_starts() {
    // winws2 prints `windivert initialized. capture is started.`; cmd.exe stands in for it.
    let dir = tempfile::tempdir().unwrap();
    let mut winws = Winws::start(
        &cmd_exe(),
        dir.path(),
        r#"/c "cd&echo windivert initialized. capture is started.&ping -n 3 127.0.0.1>nul""#,
    )
    .unwrap();
    winws.wait_ready(Duration::from_secs(5)).unwrap();
    assert!(winws.exited().is_none());
    let output = winws.stop();
    assert!(output.contains("capture is started."), "{output}");
    let dir_name = dir.path().file_name().unwrap().to_string_lossy();
    assert!(
        output.contains(&*dir_name),
        "runs in the given folder: {output}"
    );
}

#[test]
fn an_engine_that_exits_is_a_launch_error_naming_its_image() {
    let dir = tempfile::tempdir().unwrap();
    let mut winws = Winws::start(
        &cmd_exe(),
        dir.path(),
        r#"/c "echo LUA ERROR: no such function&exit 87""#,
    )
    .unwrap();
    let err = winws.wait_ready(Duration::from_secs(5)).unwrap_err();
    assert!(err.to_lowercase().starts_with("cmd.exe exited"), "{err}");
    assert!(err.contains("87") && err.contains("LUA ERROR"), "{err}");
}

#[test]
#[ignore = "needs admin rights and E:\\Zapret; replaces the zapret and zapret2 services"]
fn a_flowseal_install_takes_zapret2_down() {
    use zapret::service::{SERVICE, SERVICE2, active, create};
    // A stand-in zapret2: the install removes it whatever it runs.
    create(SERVICE2, r#""C:\Windows\System32\cmd.exe" /c exit"#, false).unwrap();
    assert_eq!(active().0, SERVICE2);
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let check = zapret::service::apply(&folder, "general").unwrap();
    eprintln!("{check:?}");
    assert_eq!(state(SERVICE2), SvcState::NotInstalled);
    assert_eq!(active(), (SERVICE, "winws.exe"));
}
```

`cmd.exe` здесь — безопасная замена `winws2.exe`: `cd` печатает рабочую папку, `ping` держит процесс живым 2 с, `exit 87` повторяет выход winws2 при ошибке Lua.

`crates/zapret/tests/checks.rs`, первую строку заменить на:

```rust
use zapret::checks::{
    conflicts, has_cyrillic, hosts_mentions_youtube, in_onedrive, matching_services,
};
```

и после теста `a_service_matches_when_one_line_has_every_word` добавить:

```rust
#[test]
fn zapret2_is_a_conflict_only_next_to_zapret() {
    let installed = |names: &'static [&'static str]| move |name: &str| names.contains(&name);
    assert!(conflicts(installed(&["zapret2"])).is_empty());
    assert!(conflicts(installed(&["zapret"])).is_empty());
    assert_eq!(conflicts(installed(&["zapret", "zapret2"])), ["zapret2"]);
    assert_eq!(
        conflicts(installed(&["zapret2", "winws2", "zapret", "GoodbyeDPI"])),
        ["GoodbyeDPI", "winws2", "zapret2"]
    );
}
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test service --test checks`
Expected: FAIL при компиляции: у `Winws::start` два аргумента, а не три (E0061); в `zapret::service` нет `strategy_of`, `SERVICE2`, `active`; в `zapret::checks` нет `conflicts` (E0425, E0432).

- [ ] **Шаг 3: Имена второго движка в `service.rs`**

`crates/zapret/src/service.rs`, строку `const SERVICE_KEY: &str = r"SYSTEM\CurrentControlSet\Services\zapret";` заменить на:

```rust
/// zapret2's service and process (zapret2 spec 3.1, 6): this crate knows the second engine by
/// these two names only.
pub const SERVICE2: &str = "zapret2";
pub const WINWS2: &str = "winws2.exe";
```

после функции `state` добавить:

```rust
/// The installed engine's service and process (zapret2 spec 3.1): `zapret2` and `winws2.exe` while
/// the `zapret2` service exists, else Flowseal's. The app installs one engine at a time; when
/// `service.bat` puts `zapret` next to `zapret2`, the system checks name the conflict (spec 9).
pub fn active() -> (&'static str, &'static str) {
    if state(SERVICE2) == SvcState::NotInstalled {
        (SERVICE, "winws.exe")
    } else {
        (SERVICE2, WINWS2)
    }
}

fn key(name: &str) -> String {
    format!(r"SYSTEM\CurrentControlSet\Services\{name}")
}
```

в функции `config` заменить

```rust
    let key = windows_registry::LOCAL_MACHINE
        .open(format!(r"SYSTEM\CurrentControlSet\Services\{name}"))
        .ok()?;
```

на

```rust
    let key = windows_registry::LOCAL_MACHINE.open(key(name)).ok()?;
```

функции `strategy` и `set_strategy` вместе с doc-комментарием заменить на:

```rust
/// The strategy name recorded next to service `name`: `service.bat` writes it for `zapret`, the app
/// writes `zapret2:<name>` for `zapret2` (zapret2 spec 6).
pub fn strategy_of(name: &str) -> Option<String> {
    windows_registry::LOCAL_MACHINE
        .open(key(name))
        .ok()?
        .get_string(STRATEGY_VALUE)
        .ok()
}

/// The active engine's strategy: a Flowseal name or `zapret2:<name>`.
pub fn strategy() -> Option<String> {
    strategy_of(active().0)
}

pub fn set_strategy_of(service: &str, name: &str) -> anyhow::Result<()> {
    let key = key(service);
    windows_registry::LOCAL_MACHINE
        .create(&key)?
        .set_string(STRATEGY_VALUE, name)?;
    log::info!(target: "exec", r"registry: HKLM\{key} {STRATEGY_VALUE} = {name}");
    Ok(())
}
```

функцию `create` вместе с doc-комментарием заменить на:

```rust
/// `sc create <name> binPath= ... DisplayName= <name> start= auto|demand` and the description, as
/// `service.bat` makes `zapret`; `zapret2` the same way (zapret2 spec 6).
pub fn create(name: &str, bin_path: &str, auto_start: bool) -> anyhow::Result<()> {
    let start = if auto_start { "auto" } else { "demand" };
    cmd(
        "sc",
        &[
            "create",
            name,
            "binPath=",
            bin_path,
            "DisplayName=",
            name,
            "start=",
            start,
        ],
    )?
    .check()?;
    let description = if name == SERVICE {
        "Zapret DPI bypass software"
    } else {
        "zapret2 DPI bypass software"
    };
    let _ = cmd("sc", &["description", name, description]);
    Ok(())
}
```

функцию `install` вместе с doc-комментарием заменить на:

```rust
/// `:service_install` after the menu: timestamps on, the old service gone, the new one created
/// and started, the strategy name recorded. One engine at a time (zapret2 spec 6): zapret2 goes
/// too, so every path that installs a Flowseal strategy takes it down.
pub fn install(folder: &Folder, name: &str, parsed: &ParsedArgs) -> anyhow::Result<()> {
    let _ = enable_tcp_timestamps();
    stop_and_delete(SERVICE)?;
    stop_and_delete(SERVICE2)?;
    crate::procs::kill_all(WINWS2)?;
    create(SERVICE, &parsed.bin_path(&folder.winws()), true)?;
    cmd("sc", &["start", SERVICE])?.check()?;
    set_strategy_of(SERVICE, name)
}
```

в функции `remove` заменить doc-комментарий и начало

```rust
/// `:service_remove`: zapret, every winws, WinDivert, WinDivert14. Only the `zapret` service
/// has to go. As in `service.bat` the rest is best effort: winws may end by itself, and a
/// WinDivert driver that another program holds stays; their errors go to the journal as warnings.
pub fn remove() -> anyhow::Result<()> {
    stop_and_delete(SERVICE)?;
    if let Err(err) = crate::procs::kill_all("winws.exe") {
        log::warn!(target: "exec", "winws.exe was not ended: {err:#}");
    }
```

на

```rust
/// `:service_remove`: zapret and zapret2, every winws and winws2, WinDivert, WinDivert14. Only the
/// services have to go. As in `service.bat` the rest is best effort: a process may end by itself,
/// and a WinDivert driver that another program holds stays; their errors go to the journal as
/// warnings.
pub fn remove() -> anyhow::Result<()> {
    stop_and_delete(SERVICE)?;
    stop_and_delete(SERVICE2)?;
    for image in ["winws.exe", WINWS2] {
        if let Err(err) = crate::procs::kill_all(image) {
            log::warn!(target: "exec", "{image} was not ended: {err:#}");
        }
    }
```

в функции `reinstall_active` строку `let Some(name) = strategy() else {` заменить на `let Some(name) = strategy_of(SERVICE) else {`: эта функция переустанавливает только Flowseal, и имя `zapret2:…` среди `.bat` она бы не нашла.

в функции `quick_check` doc-комментарий и первые две строки тела

```rust
/// 5.4: winws runs and the first two URL targets answer `HTTP:OK`.
pub fn quick_check(folder: &Folder) -> QuickCheck {
    let service = state(SERVICE);
    let winws_running = crate::procs::is_running("winws.exe");
```

заменить на

```rust
/// 5.4: the active engine's process runs and the first two URL targets answer `HTTP:OK`.
pub fn quick_check(folder: &Folder) -> QuickCheck {
    let (service, image) = active();
    let service = state(service);
    let winws_running = crate::procs::is_running(image);
```

- [ ] **Шаг 4: `Winws` для любого движка**

`crates/zapret/src/procs.rs`: удалить строку `use crate::folder::Folder;` и пустую строку после неё. Структуру `Winws` с doc-комментарием и начало `impl Winws` до `.current_dir(…)` заменить:

```rust
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
```

на

```rust
/// A `winws.exe` or `winws2.exe` the tests start directly, with the arguments the service would
/// get (7.3).
pub struct Winws {
    child: Child,
    output: Arc<Mutex<Vec<u8>>>,
    readers: Vec<JoinHandle<()>>,
    /// `winws.exe` or `winws2.exe`: the messages name it.
    image: String,
}

impl Winws {
    /// `"<exe>" <tail>` in `cwd`, no window: Flowseal's `bin\winws.exe` from `bin`, zapret2's
    /// `winws2.exe` from its root.
    pub fn start(exe: &Path, cwd: &Path, tail: &str) -> std::io::Result<Self> {
        let mut child = Command::new(exe)
            .raw_arg(tail)
            .current_dir(cwd)
```

в конце `start` заменить

```rust
        log::info!(target: "exec", "started winws (pid {}){tail}", child.id());
        Ok(Self {
            child,
            output,
            readers,
        })
```

на

```rust
        let image = exe.file_name().map_or_else(
            || exe.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        log::info!(target: "exec", "started {image} (pid {}) {}", child.id(), tail.trim_start());
        Ok(Self {
            child,
            output,
            readers,
            image,
        })
```

doc-комментарий `wait_ready`

```rust
    /// Waits until winws reports capture started, exits, or `max` passes. A winws that exited
    /// is a launch error: `Err` carries its exit status and output.
```

заменить на

```rust
    /// Waits until winws reports capture started (winws2: `windivert initialized. capture is
    /// started.`), exits, or `max` passes. One that exited is a launch error: `Err` carries its exit
    /// status and output.
```

в нём же строку `return Err(format!("winws exited ({status}):\n{}", self.output_text()));` заменить на:

```rust
                return Err(format!(
                    "{} exited ({status}):\n{}",
                    self.image,
                    self.output_text()
                ));
```

а в `stop` строку `log::info!(target: "exec", "stopped winws (pid {})\n{text}", self.child.id());` — на:

```rust
        log::info!(target: "exec", "stopped {} (pid {})\n{text}", self.image, self.child.id());
```

Проверка готовности (`contains("capture is started")`) не меняется: строка winws2 её содержит.

- [ ] **Шаг 5: Системные проверки**

`crates/zapret/src/checks.rs`, строку `use crate::service::{self, SvcState};` заменить на:

```rust
use crate::service::{self, SERVICE, SERVICE2, SvcState, WINWS2};
```

после `pub const CONFLICTING …;` добавить:

```rust
/// The installed services «Удалить службы» removes: the other bypasses, and `zapret2` while `zapret`
/// is installed too (zapret2 spec 9). `service.bat` knows nothing of zapret2 and may have put
/// `zapret` next to it: the user's newer step outside the app wins.
pub fn conflicts(installed: impl Fn(&str) -> bool) -> Vec<&'static str> {
    let zapret2 = installed(SERVICE).then_some(SERVICE2);
    CONFLICTING
        .into_iter()
        .chain(zapret2)
        .filter(|name| installed(name))
        .collect()
}

fn installed(name: &str) -> bool {
    service::state(name) != SvcState::NotInstalled
}
```

в `run_all` заменить

```rust
    let orphan = matches!(windivert, SvcState::Running | SvcState::StopPending)
        && !crate::procs::is_running("winws.exe");
    let conflicts: Vec<&str> = CONFLICTING
        .into_iter()
        .filter(|name| service::state(name) != SvcState::NotInstalled)
        .collect();
```

на

```rust
    // winws2 holds WinDivert too: removing the driver under it would break zapret2 (spec 9).
    let orphan = matches!(windivert, SvcState::Running | SvcState::StopPending)
        && !crate::procs::is_running("winws.exe")
        && !crate::procs::is_running(WINWS2);
    let conflicts = conflicts(installed);
```

в `remove_conflicts` строку `for name in CONFLICTING {` заменить на `for name in conflicts(installed) {`. Текст проверки `Conflicts` и подтверждение окна перечисляют найденные имена, поэтому `zapret2` в них попадает сам.

- [ ] **Шаг 6: Вызовы**

`crates/zapret/src/custom.rs`, в `dry_run`: `crate::procs::Winws::start(folder, &tail)?` заменить на `crate::procs::Winws::start(&folder.winws(), &folder.bin(), &tail)?`.

`crates/zapret/src/snapshot.rs`, в `take`: `strategy: service::strategy(),` заменить на `strategy: service::strategy_of(SERVICE),`; в `restore_service`: `service::create(&snap.image_path, snap.auto_start)?;` — на `service::create(SERVICE, &snap.image_path, snap.auto_start)?;`, `service::set_strategy(name)?;` — на `service::set_strategy_of(SERVICE, name)?;`. Задача 2 перепишет эти места для обоих движков.

`crates/tester/src/run.rs`, в `run_one`: `Winws::start(folder, &parsed.command_tail())` заменить на `Winws::start(&folder.winws(), &folder.bin(), &parsed.command_tail())`.

- [ ] **Шаг 7: Тесты проходят**

Run: `cargo test -p zapret --test service --test checks`
Expected: PASS: `service` — 7 тестов и 2 `ignored`, `checks` — 5 тестов и 1 `ignored`. Тесты с `cmd.exe` идут около 2 с.

Run: `cargo test -p zapret -p tester`
Expected: PASS; `ignored` — только тесты с пометкой о правах администратора.

- [ ] **Шаг 8: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 9: Коммит**

```bash
git add crates/zapret/src/service.rs crates/zapret/src/procs.rs crates/zapret/src/checks.rs crates/zapret/src/custom.rs crates/zapret/src/snapshot.rs crates/tester/src/run.rs crates/zapret/tests/service.rs crates/zapret/tests/checks.rs
git commit -m "feat(zapret): know the zapret2 service and winws2 by name"
```

---

### Задача 2: Снимок обоих движков

**Files:**
- Modify: `crates/zapret/src/snapshot.rs`
- Test: `crates/zapret/tests/snapshot.rs`

**Interfaces:**
- Consumes: `zapret::service::{SERVICE2, WINWS2, strategy_of, set_strategy_of, create(name, …)}` (задача 1).
- Produces: `zapret::snapshot::Snapshot` с новыми полями `service2: Option<ServiceSnap>`, `strategy2: Option<String>`, `manual_winws2: Vec<Proc>` (все `#[serde(default)]`). `Snapshot::take` и `Snapshot::restore` — прежние сигнатуры; откат: файлы → остановить обе службы и оба процесса → вернуть обе службы → запустить ручные процессы обоих движков (спецификация 6).

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret/tests/snapshot.rs`, в функции `sample` после поля `manual_winws: vec![…],` (перед `ipset,`) добавить:

```rust
        service2: Some(ServiceSnap {
            image_path: r#""D:\zapret2\winws2.exe" --chdir="D:\zapret2" --wf-tcp-out=443"#.into(),
            auto_start: true,
            running: false,
        }),
        strategy2: Some("zapret2:mine".into()),
        manual_winws2: vec![Proc {
            pid: 43,
            exe: Some(PathBuf::from(r"D:\zapret2\winws2.exe")),
            args: vec!["--wf-tcp-out=443".into()],
        }],
```

после теста `pending_json_round_trips` добавить:

```rust
#[test]
fn a_pending_json_from_before_zapret2_still_loads() {
    let data = tempfile::tempdir().unwrap();
    let path = pending_path(data.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        r#"{
  "operation": "apply",
  "service": {
    "image_path": "\"E:\\Zapret\\bin\\winws.exe\"   --wf-tcp 80",
    "auto_start": true,
    "running": true
  },
  "strategy": "general (ALT11)",
  "manual_winws": [],
  "ipset": "loaded",
  "ipset_backup": false,
  "game_filter": {
    "mode": "tcp",
    "tcp": "2000-2100",
    "udp": "1024-65535"
  }
}"#,
    )
    .unwrap();
    let snap = Snapshot::load(&path).unwrap().unwrap();
    assert_eq!(snap.strategy.as_deref(), Some("general (ALT11)"));
    assert_eq!(snap.service2, None);
    assert_eq!(snap.strategy2, None);
    assert!(snap.manual_winws2.is_empty());
}
```

Это `pending.json`, который записала бы версия 0.2.0, ровно в её формате.

в тесте `live_begin_restores_a_pending_json_first` после `assert_eq!(snap.strategy, earlier.strategy);` добавить:

```rust
    assert_eq!(snap.service2, earlier.service2);
    assert_eq!(snap.strategy2, earlier.strategy2);
```

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret --test snapshot`
Expected: FAIL при компиляции: у `Snapshot` нет полей `service2`, `strategy2`, `manual_winws2` (E0560, E0609).

- [ ] **Шаг 3: Снимок**

`crates/zapret/src/snapshot.rs`, строку `use crate::service::{self, SERVICE, SvcState};` заменить на:

```rust
use crate::service::{self, SERVICE, SERVICE2, SvcState, WINWS2};
```

в `pub struct Snapshot` после `pub manual_winws: Vec<Proc>,` добавить:

```rust
    /// The same three for zapret2: the `zapret2` service, its `zapret2:<name>` and winws2 started
    /// by hand (zapret2 spec 6). A `pending.json` from before zapret2 has none of them.
    #[serde(default)]
    pub service2: Option<ServiceSnap>,
    #[serde(default)]
    pub strategy2: Option<String>,
    #[serde(default)]
    pub manual_winws2: Vec<Proc>,
```

функцию `take` вместе с doc-комментарием заменить на:

```rust
    /// Fails closed: what it cannot read would be restored as something else, so an operation
    /// does not start on it.
    pub fn take(folder: &Folder, operation: &str) -> anyhow::Result<Self> {
        let (service, strategy, manual_winws) = engine(SERVICE, "winws.exe", operation)?;
        let (service2, strategy2, manual_winws2) = engine(SERVICE2, WINWS2, operation)?;
        Ok(Self {
            operation: operation.to_owned(),
            service,
            strategy,
            manual_winws,
            service2,
            strategy2,
            manual_winws2,
            ipset: ipset::try_mode(folder)?,
            ipset_backup: ipset::has_backup(folder),
            game_filter: game_filter::try_read(folder)?,
        })
    }
```

в `restore` заменить

```rust
        note("stop zapret", service::stop(SERVICE));
        note("stop winws", procs::kill_all("winws.exe"));
        note("service", self.restore_service());
        for proc in &self.manual_winws {
```

на

```rust
        for (name, image) in [(SERVICE, "winws.exe"), (SERVICE2, WINWS2)] {
            note(&format!("stop {name}"), service::stop(name));
            note(&format!("stop {image}"), procs::kill_all(image));
        }
        note(
            "service",
            restore_service(SERVICE, self.service.as_ref(), self.strategy.as_deref()),
        );
        note(
            "service zapret2",
            restore_service(SERVICE2, self.service2.as_ref(), self.strategy2.as_deref()),
        );
        for proc in self.manual_winws.iter().chain(&self.manual_winws2) {
```

Метод `restore_service` (последний в `impl Snapshot`) удалить целиком, а после закрывающей скобки `impl Snapshot` добавить две свободные функции:

```rust
/// One engine's service, its recorded strategy and the processes of its image the service did
/// not start. Fails closed, as `take`.
fn engine(
    name: &str,
    image: &str,
    operation: &str,
) -> anyhow::Result<(Option<ServiceSnap>, Option<String>, Vec<Proc>)> {
    let status = service::status(name);
    let config = service::config(name);
    anyhow::ensure!(
        status.is_none() || config.is_some(),
        "the {name} service exists, but its ImagePath could not be read from the registry, so \"{operation}\" did not start; remove the service with `sc delete {name}` if it is broken"
    );
    let snap = config.map(|config| ServiceSnap {
        image_path: config.image_path,
        auto_start: config.auto_start,
        running: matches!(
            status,
            Some((SvcState::Running | SvcState::StartPending, _))
        ),
    });
    let service_pid = status.map(|(_, pid)| pid).filter(|pid| *pid != 0);
    let manual = procs::find(image)
        .into_iter()
        .filter(|p| Some(p.pid) != service_pid)
        .collect();
    Ok((snap, service::strategy_of(name), manual))
}

/// One engine's service as the snapshot holds it: gone when there was none, else its
/// configuration, strategy name and state.
fn restore_service(
    name: &str,
    snap: Option<&ServiceSnap>,
    strategy: Option<&str>,
) -> anyhow::Result<()> {
    let Some(snap) = snap else {
        return service::stop_and_delete(name);
    };
    let same = service::config(name).is_some_and(|config| {
        config.image_path == snap.image_path && config.auto_start == snap.auto_start
    });
    if !same {
        service::stop_and_delete(name)?;
        service::create(name, &snap.image_path, snap.auto_start)?;
    }
    if let Some(strategy) = strategy {
        service::set_strategy_of(name, strategy)?;
    }
    if snap.running {
        service::start(name)
    } else {
        service::stop(name)
    }
}
```

Обе службы сначала останавливаются, потом возвращаются: если операция успела поставить `zapret2` вместо `zapret`, старая служба не встречает работающую новую на том же WinDivert. Если `zapret2` до операции не было, `restore_service` её удаляет.

- [ ] **Шаг 4: Тесты проходят**

Run: `cargo test -p zapret --test snapshot`
Expected: PASS, 11 тестов и 2 `ignored`.

Run: `cargo test -p zapret -p tester`
Expected: PASS.

- [ ] **Шаг 5: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 6: Коммит**

```bash
git add crates/zapret/src/snapshot.rs crates/zapret/tests/snapshot.rs
git commit -m "feat(zapret): snapshot and restore the zapret2 service too"
```

---

### Задача 3: Пакет `zapret2` — папка движка и версия

**Files:**
- Modify: `Cargo.toml` (`[workspace.dependencies]`)
- Create: `crates/zapret2/Cargo.toml`, `crates/zapret2/src/lib.rs`
- Test: `crates/zapret2/tests/common/mod.rs`, `crates/zapret2/tests/engine.rs`

**Interfaces:**
- Consumes: `zapret::checks::has_cyrillic(&str) -> bool`, `zapret::read_text(&Path) -> anyhow::Result<String>`, `exec::run(&mut Command, Duration, Option<…>) -> Result<exec::Output, …>`, `exec::Output::{check, describe}` — всё уже есть.
- Produces (крейт `zapret2`):
  - `pub const PREFIX: &str = "zapret2:"`;
  - `pub struct Engine` (`Debug, Clone, PartialEq, Eq`): `Engine::open(dir: impl AsRef<Path>) -> anyhow::Result<Engine>`, `root(&self) -> &Path`, `exe(&self) -> &Path`, `lua(&self) -> PathBuf`, `version(&self) -> anyhow::Result<Version>` (запускает `winws2.exe --version`, нужен администратор; кэш до смены файлов);
  - `pub struct Version { pub text: String, pub compat: u32 }`;
  - `pub fn parse_banner(output: &str) -> Option<(String, u32)>`, `pub fn required_compat(lib_lua: &str) -> Option<u32>`;
  - `pub struct Info { pub dir: Option<String>, pub version: Option<String>, pub compat: Option<u32>, pub error: Option<String> }` (`Default`, serde, `specta::Type`) и `pub fn info(dir: Option<&str>) -> Info` — для «Настроек», «Обзора» и `zapret2_info`.

- [ ] **Шаг 1: Пакет**

В корневом `Cargo.toml`, в `[workspace.dependencies]`, после `zapret = { path = "crates/zapret" }`:

```toml
zapret2 = { path = "crates/zapret2" }
```

`crates/zapret2/Cargo.toml`:

```toml
[package]
name = "zapret2"
version.workspace = true
edition.workspace = true
rust-version.workspace = true
license.workspace = true
publish.workspace = true
description = "bol-van/zapret2 as Zapret Hub's second engine: its folder, strategies, checks and service."

[dependencies]
exec.workspace = true
zapret.workspace = true
anyhow.workspace = true
log.workspace = true
serde.workspace = true
specta.workspace = true

[dev-dependencies]
tempfile.workspace = true
```

`log` понадобится в задаче 4, сейчас он не мешает: неиспользуемые зависимости не дают предупреждений.

`crates/zapret2/src/lib.rs` — пока только doc-комментарий:

```rust
//! bol-van/zapret2 (`winws2.exe`, strategies in Lua) as Zapret Hub's second engine (zapret2
//! spec): its folder and version.
```

- [ ] **Шаг 2: Падающие тесты**

`crates/zapret2/tests/common/mod.rs`:

```rust
#![allow(dead_code)]

use std::fs;
use std::path::Path;

/// A zapret2 folder under `root` with `winws2.exe` and its DLLs in `exe_dir` (`""`, `bin`, ...)
/// and `lua\`; `lib` is the text of `lua\zapret-lib.lua`. The files are empty: nothing runs them.
pub fn fake_engine(root: &Path, exe_dir: &str, lib: &str) {
    let exe_dir = root.join(exe_dir);
    fs::create_dir_all(&exe_dir).unwrap();
    for file in [
        "winws2.exe",
        "cygwin1.dll",
        "WinDivert.dll",
        "WinDivert64.sys",
    ] {
        fs::write(exe_dir.join(file), b"").unwrap();
    }
    fs::create_dir_all(root.join("lua")).unwrap();
    fs::write(root.join("lua").join("zapret-lib.lua"), lib).unwrap();
    fs::write(root.join("lua").join("zapret-antidpi.lua"), "").unwrap();
}

/// `zapret-lib.lua` as a release starts it.
pub const LIB: &str =
    "NFQWS2_COMPAT_VER_REQUIRED=6\n\nif NFQWS2_COMPAT_VER~=NFQWS2_COMPAT_VER_REQUIRED then\n";
```

`crates/zapret2/tests/engine.rs`:

```rust
mod common;

use std::path::Path;

use zapret2::{Engine, info, parse_banner, required_compat};

#[test]
fn winws2_is_found_in_every_layout() {
    for (exe_dir, exe) in [
        ("", "winws2.exe"),
        ("bin", r"bin\winws2.exe"),
        (
            r"binaries\windows-x86_64",
            r"binaries\windows-x86_64\winws2.exe",
        ),
    ] {
        let dir = tempfile::tempdir().unwrap();
        common::fake_engine(dir.path(), exe_dir, common::LIB);
        let engine = Engine::open(dir.path()).unwrap();
        assert_eq!(engine.root(), dir.path());
        assert_eq!(engine.exe(), dir.path().join(exe));
    }
}

#[test]
fn the_bundle_root_is_its_zapret_winws_folder() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("zapret-winws");
    common::fake_engine(&root, "", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    assert_eq!(engine.root(), root);
    assert_eq!(engine.lua(), root.join("lua"));
}

#[test]
fn a_trailing_backslash_is_not_part_of_the_root() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "", common::LIB);
    let picked = format!(r"{}\", dir.path().display());
    let engine = Engine::open(&picked).unwrap();
    assert_eq!(engine.root().as_os_str(), dir.path().as_os_str());
}

fn error(dir: &Path) -> String {
    format!("{:#}", Engine::open(dir).unwrap_err())
}

#[test]
fn every_missing_file_is_named() {
    let dir = tempfile::tempdir().unwrap();
    let err = error(dir.path());
    for part in [
        "is not a zapret2 folder",
        "winws2.exe",
        r"lua\zapret-lib.lua",
        r"lua\zapret-antidpi.lua",
    ] {
        assert!(err.contains(part), "{part}: {err}");
    }
    common::fake_engine(dir.path(), "bin", common::LIB);
    std::fs::remove_file(dir.path().join("bin").join("cygwin1.dll")).unwrap();
    std::fs::remove_file(dir.path().join("bin").join("WinDivert64.sys")).unwrap();
    let err = error(dir.path());
    assert!(
        err.ends_with(r"missing bin\cygwin1.dll, bin\WinDivert64.sys"),
        "{err}"
    );
}

#[test]
fn a_cyrillic_path_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("Запрет");
    common::fake_engine(&root, "", common::LIB);
    assert!(error(&root).contains("Cyrillic"), "{}", error(&root));
}

#[test]
fn the_banner_gives_the_version_and_the_compat_level() {
    let github =
        "github version v1.0.5.2 (6b6c63e3385fa73f8af3be4a69171e947f5a319d) lua_compat_ver 6\n\n";
    assert_eq!(
        parse_banner(github),
        Some((
            "v1.0.5.2 (6b6c63e3385fa73f8af3be4a69171e947f5a319d)".into(),
            6
        ))
    );
    let own = "self-built version Oct 11 2026 12:00:00 lua_compat_ver 5\r\n";
    assert_eq!(
        parse_banner(own),
        Some(("self-built version Oct 11 2026 12:00:00".into(), 5))
    );
    assert_eq!(parse_banner("winws2: unknown option\n"), None);
    assert_eq!(parse_banner("lua_compat_ver six"), None);
}

#[test]
fn the_lua_files_say_which_compat_level_they_need() {
    assert_eq!(required_compat(common::LIB), Some(6));
    assert_eq!(
        required_compat("NFQWS2_COMPAT_VER_REQUIRED = 7\r\n"),
        Some(7)
    );
    assert_eq!(
        required_compat("if NFQWS2_COMPAT_VER~=NFQWS2_COMPAT_VER_REQUIRED then"),
        None
    );
}

#[test]
fn info_is_empty_without_a_folder_and_names_what_is_wrong() {
    assert_eq!(info(None), zapret2::Info::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().display().to_string();
    let shown = info(Some(&path));
    assert_eq!(shown.dir.as_deref(), Some(path.as_str()));
    assert_eq!(shown.version, None);
    assert!(
        shown.error.is_some_and(|err| err.contains("winws2.exe")),
        "the error names the missing engine"
    );
}

#[test]
#[ignore = "needs admin rights and an unpacked zapret2 release in ZAPRET2_DIR"]
fn live_version_of_a_release() {
    let dir = std::env::var("ZAPRET2_DIR").expect("ZAPRET2_DIR is the zapret2 folder");
    let version = Engine::open(&dir).unwrap().version().unwrap();
    eprintln!("{version:?}");
    assert_eq!(version.compat, 6);
    let shown = info(Some(&dir));
    assert_eq!(shown.version, Some(version.text));
    assert_eq!(shown.error, None);
}
```

Баннеры — дословные форматы `MAKE_VER` из `nfq2/nfqws.c` (zapret2 `a1adf7b`): `github version <тег> (<коммит>) lua_compat_ver <N>` у релизов и `self-built version <дата> <время> lua_compat_ver <N>` у своей сборки; `LUA_COMPAT_VER` в `nfq2/nfqws.h` — 6, первая строка `lua/zapret-lib.lua` — `NFQWS2_COMPAT_VER_REQUIRED=6`. Тест `live_…` пользователь запускает сам из повышенного терминала: `$env:ZAPRET2_DIR='D:\zapret2'; cargo test -p zapret2 --test engine -- --ignored`.

- [ ] **Шаг 3: Убедиться, что тесты падают**

Run: `cargo test -p zapret2 --test engine`
Expected: FAIL при компиляции: `unresolved imports zapret2::Engine, zapret2::info, zapret2::parse_banner, zapret2::required_compat` (E0432), `cannot find Info in zapret2` (E0433).

- [ ] **Шаг 4: Папка и версия**

`crates/zapret2/src/lib.rs` целиком:

```rust
//! bol-van/zapret2 (`winws2.exe`, strategies in Lua) as Zapret Hub's second engine (zapret2
//! spec): its folder and version.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, SystemTime};

/// Every zapret2 strategy's name starts with it: `zapret2:general` (zapret2 spec 3.1). A `.bat`
/// file name has no colon, so the names never meet Flowseal's.
pub const PREFIX: &str = "zapret2:";

/// Where `winws2.exe` may be under the root: zapret-win-bundle, a `bin\`, the release zip.
const EXE_DIRS: [&str; 3] = ["", "bin", r"binaries\windows-x86_64"];
/// What `winws2.exe` needs next to it.
const BESIDE_EXE: [&str; 3] = ["cygwin1.dll", "WinDivert.dll", "WinDivert64.sys"];
/// What every strategy loads from `lua\`.
const LUA_FILES: [&str; 2] = ["zapret-lib.lua", "zapret-antidpi.lua"];

/// A zapret2 folder with what `winws2.exe` needs (zapret2 spec 4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Engine {
    root: PathBuf,
    exe: PathBuf,
}

/// What `winws2.exe --version` says, checked against the Lua files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// `v1.0.5.2 (6b6c63e…)`, or `self-built version <date> <time>`.
    pub text: String,
    /// `lua_compat_ver`.
    pub compat: u32,
}

impl Engine {
    /// The root is `dir` or its `zapret-winws` (zapret-win-bundle), whichever holds
    /// `lua\zapret-lib.lua`. Fails naming every missing file, and on a path with Cyrillic letters:
    /// Cygwin does not carry them.
    pub fn open(dir: impl AsRef<Path>) -> anyhow::Result<Self> {
        let dir = dir.as_ref();
        let root: PathBuf = [dir.to_path_buf(), dir.join("zapret-winws")]
            .into_iter()
            .find(|root| root.join("lua").join("zapret-lib.lua").is_file())
            .unwrap_or_else(|| dir.to_path_buf())
            .components()
            .collect();
        anyhow::ensure!(
            !zapret::checks::has_cyrillic(&root.to_string_lossy()),
            "{} has Cyrillic letters, which winws2 (a Cygwin program) cannot use: move zapret2 to a folder with a Latin path",
            root.display()
        );
        let exe_dir = EXE_DIRS
            .iter()
            .map(|sub| root.join(sub))
            .find(|dir| dir.join("winws2.exe").is_file());
        let mut missing = Vec::new();
        match &exe_dir {
            Some(exe_dir) => {
                let sub = exe_dir.strip_prefix(&root).unwrap_or(exe_dir);
                for file in BESIDE_EXE {
                    if !exe_dir.join(file).is_file() {
                        missing.push(sub.join(file).display().to_string());
                    }
                }
            }
            None => missing.push("winws2.exe".to_owned()),
        }
        for file in LUA_FILES {
            let path = Path::new("lua").join(file);
            if !root.join(&path).is_file() {
                missing.push(path.display().to_string());
            }
        }
        let exe = match exe_dir {
            Some(exe_dir) if missing.is_empty() => exe_dir.join("winws2.exe"),
            _ => anyhow::bail!(
                "{} is not a zapret2 folder: missing {}",
                root.display(),
                missing.join(", ")
            ),
        };
        Ok(Self { root, exe })
    }

    /// Where `lua\` is; `--chdir` makes it the working directory of every run.
    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn exe(&self) -> &Path {
        &self.exe
    }

    pub fn lua(&self) -> PathBuf {
        self.root.join("lua")
    }

    /// `winws2.exe --version`, run again only when `winws2.exe` or `lua\zapret-lib.lua` changes:
    /// the overview asks on every change. Needs admin rights (winws2's manifest asks for them). A
    /// `lua_compat_ver` other than the one `lua\zapret-lib.lua` requires is an error: the engine
    /// and the Lua files come from different releases.
    // ponytail: an error stays cached until a file changes; a restart reads it again.
    pub fn version(&self) -> anyhow::Result<Version> {
        type Key = (PathBuf, Option<SystemTime>, Option<SystemTime>);
        static CACHE: Mutex<Option<(Key, Result<Version, String>)>> = Mutex::new(None);
        let modified = |path: &Path| std::fs::metadata(path).and_then(|m| m.modified()).ok();
        let lib = self.lua().join("zapret-lib.lua");
        let key = (self.exe.clone(), modified(&self.exe), modified(&lib));
        let mut cache = CACHE.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some((cached, result)) = &*cache
            && *cached == key
        {
            return result.clone().map_err(anyhow::Error::msg);
        }
        let result = self.read_version(&lib).map_err(|err| format!("{err:#}"));
        *cache = Some((key, result.clone()));
        result.map_err(anyhow::Error::msg)
    }

    fn read_version(&self, lib: &Path) -> anyhow::Result<Version> {
        let mut cmd = Command::new(&self.exe);
        cmd.arg("--version").current_dir(&self.root);
        let output = exec::run(&mut cmd, Duration::from_secs(15), None)?.check()?;
        let (text, compat) = parse_banner(&output.stdout).ok_or_else(|| {
            anyhow::anyhow!("winws2.exe printed no version:\n{}", output.describe())
        })?;
        let required = required_compat(&zapret::read_text(lib)?).ok_or_else(|| {
            anyhow::anyhow!("{} has no NFQWS2_COMPAT_VER_REQUIRED line", lib.display())
        })?;
        anyhow::ensure!(
            compat == required,
            "winws2.exe {text} has lua_compat_ver {compat}, but {} requires {required}: the engine and the Lua files come from different zapret2 releases",
            lib.display()
        );
        Ok(Version { text, compat })
    }
}

/// The banner every winws2 run prints first: `github version v1.0.5.2 (6b6c63e) lua_compat_ver 6`
/// gives `v1.0.5.2 (6b6c63e)` and 6; a self-built one keeps its whole text.
pub fn parse_banner(output: &str) -> Option<(String, u32)> {
    output.lines().find_map(|line| {
        let (version, compat) = line.trim().rsplit_once(" lua_compat_ver ")?;
        let compat = compat.trim().parse().ok()?;
        let version = version.strip_prefix("github version ").unwrap_or(version);
        Some((version.to_owned(), compat))
    })
}

/// `NFQWS2_COMPAT_VER_REQUIRED=6`, the first line of `lua\zapret-lib.lua`.
pub fn required_compat(lib_lua: &str) -> Option<u32> {
    lib_lua.lines().find_map(|line| {
        line.trim()
            .strip_prefix("NFQWS2_COMPAT_VER_REQUIRED")?
            .trim_start()
            .strip_prefix('=')?
            .trim()
            .parse()
            .ok()
    })
}

/// The zapret2 folder as «Настройки», «Обзор» and `zapret2_info` show it.
#[derive(
    Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize, specta::Type,
)]
pub struct Info {
    /// The `zapret2_dir` setting; `None` while zapret2 is off.
    pub dir: Option<String>,
    /// `v1.0.5.2 (6b6c63e…)`.
    pub version: Option<String>,
    /// `lua_compat_ver`, the same in the engine and its Lua files.
    pub compat: Option<u32>,
    /// Why the folder cannot be used.
    pub error: Option<String>,
}

pub fn info(dir: Option<&str>) -> Info {
    let Some(dir) = dir else {
        return Info::default();
    };
    let version = Engine::open(dir).and_then(|engine| engine.version());
    Info {
        dir: Some(dir.to_owned()),
        version: version.as_ref().ok().map(|v| v.text.clone()),
        compat: version.as_ref().ok().map(|v| v.compat),
        error: version.err().map(|err| format!("{err:#}")),
    }
}
```

Пояснения:
- Корень нормализуется через `components()`: путь `D:\zapret2\` из диалога превращается в `D:\zapret2`, и `--chdir="D:\zapret2"` (задача 4) не получает `\` перед кавычкой.
- `version` запускает `winws2.exe` (манифест требует прав администратора), поэтому её тестирует только `live_…`. Ключ кэша — путь к exe и время изменения exe и `lua\zapret-lib.lua`: замена любого из них — новое чтение. Ошибка кэшируется так же, иначе «Обзор» запускал бы сломанный exe на каждое обновление.
- Проверка кириллицы — та же `has_cyrillic`, что у системной проверки Flowseal (core 5.7).

- [ ] **Шаг 5: Тесты проходят**

Run: `cargo test -p zapret2 --test engine`
Expected: PASS, 8 тестов и 1 `ignored`.

- [ ] **Шаг 6: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 7: Коммит**

```bash
git add Cargo.toml Cargo.lock crates/zapret2/Cargo.toml crates/zapret2/src/lib.rs crates/zapret2/tests/common/mod.rs crates/zapret2/tests/engine.rs
git commit -m "feat(zapret2): open a zapret2 folder and read its version"
```

---

### Задача 4: Стратегии zapret2 — встроенные, свои, командная строка и статическая проверка

**Files:**
- Modify: `crates/zapret/src/batparse.rs` (`replace_ci` → `pub`), `crates/zapret/src/custom.rs` (`quoted_tokens` → `pub`), `crates/zapret2/src/lib.rs` (`pub mod strategies;`)
- Create: `crates/zapret2/src/strategies.rs`, `crates/zapret2/src/builtin/general.txt`, `crates/zapret2/src/builtin/general-sni.txt`
- Create (данные тестов): `crates/zapret2/tests/fixtures/functions.txt`, `crates/zapret2/tests/fixtures/flowseal-1.10.3.txt`
- Test: `crates/zapret2/tests/common/mod.rs` (+ `fake_flowseal`, `lib_with_functions`), `crates/zapret2/tests/strategies.rs`

**Interfaces:**
- Consumes: `zapret2::{Engine, PREFIX}` (задача 3); `zapret::custom::validate_name(&str) -> anyhow::Result<String>`, `zapret::strategies::natural_key`, `zapret::game_filter::try_read(&Folder)` и `.vars()` (поля `all`, `tcp`, `udp`), `zapret::folder::Folder::{lists, bin}`, `zapret::read_text` — уже есть; `zapret::batparse::replace_ci`, `zapret::custom::quoted_tokens` — становятся `pub` здесь.
- Produces (модуль `zapret2::strategies`):
  - `pub struct Strategy { pub name: String, pub text: String, pub builtin: bool }` и `Strategy::full_name(&self) -> String` (`zapret2:<name>`);
  - `pub fn dir(data_dir: &Path) -> PathBuf` — `data\zapret2`;
  - `pub fn list(data_dir: &Path) -> anyhow::Result<Vec<Strategy>>` — встроенные, затем свои в естественном порядке; `pub fn find(data_dir: &Path, name: &str) -> anyhow::Result<Strategy>` (имя без префикса, без учёта регистра);
  - `pub fn claim(data_dir: &Path, name: &str, create: bool) -> anyhow::Result<String>`, `pub fn write(data_dir: &Path, name: &str, text: &str) -> anyhow::Result<()>`, `pub fn delete(data_dir: &Path, name: &str, active: Option<&str>) -> anyhow::Result<()>`;
  - `pub fn args(text: &str) -> String` — текст без строк-комментариев одной строкой;
  - `pub fn tail(engine: &Engine, folder: Option<&Folder>, text: &str) -> anyhow::Result<String>` — `--chdir="<корень>" ` + аргументы с подстановками;
  - `pub fn check(engine: &Engine, tail: &str) -> anyhow::Result<()>` — статическая проверка;
  - `pub fn split(args: &str) -> (String, Vec<String>)` — глобальная часть и профили для карточки.

- [ ] **Шаг 1: Помощники `zapret` открыты для `zapret2`**

`crates/zapret/src/batparse.rs`, строку `fn replace_ci(hay: &str, needle: &str, with: &str) -> String {` заменить на:

```rust
/// Every case-insensitive occurrence of an ASCII `needle` replaced, as cmd expands `%VAR%`.
pub fn replace_ci(hay: &str, needle: &str, with: &str) -> String {
```

`crates/zapret/src/custom.rs`, строку `fn quoted_tokens(text: &str) -> Vec<String> {` заменить на `pub fn quoted_tokens(text: &str) -> Vec<String> {`.

Подстановки zapret2 разворачиваются тем же `replace_ci`, что `%BIN%` и `%LISTS%` у Flowseal (без учёта регистра, как cmd), а аргументы делятся тем же `quoted_tokens`, что в редакторе своих стратегий Flowseal.

- [ ] **Шаг 2: Данные тестов**

`crates/zapret2/tests/fixtures/functions.txt` — список функций, которые определяют `lua/zapret-lib.lua`, `lua/zapret-antidpi.lua` и `lua/zapret-auto.lua` zapret2 на `a1adf7b`. Статическая проверка встроенных стратегий идёт по нему, без исходников zapret2:

```text
# Every "function <name>(" in lua/zapret-lib.lua, zapret-antidpi.lua and zapret-auto.lua of
# bol-van/zapret2 at a1adf7b (master after v1.0.5.2), for the static check of the built-in
# strategies. Regenerate from a zapret2 checkout (Git Bash):
# grep -ho 'function [A-Za-z_][A-Za-z0-9_]*(' lua/zapret-{lib,antidpi,auto}.lua | sed 's/^function //; s/($//' | sort -u
append_path apply_arg_prefix apply_execution_plan apply_fooling apply_ip_id argdebug
array_field_search array_search automate_conn_record automate_failure_check automate_failure_counter
automate_failure_counter_reset automate_host_record autottl autottl_eval barray blob blob_exist
blob_or_def bquic_tvb btable circular cond_false cond_lua cond_payload_str cond_random
cond_tcp_has_ts cond_true condition count_strategies cutoff_shim_check dbg deepcopy del_ip6_exthdr
delete_pos_1 desync_copy desync_opts desync_orchestrator_example desync_timer_name
detect_payload_str dht_dn direction_check direction_cutoff_opposite dis_ipdst dis_ipsrc dis_l4_name
dis_l4_ports dis_reconstruct_l3 dis_reverse dis_timer_name dissect_nld dissect_url drop fake
fakeddisorder fakedsplit find_ip6_exthdr find_next_line find_tcp_option fix_ip6_next fix_ip_proto
frag_idx genhost gunzip_file gzip_file has_nonprintable hexdump hexdump_dlog hop_count_guess host_ip
host_or_ip hostfakesplit http_dissect_header http_dissect_headers http_dissect_reply
http_dissect_req http_domcase http_hostcase http_methodeol http_reconstruct_headers
http_reconstruct_req http_unixeol in_list insert_ip6_exthdr instance_cutoff_shim ip2ifname ip_proto
ip_proto_l3 ip_proto_l4 ipfrag2 is_absolute_path is_dpi_redirect is_gzip_file is_retransmission
is_tls_handshake is_tls_handshake_type_hello is_tls_hello is_tls_record l3_base_len l3_extra_len
l3_len l3l4_extra_len l3l4_len l4_base_len l4_extra_len l4_len len16_header len8_header logical_xor
luaexec make_readable move_ts_top multidisorder multidisorder_legacy multidisorder_send multisplit
oob orchestrate packet_len parse_autottl parse_tcp_flags pass pattern payload_check
payload_match_filter per_instance_condition pktdebug pktmod plan_clear plan_instance_execute
plan_instance_execute_preapplied plan_instance_pop pos_array_normalize pos_check_from
pos_check_range pos_check_to pos_counter_overflow pos_get pos_get_pos pos_normalize pos_range_str
pos_str posdebug prepare_bin quic_tvb rawsend_dissect_ipfrag rawsend_dissect_segmented rawsend_opts
rawsend_opts_base rawsend_payload_segmented readfile reconstruct_opts repeater replay_drop
replay_drop_key replay_drop_set replay_execution_plan replay_first require_iff rst send
send_timer_delayed seq_ge seq_gt seq_le seq_lt seq_within standard_detector_defaults
standard_failure_detector standard_hostkey standard_success_detector stopif str_or_hex string2hex
synack synack_split syndata tcpseg tls_client_hello_clone tls_client_hello_mod tls_dissect
tls_dissect_ext tls_dissect_handshake tls_handshake_data_len tls_handshake_full tls_handshake_len
tls_handshake_type tls_mod_shim tls_reconstruct tls_reconstruct_ext tls_reconstruct_handshake
tls_record_data_len tls_record_full tls_record_type ttl_discover udplen update_ifaddrs var_debug
verdict_aggregate writable_file_name writefile wsize wsize_rewrite wssize z_readfile zero_based_pos
```

`crates/zapret2/tests/fixtures/flowseal-1.10.3.txt` — файлы `lists\` и `bin\` Flowseal 1.10.3, которые могут назвать `%LISTS%` и `%BIN%`:

```text
# lists\ and bin\ of Flowseal zapret-discord-youtube 1.10.3 (the -user lists are made by
# service.bat and Zapret Hub at the first start): the files %LISTS% and %BIN% may name.
lists\discord-voice-proxy.txt
lists\ipset-all.txt
lists\ipset-exclude-user.txt
lists\ipset-exclude.txt
lists\list-exclude-user.txt
lists\list-exclude.txt
lists\list-general-user.txt
lists\list-general.txt
lists\list-google.txt
bin\ACTIVE_DISCORD_UDP.bin
bin\ACTIVE_GAME_UDP.bin
bin\cygwin1.dll
bin\quic_initial_4pda_to.bin
bin\quic_initial_5ka_ru.bin
bin\quic_initial_rutube_ru.bin
bin\quic_initial_steamcommunity_com.bin
bin\quic_initial_tencent_com.bin
bin\quic_initial_www_google_com.bin
bin\stun.bin
bin\stun2.bin
bin\tls_clienthello_4pda_to.bin
bin\tls_clienthello_5ka_ru.bin
bin\tls_clienthello_max_ru.bin
bin\tls_clienthello_sochi_park.bin
bin\tls_clienthello_www_google_com.bin
bin\tls_clienthello_www_sferum_ru.bin
bin\WinDivert.dll
bin\WinDivert64.sys
bin\winws.exe
```

`crates/zapret2/tests/common/mod.rs`, в конец файла:

```rust
/// A Flowseal folder with what `Folder::open` checks, `utils\`, and `files` (`lists\x.txt`,
/// `bin\y.bin`) empty.
pub fn fake_flowseal(root: &Path, files: &[&str]) -> zapret::folder::Folder {
    for dir in ["bin", "lists", "utils"] {
        fs::create_dir_all(root.join(dir)).unwrap();
    }
    fs::write(root.join("service.bat"), "set \"LOCAL_VERSION=1.10.3\"\r\n").unwrap();
    for file in [r"bin\winws.exe", r"bin\WinDivert64.sys"]
        .iter()
        .chain(files)
    {
        fs::write(root.join(file), b"").unwrap();
    }
    zapret::folder::Folder::open(root).unwrap()
}

/// `zapret-lib.lua` that defines every function zapret2's own Lua files define.
pub fn lib_with_functions() -> String {
    let names = include_str!("../fixtures/functions.txt");
    let mut lib = String::from(LIB);
    for name in names
        .lines()
        .filter(|line| !line.starts_with('#'))
        .flat_map(str::split_whitespace)
    {
        lib.push_str(&format!("function {name}(ctx, desync)\nend\n"));
    }
    lib
}
```

- [ ] **Шаг 3: Падающие тесты**

`crates/zapret2/tests/strategies.rs`:

```rust
mod common;

use std::fs;
use std::path::Path;

use zapret::game_filter::{GameFilter, GameFilterMode};
use zapret2::Engine;
use zapret2::strategies::{self, args, check, claim, split, tail};

fn engine(root: &Path) -> Engine {
    common::fake_engine(root, "", common::LIB);
    Engine::open(root).unwrap()
}

fn names(data: &Path) -> Vec<(String, bool)> {
    strategies::list(data)
        .unwrap()
        .into_iter()
        .map(|s| (s.name, s.builtin))
        .collect()
}

#[test]
fn built_in_ones_come_first_then_the_users_in_natural_order() {
    let data = tempfile::tempdir().unwrap();
    let own = strategies::dir(data.path());
    fs::create_dir_all(&own).unwrap();
    for file in ["b 10.txt", "b 9.txt", "a.txt", "General.txt", "notes.md"] {
        fs::write(own.join(file), "--wf-tcp-out=443").unwrap();
    }
    fs::write(own.join("bom.txt"), "\u{feff}--wf-tcp-out=80").unwrap();
    let expected: Vec<(String, bool)> = [
        ("general", true),
        ("general-sni", true),
        ("a", false),
        ("b 9", false),
        ("b 10", false),
        ("bom", false),
    ]
    .iter()
    .map(|(name, builtin)| (name.to_string(), *builtin))
    .collect();
    assert_eq!(names(data.path()), expected);
    let bom = strategies::find(data.path(), "BOM").unwrap();
    assert_eq!(bom.text, "--wf-tcp-out=80");
    assert_eq!(bom.full_name(), "zapret2:bom");
    assert!(strategies::find(data.path(), "c").is_err());
}

#[test]
fn without_a_folder_there_are_the_built_in_ones() {
    let data = tempfile::tempdir().unwrap();
    assert_eq!(
        names(data.path()),
        [
            ("general".to_owned(), true),
            ("general-sni".to_owned(), true)
        ]
    );
}

#[test]
fn a_save_claims_a_new_name_or_one_of_the_users() {
    let data = tempfile::tempdir().unwrap();
    let err =
        |name: &str, create: bool| format!("{:#}", claim(data.path(), name, create).unwrap_err());
    assert!(err("General", true).contains("built in"));
    assert!(err("mine", false).contains("no zapret2 strategy of yours"));
    assert!(err("a/b", true).contains("1-40"));
    assert_eq!(claim(data.path(), " mine ", true).unwrap(), "mine");
    strategies::write(data.path(), "mine", "--wf-tcp-out=443\n").unwrap();
    assert!(err("MINE", true).contains("taken"));
    assert_eq!(claim(data.path(), "mine", false).unwrap(), "mine");
}

#[test]
fn only_the_users_strategies_that_are_not_active_are_deleted() {
    let data = tempfile::tempdir().unwrap();
    strategies::write(data.path(), "mine", "--wf-tcp-out=443\n").unwrap();
    let err = |name: &str, active: Option<&str>| {
        format!(
            "{:#}",
            strategies::delete(data.path(), name, active).unwrap_err()
        )
    };
    assert!(err("general", None).contains("built in"));
    assert!(err("mine", Some("ZAPRET2:Mine")).contains("active"));
    strategies::delete(data.path(), "Mine", Some("zapret2:general")).unwrap();
    assert!(!strategies::dir(data.path()).join("mine.txt").exists());
}

#[test]
fn comment_lines_go_and_zapret2_hashes_stay() {
    let text = "# a comment\r\n--wf-tcp-out=443\r\n\r\n  # indented comment\n--lua-desync=fake:blob=x:tls_mod=sni=%var:repeats=#var\n";
    assert_eq!(
        args(text),
        "--wf-tcp-out=443 --lua-desync=fake:blob=x:tls_mod=sni=%var:repeats=#var"
    );
}

#[test]
fn the_command_line_fills_every_placeholder() {
    let dir = tempfile::tempdir().unwrap();
    // Spaces as in `C:\Program Files\…`: every path must stay one quoted argument.
    let engine = engine(&dir.path().join("zapret 2"));
    let flowseal = dir.path().join("Flowseal zapret");
    let folder = common::fake_flowseal(&flowseal, &[]);
    let text = "--wf-tcp-out=443,%GameFilterTCP% --wf-udp-out=443,%GameFilterUDP%\n\
                --lua-init=@\"%LUA%zapret-lib.lua\" --blob=a:@\"%FILES%fake\\a.bin\"\n\
                --hostlist=\"%LISTS%list-general.txt\" --blob=b:@\"%bin%stun.bin\" --filter-tcp=%GameFilter%";
    let root = engine.root().display().to_string();
    let lists = flowseal.join("lists").display().to_string();
    let bin = flowseal.join("bin").display().to_string();
    let line = tail(&engine, Some(&folder), text).unwrap();
    assert_eq!(
        line,
        format!(
            "--chdir=\"{root}\" --wf-tcp-out=443,12 --wf-udp-out=443,12 \
             --lua-init=@\"{root}\\lua\\zapret-lib.lua\" --blob=a:@\"{root}\\files\\fake\\a.bin\" \
             --hostlist=\"{lists}\\list-general.txt\" --blob=b:@\"{bin}\\stun.bin\" --filter-tcp=12"
        )
    );
    let tokens = zapret::custom::quoted_tokens(&line);
    assert_eq!(tokens.len(), 8, "{tokens:?}");
    assert_eq!(tokens[0], format!("--chdir=\"{root}\""));
    assert_eq!(
        tokens[5],
        format!("--hostlist=\"{lists}\\list-general.txt\"")
    );
    zapret::game_filter::write(
        &folder,
        &GameFilter {
            mode: GameFilterMode::Tcp,
            tcp: "1024-65535".into(),
            udp: "1024-65535".into(),
        },
    )
    .unwrap();
    let filtered = tail(&engine, Some(&folder), text).unwrap();
    assert!(
        filtered.contains("--wf-tcp-out=443,1024-65535 --wf-udp-out=443,12"),
        "{filtered}"
    );
    assert!(filtered.ends_with("--filter-tcp=1024-65535"), "{filtered}");
}

#[test]
fn flowseal_placeholders_need_the_flowseal_folder() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let err = format!(
        "{:#}",
        tail(&engine, None, "--hostlist=\"%lists%list-general.txt\"").unwrap_err()
    );
    assert!(
        err.contains("%LISTS%") && err.contains("zapret folder"),
        "{err}"
    );
    let own = tail(
        &engine,
        None,
        "--wf-tcp-out=443 --lua-init=@\"%LUA%zapret-lib.lua\"",
    )
    .unwrap();
    assert!(own.starts_with(&format!("--chdir=\"{}\" ", engine.root().display())));
}

#[test]
fn the_static_check_finds_every_desync_function_where_winws2_would() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    fs::write(
        engine.lua().join("zapret-antidpi.lua"),
        "function fake(ctx, desync)\nend\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("mine.lua"),
        "function mine(ctx, desync) end",
    )
    .unwrap();
    let elsewhere = tempfile::tempdir().unwrap();
    let theirs = elsewhere.path().join("theirs.lua");
    fs::write(&theirs, "function theirs(ctx, desync) end").unwrap();
    let ok = format!(
        "--chdir=\"{}\" --lua-init=@mine.lua --lua-init=@\"{}\" \
         --lua-init=\"function inline(ctx, desync) end\" --lua-desync=fake:blob=x \
         --new --lua-desync=mine --lua-desync=theirs:x=1 --lua-desync=inline",
        engine.root().display(),
        theirs.display()
    );
    check(&engine, &ok).unwrap();
    let err = format!(
        "{:#}",
        check(
            &engine,
            "--lua-desync=fakee:blob=x --lua-desync=multisplit:pos=1 --lua-desync=fakee"
        )
        .unwrap_err()
    );
    assert!(err.contains("fakee, multisplit"), "{err}");
    let err = format!(
        "{:#}",
        check(&engine, "--lua-init=@missing.lua --lua-desync=fake").unwrap_err()
    );
    assert!(err.contains("missing.lua"), "{err}");
}

#[test]
fn a_backslash_before_a_quote_is_refused() {
    let dir = tempfile::tempdir().unwrap();
    let engine = engine(dir.path());
    let err = format!(
        "{:#}",
        check(
            &engine,
            r#"--hostlist="E:\Zapret\lists\" --lua-desync=fake"#
        )
        .unwrap_err()
    );
    assert!(err.contains(r#"\""#), "{err}");
}

#[test]
fn the_card_shows_the_global_part_and_each_profile() {
    let (global, profiles) = split(
        "--wf-tcp-out=443 --lua-init=@\"a b.lua\" --blob=x:0x00 --filter-tcp=443 --lua-desync=fake \
         --new --filter-udp=443 --lua-desync=fake:blob=x --new=last --filter-l7=stun --new",
    );
    assert_eq!(
        global,
        "--wf-tcp-out=443 --lua-init=@\"a b.lua\" --blob=x:0x00"
    );
    assert_eq!(
        profiles,
        [
            "--filter-tcp=443 --lua-desync=fake",
            "--filter-udp=443 --lua-desync=fake:blob=x",
            "--filter-l7=stun"
        ]
    );
}

/// The built-in strategies against zapret2's function list and Flowseal 1.10.3's files.
#[test]
fn every_built_in_strategy_passes_the_static_check_and_names_files_that_exist() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("zapret2");
    common::fake_engine(&root, "", &common::lib_with_functions());
    let engine = Engine::open(&root).unwrap();
    let flowseal_files: Vec<&str> = include_str!("fixtures/flowseal-1.10.3.txt")
        .lines()
        .filter(|line| !line.starts_with('#'))
        .collect();
    let folder = common::fake_flowseal(&dir.path().join("zapret"), &flowseal_files);
    let data = tempfile::tempdir().unwrap();
    let builtin: Vec<_> = strategies::list(data.path())
        .unwrap()
        .into_iter()
        .filter(|s| s.builtin)
        .collect();
    assert!((1..=3).contains(&builtin.len()));
    for strategy in builtin {
        let tail = tail(&engine, Some(&folder), &strategy.text).unwrap();
        check(&engine, &tail).unwrap_or_else(|err| panic!("{}: {err:#}", strategy.name));
        let paths: Vec<&str> = tail
            .split('"')
            .skip(1)
            .step_by(2)
            .filter(|quoted| quoted.contains(":\\"))
            .collect();
        assert!(paths.len() > 10, "{}: {paths:?}", strategy.name);
        for path in paths {
            assert!(
                Path::new(path).exists(),
                "{} names {path}, which is not there",
                strategy.name
            );
        }
        assert!(
            strategy.text.contains("bol-van") && strategy.text.contains("MIT"),
            "{}: the source and its license are named",
            strategy.name
        );
    }
}
```

Что проверяют тесты:
- порядок: встроенные, затем свои в естественном порядке (`b 9` раньше `b 10`); файл с именем встроенной (`General.txt`) и не-`.txt` пропускаются; BOM в начале файла снимается;
- `claim`: встроенное имя занято навсегда, `create` требует нового имени, правка — существующего; имя проходит ту же проверку, что имена своих стратегий Flowseal;
- `delete`: ни встроенную, ни активную (сравнение без учёта регистра);
- комментарий — только целая строка с `#` в начале (после пробелов); `#var` внутри строки остаётся; CRLF допустим;
- `tail`: все семь подстановок, `--chdir` первым; папки с пробелами остаются одним аргументом (`quoted_tokens` даёт 8 частей); игровой фильтр берётся из папки Flowseal, выключенный — `12`, как у Flowseal (core 5.5);
- без папки Flowseal `%LISTS%` (в любом регистре) — ошибка с именем подстановки, `%LUA%` работает;
- статическая проверка ищет `function <имя>(` в `lua\*.lua`, в `--lua-init=@<файл>` (путь от корня или абсолютный) и во встроенном коде `--lua-init`; недостающие функции перечислены по одному разу, по алфавиту; отсутствующий файл `--lua-init` — ошибка с его именем;
- `\"` в строке — ошибка;
- карточка: глобальная часть (`--wf-*`, `--lua-init`, `--blob`) и профили между `--new` (и `--new=<…>`);
- каждая встроенная стратегия проходит статическую проверку по `functions.txt`, каждый путь в кавычках существует в папке Flowseal 1.10.3 или zapret2, и в тексте названы источник и лицензия.

- [ ] **Шаг 4: Убедиться, что тесты падают**

Run: `cargo test -p zapret2 --test strategies`
Expected: FAIL при компиляции: `unresolved import zapret2::strategies` (E0432).

- [ ] **Шаг 5: Встроенные стратегии**

Обе собраны из примера для Windows в `docs/readme.md` zapret2 (`a1adf7b`): тот же набор профилей и техник, но на списках и фейках Flowseal. Где пример берёт свой список YouTube — `list-google.txt`; где общий список — `list-general.txt` с `list-general-user.txt` и исключениями; режимы ipset Flowseal работают через профили с `ipset-all.txt`. Голос Discord пример ловит фильтрами WinDivert из своих файлов, которых у Flowseal нет, поэтому здесь он ловится по портам, как у Flowseal. Имена функций (`fake`, `multidisorder`, `fakedsplit`) и их аргументов (`blob`, `tcp_md5`, `tcp_seq`, `repeats`, `tls_mod=rnd,dupsid,sni=…`, `pos=1,midsld`, `ip_autottl`, `ip6_autottl`) сверены с `lua/zapret-antidpi.lua` и `lua/zapret-lib.lua`; блобы `fake_default_tls`, `fake_default_http`, `fake_default_quic`, протоколы `--filter-l7` и типы `--payload` — с `docs/manual.md` и `nfq2/` того же коммита. Живьём стратегии не проверены (спецификация 5.2): их место в рейтинге покажет тест.

`crates/zapret2/src/builtin/general.txt`:

```text
# Built into Zapret Hub, read-only: clone it to change it.
# From the Windows example in zapret2's docs/readme.md (https://github.com/bol-van/zapret2,
# MIT License, Copyright (c) 2016-2026 bol-van), on Flowseal's lists and fakes: list-google.txt
# where the example has its YouTube list, the general lists and ipset-all.txt for the rest.
# Discord voice is caught by its ports, as Flowseal does: Flowseal has no WinDivert filter files.
--wf-tcp-out=80,443
--wf-udp-out=443,19294-19344,50000-50100
--lua-init=@"%LUA%zapret-lib.lua"
--lua-init=@"%LUA%zapret-antidpi.lua"
--lua-init="fake_default_tls = tls_mod(fake_default_tls,'rnd,rndsni')"
--blob=quic_google:@"%BIN%quic_initial_www_google_com.bin"

# HTTP
--filter-tcp=80 --filter-l7=http
--hostlist="%LISTS%list-general.txt" --hostlist="%LISTS%list-general-user.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--out-range=-d10 --payload=http_req
--lua-desync=fake:blob=fake_default_http:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5
--lua-desync=fakedsplit:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5
--new

# YouTube over TLS
--filter-tcp=443 --filter-l7=tls --hostlist="%LISTS%list-google.txt"
--out-range=-d10 --payload=tls_client_hello
--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=11:tls_mod=rnd,dupsid,sni=www.google.com
--lua-desync=multidisorder:pos=1,midsld
--new

# The general lists over TLS
--filter-tcp=443 --filter-l7=tls
--hostlist="%LISTS%list-general.txt" --hostlist="%LISTS%list-general-user.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--out-range=-d10 --payload=tls_client_hello
--lua-desync=fake:blob=fake_default_tls:tcp_md5:tcp_seq=-10000:repeats=6
--lua-desync=multidisorder:pos=midsld
--new

# YouTube over QUIC
--filter-udp=443 --filter-l7=quic --hostlist="%LISTS%list-google.txt"
--payload=quic_initial
--lua-desync=fake:blob=quic_google:repeats=11
--new

# The general lists over QUIC
--filter-udp=443 --filter-l7=quic
--hostlist="%LISTS%list-general.txt" --hostlist="%LISTS%list-general-user.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--payload=quic_initial
--lua-desync=fake:blob=fake_default_quic:repeats=11
--new

# Discord voice
--filter-udp=19294-19344,50000-50100 --filter-l7=wireguard,stun,discord
--payload=wireguard_initiation,wireguard_cookie,stun,discord_ip_discovery
--lua-desync=fake:blob=0x00000000000000000000000000000000:repeats=2
--new

# Addresses in ipset-all.txt: the ipset modes, and every address during the DPI tests
--filter-tcp=80 --filter-l7=http --ipset="%LISTS%ipset-all.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--ipset-exclude="%LISTS%ipset-exclude.txt" --ipset-exclude="%LISTS%ipset-exclude-user.txt"
--out-range=-d10 --payload=http_req
--lua-desync=fake:blob=fake_default_http:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5
--lua-desync=fakedsplit:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5
--new
--filter-tcp=443 --filter-l7=tls --ipset="%LISTS%ipset-all.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--ipset-exclude="%LISTS%ipset-exclude.txt" --ipset-exclude="%LISTS%ipset-exclude-user.txt"
--out-range=-d10 --payload=tls_client_hello
--lua-desync=fake:blob=fake_default_tls:tcp_md5:tcp_seq=-10000:repeats=6
--lua-desync=multidisorder:pos=midsld
--new
--filter-udp=443 --filter-l7=quic --ipset="%LISTS%ipset-all.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--ipset-exclude="%LISTS%ipset-exclude.txt" --ipset-exclude="%LISTS%ipset-exclude-user.txt"
--payload=quic_initial
--lua-desync=fake:blob=fake_default_quic:repeats=11
```

`crates/zapret2/src/builtin/general-sni.txt` — тот же текст с другой шапкой и техникой YouTube в обоих профилях TLS по общим спискам:

```text
# Built into Zapret Hub, read-only: clone it to change it.
# "general" with the example's YouTube technique for every TLS profile: a fake ClientHello with
# the SNI www.google.com and the session id of the real one, then multidisorder at 1 and midsld.
# From the Windows example in zapret2's docs/readme.md (https://github.com/bol-van/zapret2,
# MIT License, Copyright (c) 2016-2026 bol-van), on Flowseal's lists and fakes.
--wf-tcp-out=80,443
--wf-udp-out=443,19294-19344,50000-50100
--lua-init=@"%LUA%zapret-lib.lua"
--lua-init=@"%LUA%zapret-antidpi.lua"
--lua-init="fake_default_tls = tls_mod(fake_default_tls,'rnd,rndsni')"
--blob=quic_google:@"%BIN%quic_initial_www_google_com.bin"

# HTTP
--filter-tcp=80 --filter-l7=http
--hostlist="%LISTS%list-general.txt" --hostlist="%LISTS%list-general-user.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--out-range=-d10 --payload=http_req
--lua-desync=fake:blob=fake_default_http:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5
--lua-desync=fakedsplit:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5
--new

# YouTube over TLS
--filter-tcp=443 --filter-l7=tls --hostlist="%LISTS%list-google.txt"
--out-range=-d10 --payload=tls_client_hello
--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=11:tls_mod=rnd,dupsid,sni=www.google.com
--lua-desync=multidisorder:pos=1,midsld
--new

# The general lists over TLS
--filter-tcp=443 --filter-l7=tls
--hostlist="%LISTS%list-general.txt" --hostlist="%LISTS%list-general-user.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--out-range=-d10 --payload=tls_client_hello
--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=11:tls_mod=rnd,dupsid,sni=www.google.com
--lua-desync=multidisorder:pos=1,midsld
--new

# YouTube over QUIC
--filter-udp=443 --filter-l7=quic --hostlist="%LISTS%list-google.txt"
--payload=quic_initial
--lua-desync=fake:blob=quic_google:repeats=11
--new

# The general lists over QUIC
--filter-udp=443 --filter-l7=quic
--hostlist="%LISTS%list-general.txt" --hostlist="%LISTS%list-general-user.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--payload=quic_initial
--lua-desync=fake:blob=fake_default_quic:repeats=11
--new

# Discord voice
--filter-udp=19294-19344,50000-50100 --filter-l7=wireguard,stun,discord
--payload=wireguard_initiation,wireguard_cookie,stun,discord_ip_discovery
--lua-desync=fake:blob=0x00000000000000000000000000000000:repeats=2
--new

# Addresses in ipset-all.txt: the ipset modes, and every address during the DPI tests
--filter-tcp=80 --filter-l7=http --ipset="%LISTS%ipset-all.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--ipset-exclude="%LISTS%ipset-exclude.txt" --ipset-exclude="%LISTS%ipset-exclude-user.txt"
--out-range=-d10 --payload=http_req
--lua-desync=fake:blob=fake_default_http:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5
--lua-desync=fakedsplit:ip_autottl=-2,3-20:ip6_autottl=-2,3-20:tcp_md5
--new
--filter-tcp=443 --filter-l7=tls --ipset="%LISTS%ipset-all.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--ipset-exclude="%LISTS%ipset-exclude.txt" --ipset-exclude="%LISTS%ipset-exclude-user.txt"
--out-range=-d10 --payload=tls_client_hello
--lua-desync=fake:blob=fake_default_tls:tcp_md5:repeats=11:tls_mod=rnd,dupsid,sni=www.google.com
--lua-desync=multidisorder:pos=1,midsld
--new
--filter-udp=443 --filter-l7=quic --ipset="%LISTS%ipset-all.txt"
--hostlist-exclude="%LISTS%list-exclude.txt" --hostlist-exclude="%LISTS%list-exclude-user.txt"
--ipset-exclude="%LISTS%ipset-exclude.txt" --ipset-exclude="%LISTS%ipset-exclude-user.txt"
--payload=quic_initial
--lua-desync=fake:blob=fake_default_quic:repeats=11
```

Пути в обеих — подстановки в кавычках, поэтому пробелы в путях безопасны, а `\"` не появляется: каждая подстановка кончается на `\`, и после неё всегда идёт имя файла.

- [ ] **Шаг 6: Модуль `strategies`**

`crates/zapret2/src/lib.rs`, первые две строки (doc-комментарий)

```rust
//! bol-van/zapret2 (`winws2.exe`, strategies in Lua) as Zapret Hub's second engine (zapret2
//! spec): its folder and version.
```

заменить на

```rust
//! bol-van/zapret2 (`winws2.exe`, strategies in Lua) as Zapret Hub's second engine (zapret2
//! spec): its folder and version here, its strategies in `strategies`.

pub mod strategies;
```

`crates/zapret2/src/strategies.rs`:

```rust
//! zapret2 strategies (zapret2 spec 5): the built-in texts and the user's `data/zapret2/<name>.txt`,
//! the command line a text becomes, and the static part of the check.

use std::path::{Path, PathBuf};

use anyhow::Context as _;
use zapret::batparse::replace_ci;
use zapret::custom::quoted_tokens;
use zapret::folder::Folder;

use crate::{Engine, PREFIX};

/// Read-only. Not checked on live traffic: the tests rank them (spec 5.2).
const BUILTIN: [(&str, &str); 2] = [
    ("general", include_str!("builtin/general.txt")),
    ("general-sni", include_str!("builtin/general-sni.txt")),
];

/// The placeholders that come from the Flowseal folder.
const FLOWSEAL_VARS: [&str; 5] = [
    "%LISTS%",
    "%BIN%",
    "%GameFilter%",
    "%GameFilterTCP%",
    "%GameFilterUDP%",
];

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Strategy {
    /// Without the prefix: `general`.
    pub name: String,
    /// As written: comment lines and placeholders kept.
    pub text: String,
    pub builtin: bool,
}

impl Strategy {
    /// `zapret2:<name>`: what lists, tests, the registry and the CLI call it.
    pub fn full_name(&self) -> String {
        format!("{PREFIX}{}", self.name)
    }
}

/// `data/zapret2`
pub fn dir(data_dir: &Path) -> PathBuf {
    data_dir.join("zapret2")
}

/// The built-in ones, then the user's in natural order. A file named like a built-in one is left
/// out: the built-in one would hide it anyway.
pub fn list(data_dir: &Path) -> anyhow::Result<Vec<Strategy>> {
    let dir = dir(data_dir);
    let mut own = Vec::new();
    let entries = match std::fs::read_dir(&dir) {
        Ok(entries) => entries,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(builtin().collect()),
        Err(err) => return Err(anyhow::Error::new(err).context(dir.display().to_string())),
    };
    for entry in entries {
        let path = entry?.path();
        let txt = path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("txt"));
        let Some(name) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        if !txt || !path.is_file() {
            continue;
        }
        if is_builtin(name) {
            log::warn!(
                "{} is named like a built-in strategy: skipped",
                path.display()
            );
            continue;
        }
        let text = zapret::read_text(&path).with_context(|| path.display().to_string())?;
        own.push(Strategy {
            name: name.to_owned(),
            text: text.trim_start_matches('\u{feff}').to_owned(),
            builtin: false,
        });
    }
    own.sort_by_cached_key(|s| zapret::strategies::natural_key(&s.name));
    Ok(builtin().chain(own).collect())
}

fn builtin() -> impl Iterator<Item = Strategy> {
    BUILTIN.into_iter().map(|(name, text)| Strategy {
        name: name.to_owned(),
        text: text.to_owned(),
        builtin: true,
    })
}

fn is_builtin(name: &str) -> bool {
    BUILTIN
        .iter()
        .any(|(builtin, _)| builtin.eq_ignore_ascii_case(name))
}

/// `name` without the prefix.
pub fn find(data_dir: &Path, name: &str) -> anyhow::Result<Strategy> {
    list(data_dir)?
        .into_iter()
        .find(|s| s.name.eq_ignore_ascii_case(name))
        .ok_or_else(|| anyhow::anyhow!("no zapret2 strategy named {name}"))
}

/// The name a save writes under (spec 5.2): a valid one, not a built-in one; new for `create`,
/// one of the user's otherwise.
pub fn claim(data_dir: &Path, name: &str, create: bool) -> anyhow::Result<String> {
    let name = zapret::custom::validate_name(name)?;
    anyhow::ensure!(
        !is_builtin(&name),
        "{PREFIX}{name} is built in: save a copy under another name"
    );
    let exists = list(data_dir)?
        .iter()
        .any(|s| s.name.eq_ignore_ascii_case(&name));
    if create {
        anyhow::ensure!(!exists, "the name {name} is taken");
    } else {
        anyhow::ensure!(exists, "no zapret2 strategy of yours named {name}");
    }
    Ok(name)
}

/// `data/zapret2/<name>.txt`, the text as it came.
pub fn write(data_dir: &Path, name: &str, text: &str) -> anyhow::Result<()> {
    let dir = dir(data_dir);
    std::fs::create_dir_all(&dir).with_context(|| dir.display().to_string())?;
    let path = dir.join(format!("{name}.txt"));
    std::fs::write(&path, text).with_context(|| path.display().to_string())
}

/// The user's own, and not `active` (the registry's strategy name).
pub fn delete(data_dir: &Path, name: &str, active: Option<&str>) -> anyhow::Result<()> {
    let strategy = find(data_dir, name)?;
    let full = strategy.full_name();
    anyhow::ensure!(!strategy.builtin, "{full} is built in: it stays");
    anyhow::ensure!(
        !active.is_some_and(|active| active.eq_ignore_ascii_case(&full)),
        "{full} is the active strategy: apply another one first"
    );
    let path = dir(data_dir).join(format!("{}.txt", strategy.name));
    std::fs::remove_file(&path).with_context(|| path.display().to_string())
}

/// The text as `winws2` arguments: lines that start with `#` dropped, the rest joined by spaces. A
/// `#` inside a line is zapret2's own (`#var`) and stays (spec 5.1).
pub fn args(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" ")
}

/// The one command line of a text, for the test, the check and the service alike (spec 5.1):
/// `--chdir="<root>"`, then the arguments with the placeholders filled in. Every path placeholder
/// ends in `\`. `%LISTS%`, `%BIN%` and the game filter come from the Flowseal folder: without one
/// they are an error.
pub fn tail(engine: &Engine, folder: Option<&Folder>, text: &str) -> anyhow::Result<String> {
    let dir = |path: PathBuf| format!("{}\\", path.display());
    let mut args = args(text);
    for (var, value) in [
        ("%LUA%", dir(engine.lua())),
        ("%FILES%", dir(engine.root().join("files"))),
    ] {
        args = replace_ci(&args, var, &value);
    }
    match folder {
        Some(folder) => {
            let filter = zapret::game_filter::try_read(folder)?.vars();
            for (var, value) in [
                ("%LISTS%", dir(folder.lists())),
                ("%BIN%", dir(folder.bin())),
                ("%GameFilterTCP%", filter.tcp),
                ("%GameFilterUDP%", filter.udp),
                ("%GameFilter%", filter.all),
            ] {
                args = replace_ci(&args, var, &value);
            }
        }
        None => {
            let lower = args.to_ascii_lowercase();
            if let Some(var) = FLOWSEAL_VARS
                .iter()
                .find(|var| lower.contains(&var.to_ascii_lowercase()))
            {
                anyhow::bail!("the strategy uses {var}: choose the zapret folder in Settings");
            }
        }
    }
    Ok(format!("--chdir=\"{}\" {args}", engine.root().display()))
}

/// What `--dry-run` cannot see (spec 5.3): every `--lua-desync` function defined as
/// `function <name>(` in `lua\*.lua`, in `--lua-init` code or in a `--lua-init=@<file>`; and no
/// `\"`, which a placeholder right before a quote makes.
pub fn check(engine: &Engine, tail: &str) -> anyhow::Result<()> {
    anyhow::ensure!(
        !tail.contains("\\\""),
        "the command line has \\\" (a path ending in \\ right before a quote), which winws2 reads as a quote inside the value: {tail}"
    );
    let lua = engine.lua();
    let mut sources = Vec::new();
    for entry in std::fs::read_dir(&lua).with_context(|| lua.display().to_string())? {
        let path = entry?.path();
        if path.extension().is_some_and(|ext| ext == "lua") {
            sources.push(zapret::read_text(&path).with_context(|| path.display().to_string())?);
        }
    }
    let tokens = quoted_tokens(tail);
    for init in tokens.iter().filter_map(|t| t.strip_prefix("--lua-init=")) {
        match init.strip_prefix('@') {
            Some(file) => {
                let path = engine.root().join(file.trim_matches('"'));
                sources.push(
                    zapret::read_text(&path)
                        .with_context(|| format!("--lua-init file {}", path.display()))?,
                );
            }
            None => sources.push(init.trim_matches('"').to_owned()),
        }
    }
    let mut missing: Vec<&str> = tokens
        .iter()
        .filter_map(|t| t.strip_prefix("--lua-desync="))
        .map(|call| call.trim_matches('"').split(':').next().unwrap_or_default())
        .filter(|function| {
            let definition = format!("function {function}(");
            !sources.iter().any(|source| source.contains(&definition))
        })
        .collect();
    missing.sort_unstable();
    missing.dedup();
    anyhow::ensure!(
        missing.is_empty(),
        "--lua-desync calls {}, which no lua\\*.lua file and no --lua-init defines",
        missing.join(", ")
    );
    Ok(())
}

/// The global part (`--wf-*`, `--lua-init`, `--blob`) and the profiles between `--new`s, for the
/// card (spec 8).
pub fn split(args: &str) -> (String, Vec<String>) {
    let tokens = quoted_tokens(args);
    let global = |token: &String| {
        ["--wf-", "--lua-init", "--blob"]
            .iter()
            .any(|prefix| token.starts_with(prefix))
    };
    let profiles = tokens
        .split(|token| token == "--new" || token.starts_with("--new="))
        .map(|profile| {
            profile
                .iter()
                .filter(|token| !global(token))
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .filter(|profile| !profile.is_empty())
        .collect();
    let global = tokens
        .iter()
        .filter(|token| global(token))
        .cloned()
        .collect::<Vec<_>>()
        .join(" ");
    (global, profiles)
}
```

Пояснения:
- `list` пропускает свой файл с именем встроенной (с предупреждением в журнале), а `claim` такое имя не даёт: встроенная его бы спрятала.
- `tail` разворачивает `%LUA%` и `%FILES%` всегда, а подстановки Flowseal — только с папкой; без неё первая найденная подстановка Flowseal — ошибка с подсказкой.
- `check` ищет определения так же, как их найдёт winws2: в каждом `lua\*.lua`, в файлах `--lua-init=@…` (относительный путь — от корня, куда `--chdir` переводит winws2) и во встроенном коде. Шаблон `function <имя>(` совпадает с каждым определением функций в Lua zapret2.
- `split` не трогает подстановки: карточка показывает текст, как он написан.

- [ ] **Шаг 7: Тесты проходят**

Run: `cargo test -p zapret2`
Expected: PASS: `engine` — 8 и 1 `ignored`, `strategies` — 11.

Run: `cargo test -p zapret`
Expected: PASS.

- [ ] **Шаг 8: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 9: Коммит**

```bash
git add crates/zapret/src/batparse.rs crates/zapret/src/custom.rs crates/zapret2/src/lib.rs crates/zapret2/src/strategies.rs crates/zapret2/src/builtin/general.txt crates/zapret2/src/builtin/general-sni.txt crates/zapret2/tests/fixtures/functions.txt crates/zapret2/tests/fixtures/flowseal-1.10.3.txt crates/zapret2/tests/common/mod.rs crates/zapret2/tests/strategies.rs
git commit -m "feat(zapret2): add built-in and own strategies with their static check"
```

---

### Задача 5: Проверка winws2 и служба `zapret2`

**Files:**
- Modify: `crates/zapret2/src/lib.rs` (`pub mod service;`)
- Create: `crates/zapret2/src/service.rs`
- Test: `crates/zapret2/tests/service.rs`

**Interfaces:**
- Consumes: `zapret2::Engine`, `zapret2::PREFIX` (задача 3); `zapret2::strategies::{Strategy, find, claim, tail, check, write}` (задача 4); `zapret::service::{SERVICE, SERVICE2, stop_and_delete, create, start, set_strategy_of, strategy_of, state, enable_tcp_timestamps, quick_check, QuickCheck, SvcState}` (задача 1), `zapret::procs::kill_all`, `exec::run`.
- Produces (модуль `zapret2::service`):
  - `pub fn dry_run_args(tail: &str) -> String` — `<tail> --wf-dup-check=0 --dry-run`; `pub fn intercept_args(tail: &str) -> String` — `<tail> --intercept=0`; `pub fn bin_path(engine: &Engine, tail: &str) -> String` — `"<winws2.exe>" <tail>`;
  - `pub fn validate(engine: &Engine, tail: &str) -> anyhow::Result<()>` — полная проверка (5.3), запускает winws2;
  - `pub fn install(engine: &Engine, strategy: &Strategy, tail: &str) -> anyhow::Result<()>` — шаги 2–5 применения (6), без проверки;
  - `pub fn apply(engine: &Engine, folder: &Folder, data_dir: &Path, name: &str) -> anyhow::Result<QuickCheck>` — `name` без префикса; вызывается внутри `change_safely("apply")`;
  - `pub fn reinstall_active(engine: Option<&Engine>, folder: &Folder, data_dir: &Path) -> anyhow::Result<()>` — после смены игрового фильтра; без установленной `zapret2` ничего не делает;
  - `pub fn save(engine: &Engine, folder: Option<&Folder>, data_dir: &Path, name: &str, text: &str, create: bool) -> anyhow::Result<(Strategy, String)>` — имя, полная проверка, файл; возвращает сохранённую стратегию и её командную строку.

- [ ] **Шаг 1: Падающие тесты**

`crates/zapret2/tests/service.rs`:

```rust
mod common;

use std::fs;

use zapret::service::{SERVICE, SERVICE2, active, state, strategy};
use zapret2::Engine;
use zapret2::service::{bin_path, dry_run_args, intercept_args, save, validate};
use zapret2::strategies;

#[test]
fn winws2_gets_the_strategy_first_and_its_own_flags_last() {
    let tail = r#"--chdir="D:\zapret2" --wf-tcp-out=443 --lua-desync=fake:blob=fake_default_tls"#;
    assert_eq!(
        dry_run_args(tail),
        format!("{tail} --wf-dup-check=0 --dry-run")
    );
    assert_eq!(intercept_args(tail), format!("{tail} --intercept=0"));
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "bin", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    assert_eq!(
        bin_path(&engine, tail),
        format!("\"{}\\bin\\winws2.exe\" {tail}", dir.path().display())
    );
}

#[test]
fn the_static_check_comes_before_winws2_runs() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(dir.path(), "", common::LIB);
    let engine = Engine::open(dir.path()).unwrap();
    // winws2.exe is an empty file here: had it run, the error would be about starting it.
    let err = format!(
        "{:#}",
        validate(&engine, "--lua-desync=nosuch").unwrap_err()
    );
    assert!(err.contains("nosuch"), "{err}");
}

#[test]
fn a_save_writes_nothing_until_the_check_passes() {
    let dir = tempfile::tempdir().unwrap();
    common::fake_engine(
        dir.path(),
        "",
        "NFQWS2_COMPAT_VER_REQUIRED=6\nfunction fake(ctx, desync)\nend\n",
    );
    let engine = Engine::open(dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let file = strategies::dir(data.path()).join("mine.txt");
    let text = "--wf-tcp-out=443\n--lua-desync=fake:blob=fake_default_tls\n";
    // The static check passes; winws2 (an empty file) cannot run, so nothing is saved.
    let err = format!(
        "{:#}",
        save(&engine, None, data.path(), "mine", text, true).unwrap_err()
    );
    assert!(err.contains("os error 193"), "{err}");
    assert!(!file.exists());
    let err = format!(
        "{:#}",
        save(
            &engine,
            None,
            data.path(),
            "mine",
            "--hostlist=\"%LISTS%a.txt\"",
            true
        )
        .unwrap_err()
    );
    assert!(err.contains("%LISTS%"), "{err}");
    let err = format!(
        "{:#}",
        save(&engine, None, data.path(), "mine", text, false).unwrap_err()
    );
    assert!(err.contains("no zapret2 strategy of yours"), "{err}");
    assert!(!file.exists());
}

fn live() -> (Engine, zapret::folder::Folder, tempfile::TempDir) {
    let dir = std::env::var("ZAPRET2_DIR").expect("ZAPRET2_DIR is the zapret2 folder");
    let engine = Engine::open(dir).unwrap();
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    (engine, folder, tempfile::tempdir().unwrap())
}

#[test]
#[ignore = "needs admin rights, a zapret2 release in ZAPRET2_DIR and E:\\Zapret; loads WinDivert"]
fn live_every_built_in_strategy_passes_the_whole_check() {
    let (engine, folder, data) = live();
    for s in strategies::list(data.path()).unwrap() {
        let tail = strategies::tail(&engine, Some(&folder), &s.text).unwrap();
        validate(&engine, &tail).unwrap_or_else(|err| panic!("{}: {err:#}", s.name));
    }
}

#[test]
#[ignore = "needs admin rights, a zapret2 release in ZAPRET2_DIR and E:\\Zapret; replaces the zapret and zapret2 services"]
fn live_apply_zapret2_then_flowseal_back() {
    let (engine, folder, data) = live();
    let check = zapret2::service::apply(&engine, &folder, data.path(), "general").unwrap();
    eprintln!("zapret2: {check:?}");
    assert_eq!(active(), (SERVICE2, zapret::service::WINWS2));
    assert_eq!(strategy().as_deref(), Some("zapret2:general"));
    assert_eq!(state(SERVICE), zapret::service::SvcState::NotInstalled);
    let check = zapret::service::apply(&folder, "general").unwrap();
    eprintln!("flowseal: {check:?}");
    assert_eq!(active(), (SERVICE, "winws.exe"));
    assert_eq!(state(SERVICE2), zapret::service::SvcState::NotInstalled);
}

#[test]
#[ignore = "needs admin rights, a zapret2 release in ZAPRET2_DIR and E:\\Zapret; reinstalls zapret2"]
fn live_a_game_filter_change_reinstalls_zapret2() {
    let (engine, folder, data) = live();
    strategies::write(
        data.path(),
        "game",
        &format!(
            "{}\n--new --filter-tcp=%GameFilterTCP% --lua-desync=fake:blob=fake_default_tls",
            strategies::find(data.path(), "general").unwrap().text
        ),
    )
    .unwrap();
    zapret2::service::apply(&engine, &folder, data.path(), "game").unwrap();
    zapret2::service::reinstall_active(Some(&engine), &folder, data.path()).unwrap();
    let image = zapret::service::config(SERVICE2).unwrap().image_path;
    let filter = zapret::game_filter::read(&folder).vars();
    assert!(
        image.contains(&format!("--filter-tcp={}", filter.tcp)),
        "{image}"
    );
    fs::remove_dir_all(strategies::dir(data.path())).unwrap();
}
```

Без прав проверяются строки запуска и порядок «сначала статическая проверка, потом winws2, потом файл»: `winws2.exe` в поддельной папке пустой, и его запуск даёт os error 193 (не программа Win32). Если ошибка про `nosuch` или `%LISTS%`, значит winws2 до запуска не дошёл; если os error 193 — дошёл, а файла нет. Три `live_…` теста пользователь запускает из повышенного терминала с распакованным релизом: `$env:ZAPRET2_DIR='D:\zapret2'; cargo test -p zapret2 --test service -- --ignored --test-threads=1`.

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret2 --test service`
Expected: FAIL при компиляции: `unresolved import zapret2::service` (E0432), `cannot find service in zapret2` (E0433).

- [ ] **Шаг 3: Проверка и служба**

`crates/zapret2/src/lib.rs`, doc-комментарий и строку модуля

```rust
//! bol-van/zapret2 (`winws2.exe`, strategies in Lua) as Zapret Hub's second engine (zapret2
//! spec): its folder and version here, its strategies in `strategies`.

pub mod strategies;
```

заменить на

```rust
//! bol-van/zapret2 (`winws2.exe`, strategies in Lua) as Zapret Hub's second engine (zapret2
//! spec): its folder and version here, its strategies in `strategies`, the check that runs winws2
//! and the `zapret2` service in `service`.

pub mod service;
pub mod strategies;
```

`crates/zapret2/src/service.rs`:

```rust
//! The check that runs winws2 and the `zapret2` service (zapret2 spec 5.3, 6). `zapret::service`
//! does the `sc` work; this module knows what zapret2 needs from it.

use std::os::windows::process::CommandExt as _;
use std::path::Path;
use std::process::Command;
use std::time::Duration;

use zapret::folder::Folder;
use zapret::service::{self, QuickCheck, SERVICE, SERVICE2, SvcState};

use crate::strategies::{self, Strategy};
use crate::{Engine, PREFIX};

/// `--dry-run` goes last: before an option it does not know, it ends with code 1 and says
/// nothing. `--wf-dup-check=0` lets it pass next to a running winws2 with the same filter.
pub fn dry_run_args(tail: &str) -> String {
    format!("{tail} --wf-dup-check=0 --dry-run")
}

/// Lua starts for real behind an empty WinDivert filter: traffic is not touched.
pub fn intercept_args(tail: &str) -> String {
    format!("{tail} --intercept=0")
}

/// The service's `binPath`: `"<winws2.exe>" <tail>`.
pub fn bin_path(engine: &Engine, tail: &str) -> String {
    format!("\"{}\" {tail}", engine.exe().display())
}

/// The whole check (spec 5.3), to the first failure: the static one, `--dry-run`,
/// `--intercept=0`. Runs winws2, so it needs admin rights. An error carries winws2's whole output.
pub fn validate(engine: &Engine, tail: &str) -> anyhow::Result<()> {
    strategies::check(engine, tail)?;
    let dry = run(engine, &dry_run_args(tail))?;
    let verified = [&dry.stdout, &dry.stderr]
        .iter()
        .any(|text| text.contains("command line parameters verified"));
    anyhow::ensure!(
        dry.success() && verified,
        "winws2 --dry-run did not accept the strategy:\n{}",
        dry.describe()
    );
    let init = run(engine, &intercept_args(tail))?;
    anyhow::ensure!(
        init.success(),
        "winws2 --intercept=0 failed{}:\n{}",
        if init.code == Some(87) {
            " to start the Lua code"
        } else {
            ""
        },
        init.describe()
    );
    Ok(())
}

fn run(engine: &Engine, args: &str) -> anyhow::Result<exec::Output> {
    let mut cmd = Command::new(engine.exe());
    cmd.raw_arg(args).current_dir(engine.root());
    Ok(exec::run(&mut cmd, Duration::from_secs(30), None)?)
}

/// Steps 2–5 of spec 6: Flowseal's service and winws gone, the previous `zapret2` gone, TCP
/// timestamps on (as Flowseal does), `zapret2` created and started, its strategy recorded.
pub fn install(engine: &Engine, strategy: &Strategy, tail: &str) -> anyhow::Result<()> {
    service::stop_and_delete(SERVICE)?;
    zapret::procs::kill_all("winws.exe")?;
    service::stop_and_delete(SERVICE2)?;
    let _ = service::enable_tcp_timestamps();
    service::create(SERVICE2, &bin_path(engine, tail), true)?;
    service::start(SERVICE2)?;
    service::set_strategy_of(SERVICE2, &strategy.full_name())
}

/// «Применить» for `zapret2:<name>`, inside `change_safely("apply")`: the check while nothing has
/// changed yet, the service, and after 2 s the quick check (spec 6).
pub fn apply(
    engine: &Engine,
    folder: &Folder,
    data_dir: &Path,
    name: &str,
) -> anyhow::Result<QuickCheck> {
    let strategy = strategies::find(data_dir, name)?;
    let tail = strategies::tail(engine, Some(folder), &strategy.text)?;
    validate(engine, &tail)?;
    install(engine, &strategy, &tail)?;
    std::thread::sleep(Duration::from_secs(2));
    Ok(service::quick_check(folder))
}

/// After a game filter change: the active zapret2 strategy installed again, its ports are in the
/// service's arguments (spec 6). Nothing to do while `zapret2` is not installed.
pub fn reinstall_active(
    engine: Option<&Engine>,
    folder: &Folder,
    data_dir: &Path,
) -> anyhow::Result<()> {
    if service::state(SERVICE2) == SvcState::NotInstalled {
        return Ok(());
    }
    let engine = engine.ok_or_else(|| {
        anyhow::anyhow!(
            "the zapret2 service is installed, but the zapret2 folder is not usable, so the service cannot be installed again with the new game filter: check the folder in Settings"
        )
    })?;
    let name = service::strategy_of(SERVICE2)
        .and_then(|name| name.strip_prefix(PREFIX).map(str::to_owned))
        .ok_or_else(|| {
            anyhow::anyhow!(
                "the zapret2 service has no zapret2 strategy name in the registry, so it cannot be installed again with the new game filter: apply a strategy first"
            )
        })?;
    let strategy = strategies::find(data_dir, &name)?;
    let tail = strategies::tail(engine, Some(folder), &strategy.text)?;
    validate(engine, &tail)?;
    install(engine, &strategy, &tail)
}

/// `save_zapret2` up to the file (spec 5.2): the name, `create` against what exists, the whole
/// check, the file. Returns what was saved and its command line, for a service that runs it.
pub fn save(
    engine: &Engine,
    folder: Option<&Folder>,
    data_dir: &Path,
    name: &str,
    text: &str,
    create: bool,
) -> anyhow::Result<(Strategy, String)> {
    let name = strategies::claim(data_dir, name, create)?;
    let tail = strategies::tail(engine, folder, text)?;
    validate(engine, &tail)?;
    strategies::write(data_dir, &name, text)?;
    let saved = Strategy {
        name,
        text: text.to_owned(),
        builtin: false,
    };
    Ok((saved, tail))
}
```

Пояснения:
- Аргументы передаются через `raw_arg` как есть: `cygwin1.dll` разбирает командную строку сам, а `Command::arg` экранировал бы кавычки внутри строки. Рабочая папка — корень zapret2, хотя `--chdir` и так её ставит: так относительные пути работают и до разбора аргументов.
- `--dry-run` стоит последним, после стратегии: встретив неизвестный ключ раньше, winws2 молча выходит с кодом 1 (спецификация 5.3). Успех — код 0 и строка `command line parameters verified` (её `DLOG_CONDUP` печатает в stdout или stderr). `--wf-dup-check=0` даёт проверить строку рядом с работающей службой `zapret2` с тем же фильтром.
- `--intercept=0` запускает Lua по-настоящему с пустым фильтром WinDivert; код 87 — ошибка Lua.
- `install` не проверяет: её вызывают после `validate` (`apply`, `reinstall_active`) или после `save`. Временные метки TCP включаются, как у Flowseal; их ошибку `install` не замечает, так же как `zapret::service::install`.
- `reinstall_active` без пригодной папки zapret2 при установленной службе — ошибка: переустановить службу с новым игровым фильтром нечем, и `change_safely` вернёт всё как было.

- [ ] **Шаг 4: Тесты проходят**

Run: `cargo test -p zapret2`
Expected: PASS: `engine` — 8 и 1 `ignored`, `service` — 3 и 3 `ignored`, `strategies` — 11.

- [ ] **Шаг 5: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 6: Коммит**

```bash
git add crates/zapret2/src/lib.rs crates/zapret2/src/service.rs crates/zapret2/tests/service.rs
git commit -m "feat(zapret2): validate with winws2 and install the zapret2 service"
```

---

### Задача 6: Тесты и список стратегий с zapret2

**Files:**
- Modify: `crates/tester/Cargo.toml`, `crates/tester/src/model.rs`, `crates/tester/src/catalog.rs`, `crates/tester/src/run.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/jobs.rs`, `frontend/src/lib/ipc/bindings.ts` (генерируется), `Cargo.lock` (обновит cargo)
- Test: `crates/tester/tests/catalog.rs`, `crates/tester/tests/history.rs`, `crates/tester/tests/rank.rs`, `crates/tester/tests/run.rs`, `src-tauri/src/texts.rs` (модуль `tests`), `crates/cli/src/main.rs` (модуль `tests`)

**Interfaces:**
- Consumes: `zapret2::{Engine, PREFIX}`, `Engine::{exe, root, version}` (задача 3); `zapret2::strategies::{Strategy, list, find, tail, check, args, split, write}` (задача 4); `zapret::service::{SERVICE2, WINWS2}` (задача 1); `zapret::sha256_hex`.
- Produces:
  - `tester::model::TestRun.zapret2_version: Option<String>` (`#[serde(default)]`);
  - `tester::catalog::StrategyDetail.source: String` — текст стратегии zapret2 для редактора, у Flowseal пустой;
  - `tester::catalog::rows(folder, engine2: Option<&Engine>, store, data_dir, active)` и `tester::catalog::detail(folder, engine2, store, data_dir, active, name)` — второй параметр новый; без движка строк zapret2 нет;
  - `tester::run::run(folder, engine2: Option<&Engine>, data_dir, request, socks5, cancel, progress)` — второй параметр новый;
  - `pub fn tester::run::lua_failure(exited: Option<ExitStatus>, output: &str) -> Option<String>`.

Приложение до задачи 7 передаёт `None`: zapret2 в нём ещё не видно, а Flowseal работает как раньше.

- [ ] **Шаг 1: Падающие тесты**

`crates/tester/tests/history.rs`, в функции `run` после `flowseal_version: None,` добавить `zapret2_version: None,`; после теста `runs_are_saved_listed_and_loaded` добавить:

```rust
#[test]
fn a_run_saved_before_zapret2_still_loads() {
    let dir = tempfile::tempdir().unwrap();
    let mut old = serde_json::to_value(run(
        "2026-10-09_10-00-00",
        Suite::Standard,
        false,
        vec![result("general", "a")],
    ))
    .unwrap();
    old.as_object_mut().unwrap().remove("zapret2_version");
    std::fs::write(dir.path().join("2026-10-09_10-00-00.json"), old.to_string()).unwrap();
    let loaded = history::load(dir.path(), "2026-10-09_10-00-00").unwrap();
    assert_eq!(loaded.zapret2_version, None);
    assert_eq!(loaded.strategies[0].name, "general");
}
```

`crates/tester/tests/rank.rs`, в тесте `report_reads_like_test_zapret_ps1` после `flowseal_version: Some("1.10.3".into()),` добавить `zapret2_version: None,`; перед тестом `a_dead_network_ranks_by_menu_order` добавить:

```rust
#[test]
fn a_mixed_run_ranks_both_engines_by_the_same_rules() {
    let fast = |name| {
        strategy(
            name,
            vec![target("A", "https://a.com", [Pass, Pass, Pass], 80)],
        )
    };
    let mut results = vec![
        fast("zapret2:mine"),
        strategy(
            "general",
            vec![target("A", "https://a.com", [Pass, Pass, Error], 10)],
        ),
        StrategyResult {
            launch_error: Some("winws2 printed LUA ERROR".into()),
            ..fast("zapret2:general")
        },
        fast("general (ALT2)"),
    ];
    sort(Suite::Standard, &mut results);
    let names: Vec<&str> = results.iter().map(|r| r.name.as_str()).collect();
    // A tie keeps Flowseal first: `zapret2:` sorts after its names.
    assert_eq!(
        names,
        [
            "general (ALT2)",
            "zapret2:mine",
            "general",
            "zapret2:general"
        ]
    );
}
```

Код рейтинга не меняется: тест закрепляет, что имена с префиксом сортируются по тем же правилам (спецификация 7).

`crates/tester/tests/catalog.rs`, после функции `fake_zapret` добавить:

```rust
/// A zapret2 folder whose Lua defines only `fake`: the built-in strategies fail the static check.
fn fake_zapret2() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    for file in [
        "winws2.exe",
        "cygwin1.dll",
        "WinDivert.dll",
        "WinDivert64.sys",
    ] {
        std::fs::write(root.join(file), b"").unwrap();
    }
    std::fs::create_dir_all(root.join("lua")).unwrap();
    std::fs::write(
        root.join("lua").join("zapret-lib.lua"),
        "NFQWS2_COMPAT_VER_REQUIRED=6\nfunction fake(ctx, desync)\nend\n",
    )
    .unwrap();
    std::fs::write(root.join("lua").join("zapret-antidpi.lua"), "").unwrap();
    dir
}
```

в тесте `rows_carry_custom_active_errors_and_stale_results` после `flowseal_version: None,` добавить `zapret2_version: None,`; строку

```rust
    let rows = tester::catalog::rows(&folder, &store, data.path(), Some("GENERAL")).unwrap();
```

заменить на

```rust
    let rows = tester::catalog::rows(&folder, None, &store, data.path(), Some("GENERAL")).unwrap();
```

строки

```rust
    let detail =
        tester::catalog::detail(&folder, &store, data.path(), None, "general (HUB mine)").unwrap();
```

заменить на

```rust
    let detail = tester::catalog::detail(
        &folder,
        None,
        &store,
        data.path(),
        None,
        "general (HUB mine)",
    )
    .unwrap();
```

после `assert_eq!(detail.result.unwrap().args_sha256, "outdated");` добавить `assert_eq!(detail.source, "");`; в конец файла:

```rust
#[test]
fn zapret2_rows_follow_flowseals_with_their_checks_and_results() {
    let zapret_dir = fake_zapret();
    let folder = Folder::open(zapret_dir.path()).unwrap();
    let engine_dir = fake_zapret2();
    let engine = zapret2::Engine::open(engine_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let store = Store::new(data.path().join("strategies"));
    let text = "# mine\n--wf-tcp-out=443 --lua-init=@\"%LUA%zapret-lib.lua\"\n\
                --filter-tcp=443 --lua-desync=fake:blob=fake_default_tls\n--new\n--filter-udp=443\n";
    zapret2::strategies::write(data.path(), "mine", text).unwrap();
    let tail = zapret2::strategies::tail(&engine, Some(&folder), text).unwrap();
    let run = TestRun {
        id: "2026-10-11_10-00-00".into(),
        suite: Suite::Standard,
        flowseal_version: None,
        zapret2_version: Some("v1.0.5.2 (6b6c63e)".into()),
        canceled: false,
        restore_error: None,
        strategies: vec![result("zapret2:mine", &zapret::sha256_hex(tail.as_bytes()))],
    };
    history::save(&history::dir(data.path()), &run).unwrap();

    let without = tester::catalog::rows(&folder, None, &store, data.path(), None).unwrap();
    assert!(without.iter().all(|row| !row.name.starts_with("zapret2:")));
    let rows = tester::catalog::rows(
        &folder,
        Some(&engine),
        &store,
        data.path(),
        Some("ZAPRET2:MINE"),
    )
    .unwrap();
    let names: Vec<&str> = rows.iter().map(|r| r.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "general",
            "helper",
            "zapret2:general",
            "zapret2:general-sni",
            "zapret2:mine"
        ]
    );
    let builtin = &rows[2];
    assert!(!builtin.custom && !builtin.active);
    assert!(
        builtin.error.as_deref().unwrap().contains("multidisorder"),
        "{builtin:?}"
    );
    let mine = &rows[4];
    assert!(
        mine.custom && mine.active && mine.error.is_none(),
        "{mine:?}"
    );
    assert!(!mine.last.as_ref().unwrap().stale);

    let detail = tester::catalog::detail(
        &folder,
        Some(&engine),
        &store,
        data.path(),
        None,
        "zapret2:mine",
    )
    .unwrap();
    assert_eq!(
        detail.wf,
        "--wf-tcp-out=443 --lua-init=@\"%LUA%zapret-lib.lua\""
    );
    assert_eq!(
        detail.blocks,
        [
            "--filter-tcp=443 --lua-desync=fake:blob=fake_default_tls",
            "--filter-udp=443"
        ]
    );
    assert_eq!(detail.final_args, Some(tail));
    assert_eq!(detail.source, text);
    assert_eq!(detail.record, None);
    assert!(detail.result.is_some());

    zapret2::strategies::write(data.path(), "mine", &text.replace("udp=443", "udp=80")).unwrap();
    let rows = tester::catalog::rows(&folder, Some(&engine), &store, data.path(), None).unwrap();
    assert!(
        rows[4].last.as_ref().unwrap().stale,
        "an edit makes the result stale"
    );
}
```

`crates/tester/tests/run.rs`, строки импорта

```rust
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tester::model::Suite;
use tester::run::{Progress, Request, par_map, parallelism};
```

заменить на

```rust
use std::os::windows::process::ExitStatusExt as _;
use std::process::ExitStatus;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tester::model::Suite;
use tester::run::{Progress, Request, lua_failure, par_map, parallelism};
```

после теста `the_dpi_suite_falls_back_to_its_cache` добавить:

```rust
#[test]
fn winws2_that_died_or_printed_a_lua_error_did_not_run_the_strategy() {
    let ready = "windivert initialized. capture is started.\n";
    assert_eq!(lua_failure(None, ready), None);
    let died = lua_failure(
        Some(ExitStatus::from_raw(87)),
        "LUA ERROR: desync function 'fakee' does not exist\n",
    )
    .unwrap();
    assert!(died.contains("87") && died.contains("fakee"), "{died}");
    let erred = lua_failure(
        None,
        &format!("{ready}LUA ERROR: attempt to index a nil value\n"),
    )
    .unwrap();
    assert!(erred.contains("attempt to index"), "{erred}");
}

/// Refused before anything changes, so it runs here: the services are not touched.
#[test]
fn zapret2_strategies_need_a_working_engine_before_the_run_starts() {
    let folder_dir = tempfile::tempdir().unwrap();
    let root = folder_dir.path();
    std::fs::write(root.join("service.bat"), "").unwrap();
    std::fs::create_dir_all(root.join("bin")).unwrap();
    std::fs::write(root.join("bin").join("winws.exe"), b"").unwrap();
    std::fs::write(root.join("bin").join("WinDivert64.sys"), b"").unwrap();
    std::fs::create_dir_all(root.join("lists")).unwrap();
    let folder = zapret::folder::Folder::open(root).unwrap();
    let engine_dir = tempfile::tempdir().unwrap();
    for file in [
        "winws2.exe",
        "cygwin1.dll",
        "WinDivert.dll",
        "WinDivert64.sys",
    ] {
        std::fs::write(engine_dir.path().join(file), b"").unwrap();
    }
    std::fs::create_dir_all(engine_dir.path().join("lua")).unwrap();
    for file in ["zapret-lib.lua", "zapret-antidpi.lua"] {
        std::fs::write(engine_dir.path().join("lua").join(file), "").unwrap();
    }
    let engine = zapret2::Engine::open(engine_dir.path()).unwrap();
    let data = tempfile::tempdir().unwrap();
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["general".into(), "zapret2:general".into()],
    };
    let run = |engine2: Option<&zapret2::Engine>| {
        let err = tester::run::run(
            &folder,
            engine2,
            data.path(),
            &request,
            None,
            &AtomicBool::new(false),
            &|_| panic!("no strategy may start"),
        )
        .unwrap_err();
        format!("{err:#}")
    };
    assert!(run(None).contains("zapret2 folder"), "{}", run(None));
    // An empty winws2.exe cannot tell its version.
    assert!(
        run(Some(&engine)).contains("os error 193"),
        "{}",
        run(Some(&engine))
    );
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
}
```

комментарий над тестом `a_request_without_strategies_is_refused`

```rust
// `run` is called only by the ignored tests below: it stops the zapret service of this machine.
```

заменить на

```rust
// Past its refusals, `run` is called only by the ignored tests below: it stops the services here.
```

в тестах `live_run_of_two_strategies` и `live_a_panic_during_the_run_is_passed_on_after_the_restore` после `&folder,` в вызове `tester::run::run(` добавить строку `None,`; после теста `live_a_panic_during_the_run_is_passed_on_after_the_restore` добавить:

```rust
#[test]
#[ignore = "needs admin rights, E:\\Zapret, a zapret2 release in ZAPRET2_DIR and the network"]
fn live_a_run_mixes_the_engines_and_brings_the_active_one_back() {
    use zapret::service::{active, strategy};
    let folder = zapret::folder::Folder::open(r"E:\Zapret").unwrap();
    let engine = zapret2::Engine::open(std::env::var("ZAPRET2_DIR").expect("ZAPRET2_DIR")).unwrap();
    let data = tempfile::tempdir().unwrap();
    let before = (active(), strategy());
    let request = Request {
        suite: Suite::Standard,
        strategies: vec!["general".into(), "zapret2:general".into()],
    };
    let run = tester::run::run(
        &folder,
        Some(&engine),
        data.path(),
        &request,
        None,
        &AtomicBool::new(false),
        &|p| eprintln!("{p:?}"),
    )
    .unwrap();
    eprintln!("{}", tester::report::to_text(&run));
    assert_eq!(run.strategies.len(), 2);
    assert!(run.zapret2_version.is_some());
    assert!(run.restore_error.is_none(), "{:?}", run.restore_error);
    assert_eq!((active(), strategy()), before);
    assert!(!zapret::snapshot::pending_path(data.path()).exists());
}
```

в тесте `live_cancel_stops_after_the_current_strategy` строку

```rust
    let run = tester::run::run(&folder, data.path(), &request, None, &cancel, &|p| {
```

заменить на

```rust
    let run = tester::run::run(&folder, None, data.path(), &request, None, &cancel, &|p| {
```

`src-tauri/src/texts.rs`, в модуле `tests` после `flowseal_version: None,` добавить `zapret2_version: None,`.

`crates/cli/src/main.rs` (по плану CLI; сверить при выполнении), в модуле `tests` после `flowseal_version: Some("1.10.3".into()),` добавить `zapret2_version: None,`.

Что проверяют тесты:
- запуск без Lua: winws2, который вышел (код 87 — ошибка Lua) или напечатал `LUA ERROR`, — ошибка запуска с его выводом, а не стратегия с нулевым счётом выше рабочих;
- стратегия `zapret2:` без папки zapret2 или с неработающим winws2 (пустой файл: os error 193 при чтении версии) останавливает тест до снимка: `pending.json` не появляется, ни одна стратегия не стартует;
- строки zapret2 идут после строк Flowseal (встроенные, затем свои), только когда движок открыт; ошибка статической проверки видна в строке; активная — по имени без учёта регистра; результат устаревает после правки текста;
- карточка стратегии zapret2: глобальная часть, профили, командная строка с подстановками и исходный текст;
- смешанный рейтинг: сначала счёт, при равенстве Flowseal раньше (`zapret2:` сортируется после его имён), ошибки запуска последними, какой бы ни был движок;
- прогон из версии без zapret2 (без поля `zapret2_version`) читается.

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p tester`
Expected: FAIL при компиляции: у `TestRun` нет поля `zapret2_version` (E0560), у `StrategyDetail` — поля `source` (E0609), нет `tester::run::lua_failure` (E0432), `zapret2` не подключён (E0433), у `rows`, `detail` и `run` лишний аргумент (E0061).

- [ ] **Шаг 3: Зависимость и модель**

`crates/tester/Cargo.toml`, после `zapret.workspace = true` добавить `zapret2.workspace = true`.

`crates/tester/src/model.rs`, в `pub struct TestRun` после `pub flowseal_version: Option<String>,` добавить:

```rust
    /// `winws2.exe --version` when zapret2 strategies ran: the hash of a command line does not see
    /// an engine update (zapret2 spec 7). Runs from before zapret2 have none.
    #[serde(default)]
    pub zapret2_version: Option<String>,
```

- [ ] **Шаг 4: Список и карточка**

`crates/tester/src/catalog.rs` заменить целиком:

```rust
//! The rows and the detail pane of the «Стратегии» tab.

use std::collections::HashMap;
use std::path::Path;

use zapret::custom::{CustomStrategy, Store};
use zapret::folder::Folder;
use zapret::strategies::StrategyFile;
use zapret2::Engine;
use zapret2::strategies::Strategy;

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
    /// File stem: `general (ALT11)`, `general (HUB mine)`; `zapret2:<name>` for zapret2.
    pub name: String,
    /// A `HUB` file with a record in `data/strategies` (spec 5.2), or a zapret2 strategy of the
    /// user's.
    pub custom: bool,
    pub active: bool,
    /// Not a strategy (`service.bat` would capture nothing), not readable, or not passing the
    /// static check of zapret2.
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
    /// A zapret2 strategy's text as written, for its editor; empty for Flowseal's.
    pub source: String,
}

/// Every strategy file in `service.bat`'s order, then zapret2's (with `engine2`); `active` is the
/// registry's strategy name.
pub fn rows(
    folder: &Folder,
    engine2: Option<&Engine>,
    store: &Store,
    data_dir: &Path,
    active: Option<&str>,
) -> anyhow::Result<Vec<StrategyRow>> {
    let records = store.list()?;
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    let mut rows: Vec<StrategyRow> = zapret::strategies::list(folder)?
        .iter()
        .map(|file| flowseal_row(folder, &records, &latest, active, file))
        .collect();
    if let Some(engine) = engine2 {
        for strategy in zapret2::strategies::list(data_dir)? {
            rows.push(zapret2_row(engine, folder, &latest, active, &strategy).0);
        }
    }
    Ok(rows)
}

/// Flowseal's or zapret2's card, by the name's prefix (zapret2 spec 3.1).
pub fn detail(
    folder: &Folder,
    engine2: Option<&Engine>,
    store: &Store,
    data_dir: &Path,
    active: Option<&str>,
    name: &str,
) -> anyhow::Result<StrategyDetail> {
    let latest = crate::history::latest_results(&crate::history::dir(data_dir))?;
    let result = latest
        .get(&name.to_lowercase())
        .map(|latest| latest.result.clone());
    if let Some(name) = name.strip_prefix(zapret2::PREFIX) {
        let engine = engine2.ok_or_else(|| {
            anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings")
        })?;
        let strategy = zapret2::strategies::find(data_dir, name)?;
        let (wf, blocks) = zapret2::strategies::split(&zapret2::strategies::args(&strategy.text));
        let (row, tail) = zapret2_row(engine, folder, &latest, active, &strategy);
        return Ok(StrategyDetail {
            row,
            wf,
            blocks,
            final_args: tail,
            result,
            record: None,
            source: strategy.text,
        });
    }
    let file = zapret::strategies::find(folder, name)?;
    let records = store.list()?;
    let row = flowseal_row(folder, &records, &latest, active, &file);
    let (wf, blocks) =
        zapret::custom::split_bat(&zapret::read_text(&file.path)?).unwrap_or_default();
    Ok(StrategyDetail {
        row,
        wf,
        blocks,
        final_args: zapret::batparse::parse_file(folder, &file.path)
            .ok()
            .map(|parsed| parsed.final_args),
        result,
        record: records
            .into_iter()
            .find(|r| r.file_stem().eq_ignore_ascii_case(&file.name)),
        source: String::new(),
    })
}

fn flowseal_row(
    folder: &Folder,
    records: &[CustomStrategy],
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    file: &StrategyFile,
) -> StrategyRow {
    let custom = records
        .iter()
        .any(|r| r.file_stem().eq_ignore_ascii_case(&file.name));
    let sha256 = zapret::batparse::parse_file(folder, &file.path).map(|parsed| parsed.sha256());
    row(latest, active, file.name.clone(), custom, sha256)
}

/// The row and, when the placeholders could be filled, the command line.
fn zapret2_row(
    engine: &Engine,
    folder: &Folder,
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    strategy: &Strategy,
) -> (StrategyRow, Option<String>) {
    let tail = zapret2::strategies::tail(engine, Some(folder), &strategy.text);
    let sha256 = match &tail {
        Ok(tail) => {
            zapret2::strategies::check(engine, tail).map(|()| zapret::sha256_hex(tail.as_bytes()))
        }
        Err(err) => Err(anyhow::anyhow!("{err:#}")),
    };
    let row = row(
        latest,
        active,
        strategy.full_name(),
        !strategy.builtin,
        sha256,
    );
    (row, tail.ok())
}

/// `sha256` hashes the arguments the strategy would run with now, or says why it cannot run.
fn row(
    latest: &HashMap<String, Latest>,
    active: Option<&str>,
    name: String,
    custom: bool,
    sha256: anyhow::Result<String>,
) -> StrategyRow {
    let last = latest.get(&name.to_lowercase()).map(|latest| LastResult {
        run_id: latest.run_id.clone(),
        suite: latest.suite,
        score: latest.result.score.clone(),
        launch_error: latest.result.launch_error.is_some(),
        stale: sha256
            .as_ref()
            .is_ok_and(|sha256| *sha256 != latest.result.args_sha256),
    });
    StrategyRow {
        active: active.is_some_and(|active| active.eq_ignore_ascii_case(&name)),
        custom,
        error: sha256.err().map(|err| format!("{err:#}")),
        last,
        name,
    }
}
```

Пояснения:
- Ветка по префиксу в `detail` — вторая из трёх (спецификация 3.1); `rows` перечисляет оба движка подряд и ветки не требует.
- Хеш строки zapret2 — от её командной строки после подстановок, как и то, что тест запишет в `args_sha256`: правка текста или смена папки Flowseal делает результат устаревшим, как у Flowseal.
- Ошибка статической проверки идёт в `error` строки, как ошибка разбора `.bat` у Flowseal: стратегия видна, но «Применить» её не поставит.

- [ ] **Шаг 5: Тестовый прогон**

`crates/tester/src/run.rs`, doc-комментарий модуля

```rust
//! A test run (spec 7.3): snapshot, zapret stopped, each strategy started as winws directly with
//! the arguments «Применить» would install, the checks, everything put back.
```

заменить на

```rust
//! A test run (spec 7.3): snapshot, zapret stopped, each strategy started as winws (or winws2,
//! zapret2 spec 7) directly with the arguments «Применить» would install, the checks, everything
//! put back.
```

после `use std::path::{Path, PathBuf};` добавить `use std::process::ExitStatus;`; после `use zapret::procs::Winws;` добавить `use zapret::service::{SERVICE, SERVICE2, WINWS2};`; после `use zapret::targets::Target;` добавить `use zapret2::Engine;`.

после `const SETTLE: Duration = Duration::from_millis(300);` добавить:

```rust
/// winws2 reports capture started before its Lua starts: it gets this long more, and must still
/// run then (zapret2 spec 7).
const LUA_SETTLE: Duration = Duration::from_secs(2);
```

начало функции `run`

```rust
/// panic goes on afterwards).
pub fn run(
    folder: &Folder,
    data_dir: &Path,
```

заменить на

```rust
/// panic goes on afterwards). `engine2` runs the `zapret2:` strategies.
pub fn run(
    folder: &Folder,
    engine2: Option<&Engine>,
    data_dir: &Path,
```

после `request.check()?;` добавить `let zapret2_version = zapret2_version(engine2, request)?;` — версия читается и папка проверяется до снимка, пока ничего не изменено; строку

```rust
        let outcome = run_strategies(folder, request, &work, cancel, progress);
```

заменить на

```rust
        let outcome = run_strategies(folder, engine2, data_dir, request, &work, cancel, progress);
```

после `flowseal_version: folder.version(),` добавить `zapret2_version,`.

после функции `run` (перед `/// What every strategy is checked against.`) добавить:

```rust
/// The engine version for the run's record. A `zapret2:` strategy without a working engine stops
/// the run here, before anything changes (zapret2 spec 7).
fn zapret2_version(engine2: Option<&Engine>, request: &Request) -> anyhow::Result<Option<String>> {
    if !request
        .strategies
        .iter()
        .any(|name| name.starts_with(zapret2::PREFIX))
    {
        return Ok(None);
    }
    let engine = engine2
        .ok_or_else(|| anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings"))?;
    Ok(Some(engine.version()?.text))
}
```

начало функции `run_strategies`

```rust
fn run_strategies(
    folder: &Folder,
    request: &Request,
```

заменить на

```rust
fn run_strategies(
    folder: &Folder,
    engine2: Option<&Engine>,
    data_dir: &Path,
    request: &Request,
```

строки

```rust
    zapret::service::stop(zapret::service::SERVICE)?;
    zapret::procs::kill_all("winws.exe")?;
```

заменить на

```rust
    // Both engines: a run may mix them (zapret2 spec 7).
    for (service, image) in [(SERVICE, "winws.exe"), (SERVICE2, WINWS2)] {
        zapret::service::stop(service)?;
        zapret::procs::kill_all(image)?;
    }
```

строку `let result = run_one(folder, name, work, cancel);` заменить на `let result = run_one(folder, engine2, data_dir, name, work, cancel);`.

функцию `run_one` целиком (до `fn check_target`) заменить на:

```rust
/// How a strategy starts: the program, its working folder, the arguments and their hash.
struct Launch {
    exe: PathBuf,
    cwd: PathBuf,
    tail: String,
    sha256: String,
    zapret2: bool,
}

/// The one place a test run tells the engines apart (zapret2 spec 3.1): by the name's prefix.
fn launch(
    folder: &Folder,
    engine2: Option<&Engine>,
    data_dir: &Path,
    name: &str,
) -> anyhow::Result<Launch> {
    let Some(name) = name.strip_prefix(zapret2::PREFIX) else {
        let file = zapret::strategies::find(folder, name)?;
        let parsed = zapret::batparse::parse_file(folder, &file.path)?;
        return Ok(Launch {
            exe: folder.winws(),
            cwd: folder.bin(),
            tail: parsed.command_tail(),
            sha256: parsed.sha256(),
            zapret2: false,
        });
    };
    let engine = engine2
        .ok_or_else(|| anyhow::anyhow!("the zapret2 folder is not set: choose it in Settings"))?;
    let strategy = zapret2::strategies::find(data_dir, name)?;
    let tail = zapret2::strategies::tail(engine, Some(folder), &strategy.text)?;
    zapret2::strategies::check(engine, &tail)?;
    Ok(Launch {
        exe: engine.exe().to_path_buf(),
        cwd: engine.root().to_path_buf(),
        // ponytail: the engine version is not in the hash (the run keeps it); add it if old
        // results start to mislead (zapret2 spec 12).
        sha256: zapret::sha256_hex(tail.as_bytes()),
        tail,
        zapret2: true,
    })
}

fn run_one(
    folder: &Folder,
    engine2: Option<&Engine>,
    data_dir: &Path,
    name: &str,
    work: &Work,
    cancel: &AtomicBool,
) -> StrategyResult {
    let mut result = StrategyResult {
        name: name.to_owned(),
        args_sha256: String::new(),
        launch_error: None,
        targets: Vec::new(),
        dpi: Vec::new(),
        score: Score::default(),
    };
    let launch = match launch(folder, engine2, data_dir, name) {
        Ok(launch) => launch,
        Err(err) => {
            result.launch_error = Some(format!("{err:#}"));
            return result;
        }
    };
    result.args_sha256 = launch.sha256;
    let mut winws = match Winws::start(&launch.exe, &launch.cwd, &launch.tail) {
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
    std::thread::sleep(if launch.zapret2 { LUA_SETTLE } else { SETTLE });
    // A winws2 whose Lua failed is gone by now: nothing to check.
    if !(launch.zapret2 && winws.exited().is_some()) {
        let workers = parallelism();
        match work {
            Work::Standard(targets) => {
                result.targets = par_map(targets, workers, |(target, extra)| {
                    check_target(target, *extra, cancel)
                });
                result.score = crate::rank::score_standard(&result.targets);
            }
            Work::Dpi(targets, payload) => {
                result.dpi = par_map(targets, workers, |target| {
                    check_dpi(target, payload, cancel)
                });
                result.score = crate::rank::score_dpi(&result.dpi);
            }
        }
    }
    // Read after `stop`, which waits for the readers: the output is whole.
    let exited = winws.exited();
    let output = winws.stop();
    result.launch_error = if launch.zapret2 {
        lua_failure(exited, &output)
    } else {
        exited.map(|status| format!("winws exited during the checks ({status}):\n{output}"))
    };
    result
}

/// winws2 prints its ready line before its Lua starts. One that has ended, or printed `LUA ERROR`
/// (a strategy that fails on every packet does nothing), did not run the strategy: a launch error
/// with its whole output, or a strategy that did nothing would rank above working ones (zapret2
/// spec 7). Exit code 87 is a failed Lua start.
pub fn lua_failure(exited: Option<ExitStatus>, output: &str) -> Option<String> {
    if let Some(status) = exited {
        return Some(format!("winws2 exited ({status}):\n{output}"));
    }
    output
        .contains("LUA ERROR")
        .then(|| format!("winws2 printed LUA ERROR:\n{output}"))
}
```

Пояснения:
- `launch` — первая из трёх веток по префиксу (спецификация 3.1). Перед каждой стратегией zapret2 — статическая проверка: стратегия, которая не прошла бы «Применить», в тесте не запускается, а получает ошибку запуска с причиной.
- winws2 печатает строку готовности до того, как запускает Lua (спецификация 7), поэтому после неё ждём `LUA_SETTLE` и проверяем, что процесс жив. Вышедший winws2 и `LUA ERROR` в выводе — ошибка запуска с полным выводом.
- Проверки сайтов не ходят, если winws2 уже вышел: результат всё равно станет ошибкой запуска.

- [ ] **Шаг 6: Приложение передаёт `None`**

`src-tauri/src/commands.rs`, в `list_strategies` и `get_strategy` после строки `&state.folder()?,` (в вызовах `tester::catalog::rows(` и `tester::catalog::detail(`) добавить строку `None,`.

`src-tauri/src/jobs.rs`, в `start_tests` после `&folder,` в вызове `tester::run::run(` добавить строку `None,`.

Задача 7 заменит эти `None` движком из настроек.

- [ ] **Шаг 7: Привязки TypeScript**

Run: `cargo run -p zapret-hub --bin export-bindings`
Expected: в `frontend/src/lib/ipc/bindings.ts` у `StrategyDetail` — поле `source: string`, у `TestRun` — `zapret2_version?: string | null`, у `StrategyRow` — новые doc-комментарии; больше ничего.

Run: `npm --prefix frontend run check`
Expected: 0 ошибок, 0 предупреждений.

- [ ] **Шаг 8: Тесты проходят**

Run: `cargo test -p tester`
Expected: PASS: `catalog` — 2, `history` — 4, `rank` — 9, `run` — 6 и 4 `ignored`.

Run: `cargo test --workspace --exclude zapret-hub`
Expected: PASS.

Run: `cargo test -p zapret-hub --lib`
Expected: PASS, 16 тестов.

- [ ] **Шаг 9: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 10: Коммит**

```bash
git add Cargo.lock crates/tester/Cargo.toml crates/tester/src/model.rs crates/tester/src/catalog.rs crates/tester/src/run.rs crates/tester/tests/catalog.rs crates/tester/tests/history.rs crates/tester/tests/rank.rs crates/tester/tests/run.rs src-tauri/src/commands.rs src-tauri/src/jobs.rs src-tauri/src/texts.rs crates/cli/src/main.rs frontend/src/lib/ipc/bindings.ts
git commit -m "feat(tester): run and list zapret2 strategies next to Flowseal's"
```

---

### Задача 7: Приложение — папка zapret2, «Применить», обзор и трей

**Files:**
- Modify: `src-tauri/Cargo.toml`, `src-tauri/src/settings.rs`, `src-tauri/src/state.rs`, `src-tauri/src/actions.rs`, `src-tauri/src/dto.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/jobs.rs`, `src-tauri/src/tray.rs`, `src-tauri/src/texts.rs`, `frontend/src/lib/ipc/bindings.ts` (генерируется), `Cargo.lock` (обновит cargo)
- Test: `src-tauri/src/settings.rs` и `src-tauri/src/state.rs` (модули `tests`)

**Interfaces:**
- Consumes: `zapret2::{Engine, Info, info, PREFIX}` (задача 3); `zapret2::service::{apply, reinstall_active}` (задача 5); `zapret::service::active() -> (&'static str, &'static str)` (задача 1); второй параметр `engine2` у `tester::catalog::{rows, detail}` и `tester::run::run` (задача 6).
- Produces:
  - настройка `zapret2_dir: Option<String>` в `zapret-hub.toml` (в `bindings.ts` — `zapret2_dir?: string | null`);
  - `settings::to_save` отклоняет смену `zapret2_dir` из канала CLI (спецификация 10);
  - `AppState::engine2(&self) -> anyhow::Result<Option<zapret2::Engine>>` — `None`, пока папка не выбрана; `AppState::engine2_required(&self) -> anyhow::Result<zapret2::Engine>`;
  - `Overview.service_name: String` (`zapret` или `zapret2`) и `Overview.zapret2: zapret2::Info`; `winws_running` теперь про процесс активной службы;
  - `actions::apply` — третья ветка по префиксу (спецификация 3.1): `zapret2:<имя>` уходит в `zapret2::service::apply`.

- [ ] **Шаг 1: Падающие тесты**

`src-tauri/src/settings.rs`, в тесте `settings_survive_a_save` после строки `zapret_dir: Some(r"E:\Zapret".into()),` добавить (в `only_the_window_moves_the_zapret_folder` не трогать):

```rust
            zapret2_dir: Some(r"D:\zapret2".into()),
```

там же, после теста `only_the_window_moves_the_zapret_folder`, добавить:

```rust
    #[test]
    fn only_the_window_moves_the_zapret2_folder() {
        let current = Settings {
            zapret2_dir: Some(r"D:\zapret2".into()),
            cli_enabled: true,
            ..Settings::default()
        };
        let in_dir = |dir: Option<&str>| Settings {
            zapret2_dir: dir.map(str::to_owned),
            ..current.clone()
        };
        for moved in [in_dir(Some(r"C:\Users\me\zapret2")), in_dir(None)] {
            let refused = to_save(&current, moved.clone(), true).map_err(|err| err.to_string());
            assert_eq!(
                refused,
                Err(
                    "the zapret2 folder can be changed only in the Zapret Hub window (Settings)"
                        .into()
                ),
                "{moved:?} from the pipe"
            );
            assert_eq!(
                to_save(&current, moved.clone(), false).unwrap(),
                moved,
                "the window moves it"
            );
        }
        assert_eq!(
            to_save(&current, current.clone(), true).unwrap(),
            current,
            "the same folder goes through"
        );
    }
```

`src-tauri/src/state.rs`, в конец модуля `tests` (после теста `a_run_ends_with_its_outcome_in_place` — по плану CLI; сверить при выполнении):

```rust
    #[test]
    fn zapret2_is_off_until_its_folder_is_set() {
        let dir = tempfile::tempdir().unwrap();
        let state = state(dir.path());
        assert_eq!(state.engine2().unwrap(), None);
        let err = state.engine2_required().unwrap_err();
        assert!(
            format!("{err:#}").contains("zapret2 folder is not set"),
            "{err:#}"
        );
        state.set_settings(Settings {
            zapret2_dir: Some(dir.path().display().to_string()),
            ..Settings::default()
        });
        let err = state.engine2_required().unwrap_err();
        assert!(format!("{err:#}").contains("missing winws2.exe"), "{err:#}");
    }
```

Пустая настройка — zapret2 выключен, и это не ошибка; выбранная, но негодная папка — ошибка с недостающим файлом.

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p zapret-hub --lib`
Expected: FAIL при компиляции: у `Settings` нет поля `zapret2_dir` (E0560), у `AppState` нет методов `engine2` и `engine2_required` (E0599).

- [ ] **Шаг 3: Настройка и движок в состоянии**

`src-tauri/Cargo.toml`, после `zapret.workspace = true` добавить `zapret2.workspace = true`.

`src-tauri/src/settings.rs`, в `pub struct Settings` после `pub zapret_dir: Option<String>,` добавить:

```rust
    /// The zapret2 folder (zapret2 spec 4); `None` while zapret2 is off.
    pub zapret2_dir: Option<String>,
```

в `impl Default for Settings` после `zapret_dir: None,` добавить `zapret2_dir: None,`.

функцию `to_save` вместе с doc-комментарием заменить на:

```rust
/// What a save leaves of `sent` against the settings in force. The window's goes as sent. One from
/// zapret-hub-cli (`pipe`) never turns the CLI on: a save that passed the gate just before the user
/// turned the CLI off must not turn it back on (CLI spec 7.2). Nor does it move the zapret or the
/// zapret2 folder: the SYSTEM service runs winws.exe or winws2.exe from it, so any program of the
/// user could pick what SYSTEM runs (CLI spec 5, zapret2 spec 10).
pub fn to_save(current: &Settings, mut sent: Settings, pipe: bool) -> anyhow::Result<Settings> {
    if pipe {
        anyhow::ensure!(
            sent.zapret_dir == current.zapret_dir,
            "the zapret folder can be changed only in the Zapret Hub window (Settings)"
        );
        anyhow::ensure!(
            sent.zapret2_dir == current.zapret2_dir,
            "the zapret2 folder can be changed only in the Zapret Hub window (Settings)"
        );
        sent.cli_enabled &= current.cli_enabled;
    }
    Ok(sent)
}
```

`src-tauri/src/state.rs`, после метода `folder` (перед `pub fn store`) добавить:

```rust
    /// The zapret2 engine; `None` while `zapret2_dir` is empty (zapret2 spec 4).
    pub fn engine2(&self) -> anyhow::Result<Option<zapret2::Engine>> {
        let dir = lock(&self.settings).zapret2_dir.clone();
        dir.map(zapret2::Engine::open).transpose()
    }

    /// For what cannot go without zapret2: applying, checking and saving its strategies.
    pub fn engine2_required(&self) -> anyhow::Result<zapret2::Engine> {
        self.engine2()?
            .context("the zapret2 folder is not set: choose it in Settings")
    }
```

Движок открывается заново при каждом обращении, как `folder()`: папку могли обновить или удалить, пока приложение работало. Это дёшево: `Engine::open` только проверяет файлы, а winws2 запускает лишь `Engine::version`, и то один раз до смены файлов (кеш задачи 3).

- [ ] **Шаг 4: «Применить», перезапуск, настройки и обзор**

`src-tauri/src/actions.rs`, строку `use zapret::service::{QuickCheck, SERVICE};` заменить на `use zapret::service::{QuickCheck, active};`.

функцию `apply`

```rust
/// «Применить» (spec 5.4).
pub fn apply(app: &AppHandle, name: &str) -> anyhow::Result<QuickCheck> {
    let result = state(app).change_safely("apply", |folder| zapret::service::apply(folder, name));
    finish_with_check(app, result)
}
```

заменить на

```rust
/// «Применить» (spec 5.4). A `zapret2:` name goes to zapret2 (zapret2 spec 3.1, 6); its folder
/// is opened first, so a broken one changes nothing.
pub fn apply(app: &AppHandle, name: &str) -> anyhow::Result<QuickCheck> {
    let state = state(app);
    let result = match name.strip_prefix(zapret2::PREFIX) {
        Some(name) => state.engine2_required().and_then(|engine| {
            state.change_safely("apply", |folder| {
                zapret2::service::apply(&engine, folder, &state.paths.data(), name)
            })
        }),
        None => state.change_safely("apply", |folder| zapret::service::apply(folder, name)),
    };
    finish_with_check(app, result)
}
```

в `restart` строку `zapret::service::restart(SERVICE)?;` заменить на `zapret::service::restart(active().0)?;`.

в `save_settings` строки

```rust
    let folder_changed = settings.zapret_dir != old.zapret_dir;
    if folder_changed {
        anyhow::ensure!(
            !state.testing(),
            "a test run is going: wait for it or cancel it"
        );
        if let Some(dir) = &settings.zapret_dir {
            Folder::open(dir)?;
        }
    }
```

заменить на

```rust
    let folder_changed = settings.zapret_dir != old.zapret_dir;
    let engine_changed = settings.zapret2_dir != old.zapret2_dir;
    if folder_changed || engine_changed {
        anyhow::ensure!(
            !state.testing(),
            "a test run is going: wait for it or cancel it"
        );
    }
    if folder_changed && let Some(dir) = &settings.zapret_dir {
        Folder::open(dir)?;
    }
    if engine_changed && let Some(dir) = &settings.zapret2_dir {
        zapret2::Engine::open(dir)?;
    }
```

в doc-комментарии `save_settings` две последние строки

```rust
/// file, the memory and the scheduled task could disagree. A save from the `pipe` never turns the
/// CLI on and never moves the zapret folder (`settings::to_save`, CLI spec 5, 7.2).
```

заменить на

```rust
/// file, the memory and the scheduled task could disagree. A save from the `pipe` never turns the
/// CLI on and never moves the zapret or zapret2 folder (`settings::to_save`, CLI spec 5, 7.2,
/// zapret2 spec 10).
```

начало функции `overview`

```rust
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
```

заменить на

```rust
    let state = state(app);
    let folder = state.folder();
    let settings = state.settings();
    let (service, image) = active();
    Overview {
        folder: settings.zapret_dir,
        folder_error: folder.as_ref().err().map(|err| format!("{err:#}")),
        version: folder.as_ref().ok().and_then(Folder::version),
        update: state.update(),
        service_name: service.to_owned(),
        service: zapret::service::state(service),
        windivert: zapret::service::state("WinDivert"),
        winws_running: zapret::procs::is_running(image),
        strategy: zapret::service::strategy(),
        zapret2: zapret2::info(settings.zapret2_dir.as_deref()),
```

`src-tauri/src/dto.rs`, в `pub struct Overview` строки

```rust
    pub service: SvcState,
    pub windivert: SvcState,
    pub winws_running: bool,
    pub strategy: Option<String>,
```

заменить на

```rust
    /// The active engine's service: `zapret`, or `zapret2` once installed (zapret2 spec 3.1).
    pub service_name: String,
    pub service: SvcState,
    pub windivert: SvcState,
    /// That service's process: `winws.exe` or `winws2.exe`.
    pub winws_running: bool,
    pub strategy: Option<String>,
    /// The zapret2 folder and its version, or why it cannot be used.
    pub zapret2: zapret2::Info,
```

Пояснения:
- `apply` открывает движок до `change_safely`: негодная папка zapret2 не трогает ни службы, ни снимок. Сам `zapret2::service::apply` идёт под снимком, как Flowseal: неудача на любом шаге возвращает обе службы.
- `save_settings` проверяет только `Engine::open` — файлы на месте. Версию и совместимость Lua показывают «Настройки» и «Обзор» через `zapret2::info`, а «Применить» и тест их требуют; так выбор папки с ещё не обновлённым Lua не блокируется.
- `strategy` уже возвращает имя активной службы (задача 1), поэтому `strategy: zapret::service::strategy()` не меняется.

- [ ] **Шаг 5: Команды, тест, трей и тексты**

`src-tauri/src/commands.rs`, строку `use zapret::service::{QuickCheck, SERVICE};` заменить на `use zapret::service::{QuickCheck, active};`; в doc-комментарии `save_settings_from_pipe` строку

```rust
/// `save_settings` for zapret-hub-cli (`cli.rs`), not a window command: it never turns the CLI on.
```

заменить на

```rust
/// `save_settings` for zapret-hub-cli (`cli.rs`), not a window command: it never turns the CLI on
/// and never moves the zapret or zapret2 folder (`settings::to_save`).
```

в `list_strategies` и `get_strategy` строку `None,`, добавленную задачей 6, заменить на:

```rust
            state.engine2().ok().flatten().as_ref(),
```

Негодная папка zapret2 прячет строки zapret2, а Flowseal показывается как раньше; причина видна в «Настройках» и «Обзоре».

функцию `set_game_filter` вместе с doc-комментарием заменить на:

```rust
/// The ports are baked into the active service's arguments: it is installed again (spec 5.5).
#[tauri::command]
#[specta::specta]
pub async fn set_game_filter(handle: AppHandle, filter: GameFilter) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let engine2 = state.engine2().ok().flatten();
        let done = state.change_safely("game_filter", |folder| {
            zapret::game_filter::write(folder, &filter)?;
            zapret::service::reinstall_active(folder)?;
            zapret2::service::reinstall_active(engine2.as_ref(), folder, &state.paths.data())
        });
        actions::changed(&handle);
        done
    })
    .await
}
```

в `set_ipset_mode` и `replace_fake` строку `zapret::service::restart_if_running(SERVICE)` заменить на `zapret::service::restart_if_running(active().0)`: списки и фейки Flowseal читает и служба `zapret2` (спецификация 5.1, 6), и после замены её тоже надо перезапустить.

`src-tauri/src/jobs.rs`, в `start_tests` после `let folder = state.folder()?;` добавить:

```rust
    // Only a run with zapret2 strategies needs zapret2: then a folder that opens, or the reason.
    let engine2 = if request
        .strategies
        .iter()
        .any(|name| name.starts_with(zapret2::PREFIX))
    {
        Some(state.engine2_required()?)
    } else {
        None
    };
```

и строку `None,`, добавленную задачей 6 в вызов `tester::run::run(`, заменить на `engine2.as_ref(),`. Прогон только из стратегий Flowseal папку zapret2 не открывает вовсе; прогон со стратегиями zapret2 при пустой или негодной папке не начинается, и ошибка называет причину (`the zapret2 folder is not set…` или недостающий файл). `tester::run::run` проверяет то же сам — для других вызывающих.

`src-tauri/src/tray.rs`, строку `use zapret::service::{QuickCheck, SERVICE, SvcState};` заменить на `use zapret::service::{QuickCheck, SvcState, active};`, а `light(zapret::service::state(SERVICE), state.quick().as_ref())` — на `light(zapret::service::state(active().0), state.quick().as_ref())`.

`src-tauri/src/texts.rs`, в `quick_result` строку `return "Служба zapret не запущена".to_owned();` заменить на `return "Служба не запущена".to_owned();`, а `return "Служба запущена, но winws.exe не работает".to_owned();` — на `return "Служба запущена, но её процесс не работает".to_owned();`: служба может быть любой из двух.

- [ ] **Шаг 6: Привязки TypeScript**

Run: `cargo run -p zapret-hub --bin export-bindings`
Expected: в `frontend/src/lib/ipc/bindings.ts` новый тип `Info`, у `Overview` — поля `service_name` и `zapret2` и doc-комментарий у `winws_running`, у `Settings` — `zapret2_dir?: string | null`, у `setGameFilter` — новый doc-комментарий; больше ничего.

Run: `npm --prefix frontend run check`
Expected: 0 ошибок, 0 предупреждений: окно новые поля пока не читает.

- [ ] **Шаг 7: Тесты проходят**

Run: `cargo test -p zapret-hub --lib`
Expected: PASS, 18 тестов.

Run: `cargo test --workspace --exclude zapret-hub`
Expected: PASS.

- [ ] **Шаг 8: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 9: Коммит**

```bash
git add Cargo.lock src-tauri/Cargo.toml src-tauri/src/settings.rs src-tauri/src/state.rs src-tauri/src/actions.rs src-tauri/src/dto.rs src-tauri/src/commands.rs src-tauri/src/jobs.rs src-tauri/src/tray.rs src-tauri/src/texts.rs frontend/src/lib/ipc/bindings.ts
git commit -m "feat(app): wire zapret2 into settings, apply, overview and the tray"
```

---

### Задача 8: Команды zapret2 — окно и `zapret-hub-cli`

Все якоря в `crates/cli/src/lib.rs`, `crates/cli/src/main.rs`, `src-tauri/src/cli.rs` и `docs/cli.md` — по плану CLI; сверить при выполнении.

**Files:**
- Modify: `src-tauri/src/actions.rs`, `src-tauri/src/commands.rs`, `src-tauri/src/lib.rs`, `src-tauri/src/cli.rs`, `crates/cli/src/lib.rs`, `crates/cli/src/main.rs`, `docs/cli.md`, `frontend/src/lib/ipc/bindings.ts` (генерируется)
- Test: `crates/cli/src/lib.rs` и `crates/cli/src/main.rs` (модули `tests`)

**Interfaces:**
- Consumes: `AppState::{engine2, engine2_required}`, настройка `zapret2_dir` (задача 7); `zapret2::{info, PREFIX}` (задача 3); `zapret2::strategies::{tail, delete}` (задача 4); `zapret2::service::{validate, save, install}` (задача 5); `zapret::service::{strategy, quick_check}`.
- Produces:
  - команды окна (спецификация 5.2, 8): `zapret2_info() -> zapret2::Info`, `validate_zapret2(text: String) -> ()`, `save_zapret2(name: String, text: String, create: bool) -> Option<QuickCheck>`, `delete_zapret2(name: String) -> ()`; `name` — без префикса;
  - `actions::save_zapret2(app, name, text, create) -> anyhow::Result<Option<QuickCheck>>`;
  - запросы протокола CLI `Request::{Zapret2Info, ValidateZapret2 { text }, SaveZapret2 { name, text, create }, DeleteZapret2 { name }}` (в JSON — `zapret2_info`, `validate_zapret2`, `save_zapret2`, `delete_zapret2`);
  - команды `zapret-hub-cli zapret2 info|validate <file|->|save <name> <file|-> [--create]|delete <name> --yes`; `settings set zapret2_dir` — неизвестный ключ (код 2), как `zapret_dir`.

- [ ] **Шаг 1: Падающие тесты**

`crates/cli/src/lib.rs`, в функции `samples` модуля `tests` после образца

```rust
            Request::DeleteCustom {
                name: "mine".into(),
            },
```

добавить:

```rust
            Request::Zapret2Info,
            Request::ValidateZapret2 {
                text: "--wf-tcp-out=443".into(),
            },
            Request::SaveZapret2 {
                name: "mine".into(),
                text: "--wf-tcp-out=443".into(),
                create: true,
            },
            Request::DeleteZapret2 {
                name: "mine".into(),
            },
```

Так `every_request_is_a_window_command_with_its_arguments` проверит, что у каждого нового запроса есть команда окна с теми же аргументами, а `every_request_comes_through_the_gate_as_it_left` — что запросы проходят протокол без потерь.

`crates/cli/src/main.rs`, в таблице теста `every_command_sends_its_window_command` после записи

```rust
            (
                &["custom", "delete", "mine", "--yes"],
                Request::DeleteCustom {
                    name: "mine".into(),
                },
            ),
```

добавить:

```rust
            (&["zapret2", "info"], Request::Zapret2Info),
            (
                &["zapret2", "delete", "mine", "--yes"],
                Request::DeleteZapret2 {
                    name: "mine".into(),
                },
            ),
```

в тесте `what_the_window_confirms_needs_yes` после `&["custom", "delete", "mine"][..],` добавить `&["zapret2", "delete", "mine"],`;

в тесте `bad_arguments_are_refused_by_clap` строку

```rust
            // Only the window moves the folder whose winws.exe the SYSTEM service runs.
```

заменить на

```rust
            // Only the window moves the folders whose winws.exe and winws2.exe SYSTEM runs.
```

после строки

```rust
            &["settings", "set", "zapret_dir", r"C:\Users\me\winws"],
```

добавить

```rust
            &["settings", "set", "zapret2_dir", r"D:\zapret2"],
```

а после строки `                && !err.contains("zapret_dir")` — строку `                && !err.contains("zapret2_dir")`;

в функции `settings()` после строки `"zapret_dir": "E:\\Zapret",` добавить `"zapret2_dir": "D:\\zapret2",` — тогда `settings_set_changes_one_key_and_sends_all_of_them` доказывает, что CLI возвращает папки окна без изменений, и `settings::to_save` обычный `settings set` не отклоняет;

перед `fn rows() -> Value {` добавить:

```rust
    #[test]
    fn zapret2_save_and_validate_send_the_text_of_a_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("mine.txt");
        let text = "# mine\n--wf-tcp-out=443\n--lua-desync=fake:blob=fake_default_tls\n";
        std::fs::write(&file, format!("\u{feff}{text}")).unwrap();
        let path = file.to_str().unwrap();
        let (_, asked) = drive(&["zapret2", "save", "mine", path, "--create"], Vec::new());
        assert_eq!(
            asked,
            [Request::SaveZapret2 {
                name: "mine".into(),
                text: text.into(),
                create: true
            }]
        );
        let (_, asked) = drive(&["zapret2", "validate", path], Vec::new());
        assert_eq!(asked, [Request::ValidateZapret2 { text: text.into() }]);
        let missing = dir.path().join("none.txt");
        let (outcome, asked) = drive(
            &["zapret2", "validate", missing.to_str().unwrap()],
            Vec::new(),
        );
        assert_eq!(code(outcome), USAGE);
        assert!(asked.is_empty());
    }
```

Тест проверяет, что текст стратегии уходит как есть (переводы строк, `#`-комментарии), BOM Блокнота и PowerShell 5.1 снимается, а нечитаемый файл — код 2 без запроса к приложению.

- [ ] **Шаг 2: Убедиться, что тесты падают**

Run: `cargo test -p cli`
Expected: FAIL при компиляции: у `Request` нет вариантов `Zapret2Info`, `ValidateZapret2`, `SaveZapret2`, `DeleteZapret2` (E0599).

- [ ] **Шаг 3: Запросы протокола**

`crates/cli/src/lib.rs`, в `pub enum Request` после варианта

```rust
    DeleteCustom {
        name: String,
    },
```

добавить:

```rust
    Zapret2Info,
    ValidateZapret2 {
        text: String,
    },
    /// `name` without the prefix: `mine` for `zapret2:mine`.
    SaveZapret2 {
        name: String,
        text: String,
        create: bool,
    },
    DeleteZapret2 {
        name: String,
    },
```

- [ ] **Шаг 4: Команды окна**

`src-tauri/src/actions.rs`, перед `/// A folder found at start or chosen in Settings` (функция `prepare_folder`) добавить:

```rust
/// `save_zapret2` (zapret2 spec 5.2). The active strategy's command line is baked into the
/// service: saving it installs it again, under a snapshot.
pub fn save_zapret2(
    app: &AppHandle,
    name: &str,
    text: &str,
    create: bool,
) -> anyhow::Result<Option<QuickCheck>> {
    let state = state(app);
    let engine = state.engine2_required()?;
    let data = state.paths.data();
    let full = format!("{}{}", zapret2::PREFIX, name.trim());
    let active = zapret::service::strategy();
    if !create && active.is_some_and(|active| active.eq_ignore_ascii_case(&full)) {
        let result = state.change_safely("custom", |folder| {
            let (saved, tail) =
                zapret2::service::save(&engine, Some(folder), &data, name, text, false)?;
            zapret2::service::install(&engine, &saved, &tail)?;
            // winws2 needs a moment before it intercepts anything (spec 5.4).
            std::thread::sleep(Duration::from_secs(2));
            Ok(zapret::service::quick_check(folder))
        });
        return finish_with_check(app, result).map(Some);
    }
    let result = state
        .change(|folder| zapret2::service::save(&engine, Some(folder), &data, name, text, create));
    changed(app);
    result.map(|_| None)
}
```

`src-tauri/src/commands.rs`, после функции `delete_custom` (перед `start_tests`) добавить:

```rust
/// The zapret2 folder, its version and Lua compatibility, or why it cannot be used.
#[tauri::command]
#[specta::specta]
pub async fn zapret2_info(handle: AppHandle) -> Answer<zapret2::Info> {
    blocking(move || {
        Ok(zapret2::info(
            app(&handle).settings().zapret2_dir.as_deref(),
        ))
    })
    .await
}

/// The zapret2 editor's «Проверить»: the whole check of zapret2 spec 5.3, nothing saved.
#[tauri::command]
#[specta::specta]
pub async fn validate_zapret2(handle: AppHandle, text: String) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let engine = state.engine2_required()?;
        state.change(|folder| {
            let tail = zapret2::strategies::tail(&engine, Some(folder), &text)?;
            zapret2::service::validate(&engine, &tail)
        })
    })
    .await
}

/// `name` without the prefix: `mine` for `zapret2:mine`.
#[tauri::command]
#[specta::specta]
pub async fn save_zapret2(
    handle: AppHandle,
    name: String,
    text: String,
    create: bool,
) -> Answer<Option<QuickCheck>> {
    blocking(move || actions::save_zapret2(&handle, &name, &text, create)).await
}

/// `name` without the prefix: `mine` for `zapret2:mine`.
#[tauri::command]
#[specta::specta]
pub async fn delete_zapret2(handle: AppHandle, name: String) -> Answer<()> {
    blocking(move || {
        let state = app(&handle);
        let active = zapret::service::strategy();
        let deleted = state
            .change(|_| zapret2::strategies::delete(&state.paths.data(), &name, active.as_deref()));
        actions::changed(&handle);
        deleted
    })
    .await
}
```

`src-tauri/src/lib.rs`, в `specta_builder` после `commands::delete_custom,` добавить:

```rust
            commands::zapret2_info,
            commands::validate_zapret2,
            commands::save_zapret2,
            commands::delete_zapret2,
```

`src-tauri/src/cli.rs`, в `dispatch` после строки `Request::DeleteCustom { name } => json(block_on(commands::delete_custom(handle, name))?),` добавить:

```rust
        Request::Zapret2Info => json(block_on(commands::zapret2_info(handle))?),
        Request::ValidateZapret2 { text } => {
            json(block_on(commands::validate_zapret2(handle, text))?)
        }
        Request::SaveZapret2 { name, text, create } => json(block_on(commands::save_zapret2(
            handle, name, text, create,
        ))?),
        Request::DeleteZapret2 { name } => json(block_on(commands::delete_zapret2(handle, name))?),
```

Пояснения:
- `validate_zapret2` и `delete_zapret2` идут через `state.change`, как `validate_custom` и `delete_custom`: по одной за раз и не во время прогона. Проверка запускает winws2 (`--dry-run`, затем `--intercept=0`) и служб не трогает, поэтому снимок ей не нужен.
- `save_zapret2` активной стратегии — как у своих стратегий Flowseal (core 5.2): её командная строка зашита в службу, поэтому сохранение под снимком ставит службу заново той строкой, которую проверил `zapret2::service::save`, и возвращает быструю проверку. Неактивная или новая стратегия только сохраняется, ответ — `null`.
- Удаление проверяет активную стратегию по реестру (`zapret::service::strategy`): активную удалить нельзя (спецификация 5.2).

- [ ] **Шаг 5: Привязки TypeScript**

Run: `cargo run -p zapret-hub --bin export-bindings`
Expected: в `frontend/src/lib/ipc/bindings.ts` после `deleteCustom` — `zapret2Info`, `validateZapret2`, `saveZapret2`, `deleteZapret2`; больше ничего.

- [ ] **Шаг 6: Команды `zapret-hub-cli`**

`crates/cli/src/main.rs`, doc-комментарий флага `--yes`

```rust
    /// Confirm what the window asks to confirm: custom delete, hosts apply, checks fix
    /// remove-windivert|remove-conflicts, discord clear-cache, service remove.
```

заменить на

```rust
    /// Confirm what the window asks to confirm: custom delete, zapret2 delete, hosts apply, checks
    /// fix remove-windivert|remove-conflicts, discord clear-cache, service remove.
```

в `enum Command` строки

```rust
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
```

заменить на

```rust
    /// Flowseal's strategies, then zapret2's.
    #[command(subcommand)]
    Strategies(StrategiesCommand),
    /// Install a strategy as its engine's service (zapret or zapret2), then run a quick check.
    Apply { name: String },
    /// Apply the best strategy of the newest standard test run.
    ApplyBest,
    /// Restart the active service (zapret or zapret2), then run a quick check.
    Restart,
    /// Check that zapret works now.
    QuickCheck,
    /// Custom strategies.
    #[command(subcommand)]
    Custom(CustomCommand),
    /// zapret2: its folder and your own strategies (zapret2:<name>).
    #[command(subcommand)]
    Zapret2(Zapret2Command),
```

в `enum SettingsCommand` строки

```rust
    /// Change one setting; an empty value clears socks5. The zapret folder is changed only in the
    /// Zapret Hub window.
```

заменить на

```rust
    /// Change one setting; an empty value clears socks5. The zapret and zapret2 folders are
    /// changed only in the Zapret Hub window.
```

после `enum CustomCommand` (перед `#[derive(Debug, Subcommand)]` у `enum TestsCommand`) добавить:

```rust
#[derive(Debug, Subcommand)]
enum Zapret2Command {
    /// The zapret2 folder, its version and Lua compatibility, or why it cannot be used.
    Info,
    /// Check a strategy's text (a file, or - for stdin) with winws2, without saving it.
    Validate { file: PathBuf },
    /// Save a strategy's text (a file, or - for stdin) as zapret2:<name>.
    Save {
        name: String,
        file: PathBuf,
        /// A new strategy rather than a change to one.
        #[arg(long)]
        create: bool,
    },
    /// Delete one of your zapret2 strategies by its own name: mine for zapret2:mine (needs --yes).
    Delete { name: String },
}
```

в `enum ServiceCommand` строку `/// Remove the zapret service and WinDivert (needs --yes).` заменить на `/// Remove the zapret or zapret2 service and WinDivert (needs --yes).`;

у `enum Key` doc-комментарий

```rust
/// The keys of zapret-hub.toml the CLI may change (spec 7.2). Not `zapret_dir`: the SYSTEM service
/// runs that folder's winws.exe, so the app takes a new one only from its window (spec 5).
```

заменить на

```rust
/// The keys of zapret-hub.toml the CLI may change (spec 7.2). Not `zapret_dir` or `zapret2_dir`:
/// the SYSTEM service runs that folder's winws.exe or winws2.exe, so the app takes a new one only
/// from its window (spec 5; zapret2 spec 10).
```

сам `enum Key` и функция `setting` не меняются: ключа `zapret2_dir` нет, как нет и `zapret_dir`;

в `needs_yes` после `Command::Custom(CustomCommand::Delete { .. })` добавить строку `| Command::Zapret2(Zapret2Command::Delete { .. })`;

в `run` после ветки

```rust
        Command::Custom(CustomCommand::Save { file, create }) => Request::SaveCustom {
            strategy: read_strategy(&file)?,
            create,
        },
```

добавить:

```rust
        Command::Zapret2(Zapret2Command::Validate { file }) => Request::ValidateZapret2 {
            text: read_text(&file)?,
        },
        Command::Zapret2(Zapret2Command::Save { name, file, create }) => Request::SaveZapret2 {
            name,
            text: read_text(&file)?,
            create,
        },
```

а после ветки `Command::Custom(CustomCommand::Delete { name }) => Request::DeleteCustom { name },` — две:

```rust
        Command::Zapret2(Zapret2Command::Info) => Request::Zapret2Info,
        Command::Zapret2(Zapret2Command::Delete { name }) => Request::DeleteZapret2 { name },
```

начало `read_strategy`

```rust
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
```

заменить на

```rust
/// A file, or stdin for `-` (spec 7.3). PowerShell 5.1 (`Set-Content -Encoding UTF8`) and
/// Notepad start the file with a BOM: it is dropped.
fn read_text(file: &Path) -> Result<String, Fail> {
    let text = if file == Path::new("-") {
        io::read_to_string(io::stdin())
    } else {
        std::fs::read_to_string(file)
    }
    .map_err(|err| Fail::new(USAGE, format!("cannot read {}: {err}", file.display())))?;
    Ok(text.trim_start_matches('\u{feff}').to_owned())
}

/// A `CustomStrategy` JSON from a file, or from stdin for `-` (spec 7.3).
fn read_strategy(file: &Path) -> Result<CustomStrategy, Fail> {
    serde_json::from_str(&read_text(file)?).map_err(|err| {
```

Имена в CLI — без префикса (`zapret2 delete mine`), как у `custom delete mine`; `apply`, `strategies show` и `tests run` берут полные имена (`zapret2:mine`) из `strategies list` как есть.

- [ ] **Шаг 7: Тесты проходят**

Run: `cargo test -p cli`
Expected: PASS: `lib` — 8, `main` — 18, `pipe` — 11.

Run: `cargo test -p zapret-hub --lib`
Expected: PASS, 18 тестов.

Run: `npm --prefix frontend run check`
Expected: 0 ошибок, 0 предупреждений.

- [ ] **Шаг 8: `docs/cli.md`**

в разделе «Как включить», в абзаце «Включённый CLI даёт любой программе…» две последние строки

```markdown
программа, которая пишет в папку Flowseal: она меняет сами `.bat` и `winws.exe`. Папку zapret, а с ней и то,
какой `winws.exe` запускает служба, CLI не меняет: её меняют только в окне.
```

заменить на

```markdown
программа, которая пишет в папку Flowseal: она меняет сами `.bat` и `winws.exe`.
Так же и со своими стратегиями zapret2: их текст получает `winws2.exe` от SYSTEM (служба zapret2) или от
администратора (проверка), а программа, которая пишет в папку zapret2, меняет сам `winws2.exe` и его Lua. Папки
zapret и zapret2, а с ними и то, какие `winws.exe` и `winws2.exe` запускают службы, CLI не меняет: их меняют
только в окне.
```

вторую строку абзаца перед таблицей команд

```markdown
`zapret-hub.toml` (`start_minimized`). Имя стратегии — имя её файла без `.bat`, например `"general (ALT11)"`.
```

заменить на

```markdown
`zapret-hub.toml` (`start_minimized`). Имя стратегии — имя её файла без `.bat`, например `"general (ALT11)"`;
стратегии zapret2 называются `zapret2:<имя>`, например `zapret2:general` (раздел «Стратегии zapret2»).
```

в таблице команд после строки `custom delete <name> --yes` добавить:

```markdown
| `zapret2 info` | папка zapret2, версия движка и совместимость Lua или ошибка | `zapret2_info` |
| `zapret2 validate <file\|->` | проверяет текст стратегии zapret2, не сохраняя | `validate_zapret2` |
| `zapret2 save <name> <file\|-> [--create]` | сохраняет текст как `zapret2:<name>`; `--create` — как новую стратегию | `save_zapret2` |
| `zapret2 delete <name> --yes` | удаляет свою стратегию zapret2; `<name>` — без префикса: `mine` для `zapret2:mine` | `delete_zapret2` |
```

строку

```markdown
| `service remove --yes` | удаляет службу zapret и WinDivert | `remove_service` |
```

заменить на

```markdown
| `service remove --yes` | удаляет службу zapret или zapret2 и WinDivert | `remove_service` |
```

в разделе «Настройки» абзац после таблицы ключей

```markdown
Папку zapret (`zapret_dir`) CLI не меняет: служба zapret запускает `winws.exe` из неё от SYSTEM, поэтому папку
меняют только в окне («Настройки» → «Папка zapret» → «Сохранить»). `settings get` её показывает, а
`settings set zapret_dir …` — неизвестный ключ, код 2.
```

заменить на

```markdown
Папки zapret и zapret2 (`zapret_dir`, `zapret2_dir`) CLI не меняет: службы zapret и zapret2 запускают из них
`winws.exe` и `winws2.exe` от SYSTEM, поэтому папки меняют только в окне («Настройки» → «Папка zapret» или
«Папка zapret2» → «Сохранить»). `settings get` их показывает, а `settings set zapret_dir …` и
`settings set zapret2_dir …` — неизвестный ключ, код 2.
```

следующий за ним абзац

```markdown
Сохранение через CLI не может включить `cli_enabled`: если вы выключили его в окне, пока команда читала и
сохраняла настройки, он остаётся выключенным. Если за это время вы сменили в окне папку zapret, приложение
отклонит сохранение CLI ошибкой `the zapret folder can be changed only in the Zapret Hub window (Settings)`
(код 1): повторите команду.
```

заменить на

```markdown
Сохранение через CLI не может включить `cli_enabled`: если вы выключили его в окне, пока команда читала и
сохраняла настройки, он остаётся выключенным. Если за это время вы сменили в окне папку zapret или zapret2,
приложение отклонит сохранение CLI ошибкой
`the zapret folder can be changed only in the Zapret Hub window (Settings)` или
`the zapret2 folder can be changed only in the Zapret Hub window (Settings)` (код 1): повторите команду.
```

после строки ``Удалить: `zapret-hub-cli custom delete mine --yes`.`` (перед `## Если приложение не запущено`) добавить раздел:

````markdown
## Стратегии zapret2

Если в окне задана папка zapret2 («Настройки» → «Папка zapret2»; CLI её не меняет), `strategies list` после стратегий
Flowseal показывает стратегии zapret2: сначала встроенные (`zapret2:general`, `zapret2:general-sni`), потом
свои. `apply`, `strategies show` и `tests run` принимают эти имена как есть, прогон может смешивать оба движка.
Применение стратегии zapret2 ставит службу `zapret2` и удаляет службу `zapret`, применение стратегии Flowseal —
наоборот.

`zapret2 info` печатает папку, версию движка и совместимость Lua (`compat`) или ошибку (`error`).

Своя стратегия — текст аргументов `winws2` в файле: глобальная часть (`--wf-*`, `--lua-init`, `--blob`), затем
профили через `--new`. Строки, которые начинаются с `#`, — комментарии. `%LUA%`, `%FILES%`, `%LISTS%`, `%BIN%` и
`%GameFilter%` подставляются при проверке и установке. Текст встроенной стратегии — поле `source` в выводе
`strategies show zapret2:general`: с него удобно начать.

```
zapret-hub-cli zapret2 validate mine.txt
zapret-hub-cli zapret2 save mine mine.txt --create
zapret-hub-cli apply zapret2:mine
```

- `validate` и `save` проводят полную проверку: статическую (функции `--lua-desync`, кавычки), `winws2 --dry-run`
  и `winws2 --intercept=0`. Ошибка — код 1 с полным выводом `winws2`.
- `save` без `--create` меняет существующую стратегию. Если она активна, служба ставится заново и печатается
  итог быстрой проверки, иначе — `null`.
- `-` вместо файла читает текст из stdin; BOM допускается.

Удалить: `zapret-hub-cli zapret2 delete mine --yes`. Встроенные стратегии и активную удалить нельзя.
````

в конец списка живой проверки добавить пункт:

```markdown
12. zapret2, с распакованным релизом zapret2: `settings set zapret2_dir <папка релиза>` → код 2, настройки не
    меняются; в окне «Настройки» → «Папка zapret2» выбрать папку релиза и сохранить; `settings get` →
    `zapret2_dir` с ней; `zapret2 info` →
    версия и `compat`. Текст `strategies show zapret2:general` (поле `source`) в файл → `zapret2 validate` →
    код 0; `zapret2 save mine <файл> --create` → `null`; `apply zapret2:mine` → `sc query zapret2` — RUNNING,
    `sc query zapret` службу не находит; `apply general` → наоборот. `zapret2 delete mine` → код 2, с `--yes` →
    код 0.
```

- [ ] **Шаг 9: Формат и clippy**

Run: `cargo fmt --all`
Expected: файлы не меняются.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

- [ ] **Шаг 10: Коммит**

```bash
git add src-tauri/src/actions.rs src-tauri/src/commands.rs src-tauri/src/lib.rs src-tauri/src/cli.rs crates/cli/src/lib.rs crates/cli/src/main.rs docs/cli.md frontend/src/lib/ipc/bindings.ts
git commit -m "feat(cli): add the zapret2 commands to the window and the CLI"
```

---

### Задача 9: Окно — папка zapret2, карточка на обзоре, редактор и метки

**Files:**
- Create: `frontend/src/tabs/Zapret2Editor.svelte`
- Modify: `frontend/src/lib/format.ts`, `frontend/src/lib/strings.ts`, `frontend/src/tabs/Strategies.svelte`, `frontend/src/tabs/Overview.svelte`, `frontend/src/tabs/Settings.svelte`, `frontend/src/tabs/Tests.svelte`
- Test: `frontend/src/lib/format.test.ts`

**Interfaces:**
- Consumes: `commands.{zapret2Info, validateZapret2, saveZapret2, deleteZapret2}` (задача 8), `Info`, `Overview.{service_name, zapret2}`, `Settings.zapret2_dir` (задача 7), `StrategyDetail.source`, `TestRun.zapret2_version` (задача 6); `quickSummary` из `$lib/format`, `call`, `change`, `confirmation` — уже есть.
- Produces: `ZAPRET2 = "zapret2:"` и `zapret2Name(name: string): string | null` в `$lib/format`; компонент `Zapret2Editor` (`name`, `from`, `create`, `onClose(saved: string | null)`); строки `S.zapret2.*`, `S.settings.zapret2*`, `S.tests.versions(flowseal, zapret2?)` вместо `S.tests.flowseal`, `S.overview.service(name)` вместо строки.

- [ ] **Шаг 1: Падающий тест**

`frontend/src/lib/format.test.ts`, строку `import { blockSummary, runDate } from "./format";` заменить на `import { blockSummary, runDate, zapret2Name } from "./format";`; перед `describe("runDate", () => {` добавить:

```ts
describe("zapret2Name", () => {
  it("tells zapret2 strategies by their prefix", () => {
    expect(zapret2Name("zapret2:general-sni")).toBe("general-sni");
    expect(zapret2Name("general (ALT11)")).toBeNull();
  });
});
```

- [ ] **Шаг 2: Убедиться, что тест падает**

Run: `npx vitest run src/lib/format.test.ts` (из `frontend`)
Expected: FAIL: `TypeError: zapret2Name is not a function`.

- [ ] **Шаг 3: Имя zapret2 и строки**

`frontend/src/lib/format.ts`, после функции `hubStem` добавить:

```ts
/** A zapret2 strategy's name is `zapret2:<name>` (zapret2 spec 3.1). */
export const ZAPRET2 = "zapret2:";

/** `mine` for `zapret2:mine`; `null` for a Flowseal strategy. */
export function zapret2Name(name: string): string | null {
  return name.startsWith(ZAPRET2) ? name.slice(ZAPRET2.length) : null;
}
```

`frontend/src/lib/strings.ts` (группа `settings` — по плану CLI; сверить при выполнении):

в `LINKS` после строки `release: …` добавить:

```ts
  zapret2: "https://github.com/bol-van/zapret2/releases",
```

в `failed` после `removeService: "Служба не удалена",` добавить `zapret2: "Папка zapret2 не проверена",`;

в `overview` строки

```ts
    service: "Служба zapret",
    strategy: "Стратегия",
    noStrategy: "не выбрана",
    notRunning: "служба zapret не запущена",
    winwsMissing: "служба запущена, но winws.exe не работает",
```

заменить на

```ts
    service: (name: string) => `Служба ${name}`,
    strategy: "Стратегия",
    noStrategy: "не выбрана",
    notRunning: "служба не запущена",
    winwsMissing: "служба запущена, но её процесс не работает",
```

и после `flowseal: "Flowseal",` добавить `zapret2: "zapret2",`;

в `settings` после строки `folderOk: …` добавить:

```ts
    zapret2: "Папка zapret2",
    zapret2Hint:
      "Необязательно. Распакованный релиз bol-van/zapret2: его стратегии появятся рядом со стратегиями Flowseal. Пусто — zapret2 выключен. Нужна и папка zapret выше: списки и фейки берутся из неё.",
    zapret2Example: "Например, D:\\zapret2",
    zapret2Ok: (version: string | null, compat: number | null) =>
      `zapret2 ${version ?? "?"}, совместимость Lua ${compat ?? "?"}`,
    zapret2Releases: "Релизы zapret2",
```

в `strategies` после `custom: "своя",` добавить `zapret2: "zapret2",`;

после группы `editor` (её последняя строка — `allTraffic: "все пакеты",`) и перед `results: {` добавить группу:

```ts
  zapret2: {
    hint: "Аргументы winws2: глобальная часть (--wf-…, --lua-init, --blob), затем профили через --new. Строка с # в начале — комментарий. %LUA%, %FILES%, %LISTS%, %BIN% и %GameFilter% подставляются при проверке и установке.",
    text: "Текст стратегии",
    valid: "Проверка пройдена: статическая, winws2 --dry-run и --intercept=0 без ошибок.",
    global: "Глобальная часть",
    profile: (n: number) => `Профиль ${n}`,
    commandLine: "Командная строка winws2",
    deleteText: (name: string) => `Файл «${name}.txt» в data\\zapret2 будет удалён.`,
  },
```

в `tests` строку

```ts
    flowseal: (version: string | null) => `Flowseal ${version ?? "?"}`,
```

заменить на

```ts
    versions: (flowseal: string | null, zapret2?: string | null) =>
      `Flowseal ${flowseal ?? "?"}${zapret2 ? ` · zapret2 ${zapret2}` : ""}`,
```

в `zapret` строки

```ts
    serviceHint: "Как «Remove Services» в service.bat: служба zapret, процессы winws, затем WinDivert.",
    remove: "Удалить службу",
    removeTitle: "Удалить службу zapret?",
```

заменить на

```ts
    serviceHint: "Как «Remove Services» в service.bat: служба zapret или zapret2, процессы winws, затем WinDivert.",
    remove: "Удалить службу",
    removeTitle: "Удалить службу?",
```

Run: `npx vitest run src/lib/format.test.ts` (из `frontend`)
Expected: PASS.

- [ ] **Шаг 4: Редактор стратегии zapret2**

`frontend/src/tabs/Zapret2Editor.svelte`:

```svelte
<script lang="ts">
  import { onMount } from "svelte";
  import { commands } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { ZAPRET2, quickSummary } from "$lib/format";
  import { S } from "$lib/strings";

  interface Props {
    /** Without the prefix: `mine` for `zapret2:mine`. */
    name: string;
    /** The strategy whose text the editor starts with: this one, or the one a copy is made of. */
    from: string;
    /** A copy: saved as a new strategy. */
    create: boolean;
    /** The full name of what was saved, `null` when nothing was. */
    onClose: (saved: string | null) => void;
  }

  let { name, from, create, onClose }: Props = $props();

  let text = $state<string | null>(null);
  let status = $state("");

  onMount(async () => {
    text = (await call(S.failed.strategy, commands.getStrategy(from)))?.source ?? null;
  });

  async function validate(): Promise<void> {
    if (text === null) return;
    const draft = text;
    status = "";
    if ((await change(S.failed.validate, () => commands.validateZapret2(draft))) !== undefined) status = S.zapret2.valid;
  }

  async function save(): Promise<void> {
    if (text === null) return;
    const draft = text;
    status = "";
    const check = await change(S.failed.saveCustom, () => commands.saveZapret2(name, draft, create));
    if (check === undefined) return;
    // The active strategy was installed again: its quick check is worth reading before closing.
    if (check) status = `${check.ok ? S.editor.appliedOk : S.editor.appliedFailed}\n${quickSummary(check)}`;
    else onClose(ZAPRET2 + name);
  }
</script>

{#if text !== null}
  <h2>{ZAPRET2 + name}</h2>
  <p class="muted">{S.zapret2.hint}</p>
  <textarea rows="24" spellcheck="false" bind:value={text} aria-label={S.zapret2.text}></textarea>
  <div class="buttons">
    <span class="grow"></span>
    <button disabled={app.busy} onclick={validate}>{S.editor.validate}</button>
    <button class="primary" disabled={app.busy} onclick={save}>{S.common.save}</button>
    <button onclick={() => onClose(null)}>{S.common.close}</button>
  </div>
  {#if status}<pre>{status}</pre>{/if}
{:else}
  <p class="muted">{S.common.loading}</p>
{/if}
```

Редактор — текст целиком, без блоков: стратегия zapret2 — строка аргументов, и её профили могут ссылаться на всё, что задано выше (`--lua-init`, `--blob`). Текст берётся из `source` карточки стратегии `from`: при правке — своей, при копировании — исходной. Копия сохраняется только «Сохранить» (`save_zapret2` с `create`): если исходная не прошла статическую проверку, её можно поправить до сохранения. После сохранения активной стратегии редактор остаётся открытым и показывает быструю проверку, как редактор своих стратегий Flowseal.

- [ ] **Шаг 5: Вкладка «Стратегии»**

`frontend/src/tabs/Strategies.svelte` заменить целиком:

```svelte
<script lang="ts">
  import { commands, type StrategyDetail, type StrategyRow } from "$lib/ipc/bindings";
  import { call } from "$lib/api";
  import { app, change } from "$lib/app.svelte";
  import { confirmation } from "$lib/confirm.svelte";
  import { blockSummary, hubStem, ms, runDate, zapret2Name } from "$lib/format";
  import { S } from "$lib/strings";
  import TargetChecks from "../components/TargetChecks.svelte";
  import StrategyEditor from "./StrategyEditor.svelte";
  import Zapret2Editor from "./Zapret2Editor.svelte";

  let rows = $state<StrategyRow[]>([]);
  let query = $state("");
  let detail = $state<StrategyDetail | null>(null);
  /** The stem of the custom strategy under edit. */
  let editing = $state<string | null>(null);
  /** The zapret2 strategy under edit, or a copy not saved yet (zapret2 spec 5.2). */
  let editing2 = $state<{ name: string; from: string; create: boolean } | null>(null);
  let cloneName = $state("");

  const visible = $derived(rows.filter((row) => row.name.toLowerCase().includes(query.trim().toLowerCase())));
  const locked = $derived(app.busy || (app.overview?.testing ?? false));

  async function open(name: string): Promise<void> {
    detail = (await call(S.failed.strategy, commands.getStrategy(name))) ?? null;
  }

  async function load(): Promise<void> {
    rows = (await call(S.failed.strategies, commands.listStrategies())) ?? [];
    const name = detail?.row.name;
    // A deletion announces itself before it answers: the strategy it removed must not be read again.
    if (name && rows.some((row) => row.name === name)) await open(name);
    else detail = null;
  }

  $effect(() => {
    void app.revision;
    void load();
  });

  async function clone(source: string): Promise<void> {
    const name = cloneName.trim();
    if (zapret2Name(source) !== null) {
      // A zapret2 copy is saved from its editor: the check may want its text changed first.
      cloneName = "";
      editing2 = { name, from: source, create: true };
      return;
    }
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

  async function removeZapret2(name: string): Promise<void> {
    const sure = await confirmation.ask({
      title: S.strategies.deleteTitle,
      text: S.zapret2.deleteText(name),
      yes: S.strategies.delete,
      danger: true,
    });
    if (sure && (await change(S.failed.deleteCustom, () => commands.deleteZapret2(name))) !== undefined) detail = null;
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
                  editing2 = null;
                  void open(row.name);
                }}>{row.name}</button
              >
              {#if row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
              {#if row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
              {#if zapret2Name(row.name) !== null}<span class="badge muted">{S.strategies.zapret2}</span>{/if}
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
    {:else if editing2}
      <Zapret2Editor
        name={editing2.name}
        from={editing2.from}
        create={editing2.create}
        onClose={(saved) => {
          editing2 = null;
          void (saved ? open(saved) : load());
        }}
      />
    {:else if detail}
      {@const d = detail}
      {@const z2 = zapret2Name(d.row.name)}
      <h2>
        {d.row.name}
        {#if d.row.active}<span class="badge ok">{S.strategies.active}</span>{/if}
        {#if d.row.custom}<span class="badge muted">{S.strategies.custom}</span>{/if}
        {#if z2 !== null}<span class="badge muted">{S.strategies.zapret2}</span>{/if}
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
        {#if z2 !== null && d.row.custom}
          {@const own = z2}
          <button disabled={locked} onclick={() => (editing2 = { name: own, from: d.row.name, create: false })}>
            {S.strategies.edit}
          </button>
          <button class="danger" disabled={locked || d.row.active} onclick={() => removeZapret2(own)}>
            {S.strategies.delete}
          </button>
        {/if}
      </div>
      <div class="buttons">
        <input placeholder={S.strategies.cloneName} bind:value={cloneName} aria-label={S.strategies.cloneName} />
        <!-- A zapret2 copy starts from the text: a static check error is a reason to copy it. -->
        <button
          disabled={locked || !cloneName.trim() || (z2 === null && d.row.error !== null)}
          onclick={() => clone(d.row.name)}
        >
          {S.strategies.clone}
        </button>
      </div>
      {#if d.row.error}<pre class="bad">{d.row.error}</pre>{/if}
      {#if d.wf}
        <h3>{z2 === null ? S.strategies.filter : S.zapret2.global}</h3>
        <pre>{d.wf}</pre>
      {/if}
      {#each d.blocks as block, i (i)}
        <h3>
          {z2 === null ? S.strategies.block(i + 1) : S.zapret2.profile(i + 1)}
          <span class="muted">· {blockSummary(block)}</span>
        </h3>
        <pre>{block}</pre>
      {/each}
      {#if d.final_args}
        <details>
          <summary>{z2 === null ? S.strategies.finalArgs : S.zapret2.commandLine}</summary>
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

  /* The list gives way before it scrolls sideways: at the minimum window the result column would be cut off. */
  td {
    white-space: normal;
  }
</style>
```

Что изменилось: метка `zapret2` в списке и в заголовке карточки; заголовки «Глобальная часть», «Профиль N», «Командная строка winws2» у стратегий zapret2; у своих стратегий zapret2 — «Изменить» и «Удалить» (активную удалить нельзя, текст подтверждения называет файл); «Клонировать» у стратегии zapret2 открывает редактор с её текстом и доступно, даже если она не прошла статическую проверку.

- [ ] **Шаг 6: Обзор, настройки и прогоны**

`frontend/src/tabs/Overview.svelte`, строку `<h2>{S.overview.service}</h2>` заменить на `<h2>{S.overview.service(o.service_name)}</h2>`; перед карточкой прокси (`<section class="card">` с `<h2>{S.overview.proxy}</h2>`) добавить:

```svelte
    {#if o.zapret2.dir}
      <section class="card">
        <h2>{S.overview.zapret2}</h2>
        {#if o.zapret2.error}
          <pre class="bad">{o.zapret2.error}</pre>
        {:else}
          <p>{S.overview.version(o.zapret2.version)}</p>
        {/if}
      </section>
    {/if}

```

`frontend/src/tabs/Settings.svelte` (по плану CLI; сверить при выполнении), строки импорта

```ts
  import { commands, type FolderCheck, type ProxyStatus, type Settings, type Theme } from "$lib/ipc/bindings";
```

и

```ts
  import { S } from "$lib/strings";
```

заменить на

```ts
  import { commands, type FolderCheck, type Info, type ProxyStatus, type Settings, type Theme } from "$lib/ipc/bindings";
```

и

```ts
  import { LINKS, S } from "$lib/strings";
```

после `let folder = $state<FolderCheck | null>(null);` добавить `let zapret2 = $state<Info | null>(null);`; перед `async function checkProxy(): Promise<void> {` добавить:

```ts
  async function pickZapret2(): Promise<void> {
    const path = await call(S.failed.pick, commands.pickFolder());
    if (path && draft) draft.zapret2_dir = path;
  }

  /** The saved folder's version, or why it cannot be used: a new folder is checked by «Сохранить». */
  async function checkZapret2(): Promise<void> {
    zapret2 = (await call(S.failed.zapret2, commands.zapret2Info())) ?? null;
  }

```

в `save` после `zapret_dir: draft.zapret_dir?.trim() || null,` добавить `zapret2_dir: draft.zapret2_dir?.trim() || null,` (обязательно: пустое поле уходит как `null` и выключает zapret2); в `$effect` после `void checkFolder();` добавить `void checkZapret2();`; `await checkZapret2();` в `save` и правка `onMount` не нужны: после сохранения `follow` отдаёт новую копию, и эффект перечитывает обе папки;

перед карточкой прокси (`<section class="card">` с `<h2>{S.settings.proxy}</h2>`) добавить:

```svelte
  <section class="card">
    <h2>{S.settings.zapret2}</h2>
    <p class="muted">{S.settings.zapret2Hint}</p>
    <div class="row">
      <input
        class="grow"
        bind:value={draft.zapret2_dir}
        placeholder={S.settings.zapret2Example}
        aria-label={S.settings.zapret2}
      />
      <button onclick={pickZapret2}>{S.settings.pick}</button>
    </div>
    {#if zapret2?.dir}
      {#if zapret2.error}
        <pre class="bad">{zapret2.error}</pre>
      {:else}
        <p class="ok">{S.settings.zapret2Ok(zapret2.version, zapret2.compat)}</p>
      {/if}
    {/if}
    <button class="link" onclick={() => call(S.failed.open, commands.openUrl(LINKS.zapret2))}>
      {S.settings.zapret2Releases}
    </button>
  </section>

```

и строку `<!-- One button for the four cards above: they are saved together. -->` заменить на `<!-- One button for the five cards above: they are saved together. -->`.

`frontend/src/tabs/Tests.svelte`, строку

```svelte
        {S.tests.flowseal(run.flowseal_version)}{#if run.canceled}{" · "}{S.tests.canceled}{/if}
```

заменить на

```svelte
        {S.tests.versions(run.flowseal_version, run.zapret2_version)}{#if run.canceled}{" · "}{S.tests.canceled}{/if}
```

Пояснения:
- Карточка «Папка zapret2» сохраняется общей кнопкой «Сохранить», как остальные настройки; `save_settings` проверяет файлы папки, а версию и совместимость Lua после сохранения показывает `zapret2_info`. Пустое поле выключает zapret2.
- Карточка zapret2 на обзоре появляется, только когда папка выбрана: без zapret2 обзор тот же, что в 0.2.0.
- Заголовок «Служба zapret» стал «Служба <имя>»: обзор показывает активную службу из `service_name`.

- [ ] **Шаг 7: Проверки окна**

Run: `npm --prefix frontend run check`
Expected: 0 ошибок, 0 предупреждений.

Run: `npm --prefix frontend run test`
Expected: PASS, 11 тестов.

Run: `npm --prefix frontend run build`
Expected: сборка без ошибок.

- [ ] **Шаг 8: Коммит**

```bash
git add frontend/src/tabs/Zapret2Editor.svelte frontend/src/lib/format.ts frontend/src/lib/format.test.ts frontend/src/lib/strings.ts frontend/src/tabs/Strategies.svelte frontend/src/tabs/Overview.svelte frontend/src/tabs/Settings.svelte frontend/src/tabs/Tests.svelte
git commit -m "feat(ui): add the zapret2 folder, card, editor and badges"
```

---

### Задача 10: README, поправка к core и выпуск 0.3.0

**Files:**
- Modify: `README.md`, `CHANGELOG.md`, `docs/cli.md`, `docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md`, `docs/superpowers/specs/2026-10-11-zapret-hub-cli-design.md`
- Modify: `Cargo.toml` (версия), `Cargo.lock`, `package.json`, `frontend/package.json`, `package-lock.json`

**Interfaces:**
- Consumes: всё, что сделали задачи 1–9; README, `docs/cli.md`, `CHANGELOG.md` и `tools/package.ps1` из плана CLI (задача 5 CLI).
- Produces: раздел «zapret2» в README, запись 0.3.0, поправка к разделу 13 core (спецификация zapret2, 3.1), поправка к разделу 4 спецификации CLI (`start` — единственное, что CLI меняет в системе сам), версия 0.3.0 во всех манифестах; `dist\zapret-hub-0.3.0.zip`.

- [ ] **Шаг 1: README**

Все якоря README из плана CLI (пункт «Командная строка», раздел «Командная строка для агентов») — по плану CLI; сверить при выполнении.

1. В списке возможностей после пункта «**Командная строка:**» (он кончается строкой `  Включается в настройках.`) добавить:

```markdown
- **zapret2:** стратегии [bol-van/zapret2](https://github.com/bol-van/zapret2) (`winws2.exe`, стратегии на Lua)
  рядом со стратегиями Flowseal: те же тесты и рейтинг, служба `zapret2` в один клик, свои стратегии в текстовом
  редакторе.
```

2. В требованиях после `- Папка Flowseal zapret-discord-youtube (проверено на 1.10.3). Приложение её не скачивает.` добавить:

```markdown
- Для zapret2 — распакованный релиз bol-van/zapret2 (по желанию). Его приложение тоже не скачивает.
```

3. В списке файлов приложения после ``- `data\strategies\` — свои стратегии;`` добавить:

```markdown
- `data\zapret2\` — свои стратегии zapret2, по файлу `<имя>.txt`;
```

4. В «Что меняется в системе» после пункта о службе `zapret` (он кончается строкой `  продолжают работать.`) добавить:

```markdown
- Службу `zapret2`, когда применена стратегия zapret2, и имя стратегии в реестре на её ключе. Установлена одна
  служба из двух: применение стратегии одного движка удаляет службу другого.
```

5. В «Тесты стратегий» абзац

```markdown
На время прогона служба zapret останавливается, а winws по очереди запускается с каждой выбранной стратегией;
после прогона служба, ipset и winws возвращаются как были. Стандартный набор — около 15 секунд на стратегию,
все 22 стратегии Flowseal 1.10.3 — около 6 минут. Прогон можно отменить; закрытие окна его не прерывает.
```

заменить на

```markdown
На время прогона служба zapret или zapret2 останавливается, а winws или winws2 по очереди запускается с каждой
выбранной стратегией; после прогона служба, ipset и winws возвращаются как были. Стандартный набор — около
15 секунд на стратегию, все 22 стратегии Flowseal 1.10.3 — около 6 минут. Прогон можно отменить; закрытие окна
его не прерывает.
```

6. Перед `## Командная строка для агентов` новый раздел:

````markdown
## zapret2

[bol-van/zapret2](https://github.com/bol-van/zapret2) — следующий движок автора zapret: `winws2.exe` и
стратегии на Lua. Скачайте релиз со [страницы релизов](https://github.com/bol-van/zapret2/releases), распакуйте
в папку без кириллицы в пути (например `D:\zapret2`) и укажите её в «Настройках» → «Папка zapret2». Подходит и
zapret-win-bundle. Приложение проверит файлы и покажет версию движка или ошибку, если `lua\` не подходит к
движку.

- В списке стратегий после стратегий Flowseal появятся `zapret2:general` и `zapret2:general-sni`. Они собраны
  по примеру из документации zapret2 на списках и фейках Flowseal и живьём не проверены: подберите лучшую
  тестом.
- Свои стратегии — текст аргументов `winws2` в редакторе: «Клонировать» у встроенной, затем «Проверить» и
  «Сохранить». Проверка — статическая, `winws2 --dry-run` и `winws2 --intercept=0`.
- Папка Flowseal нужна и для zapret2: списки, свои списки, режим ipset, фейки и игровой фильтр общие, стратегии
  zapret2 берут их из неё.
- Папку zapret2, как и папку Flowseal, меняют только в окне: `zapret-hub-cli` её не меняет.
- «Применить» на стратегии zapret2 ставит службу `zapret2` и удаляет службу `zapret`; стратегия Flowseal —
  наоборот. Прогон тестов может смешивать стратегии обоих движков.
- `service.bat` и `test zapret.ps1` о zapret2 не знают. Если `service.bat` поставит `zapret` рядом с `zapret2`,
  системная проверка «Другие обходы блокировок» это покажет; следующее «Применить» наведёт порядок.
````

7. В «Удаление» строку

```markdown
Служба zapret и папка Flowseal остаются. Службу удаляет кнопка на вкладке Zapret или `service.bat`.
```

заменить на

```markdown
Служба zapret или zapret2 и папки Flowseal и zapret2 остаются. Службу удаляет кнопка на вкладке Zapret
(службу `zapret` — и `service.bat`).
```

8. В «Что приложение меняет в системе», в пункте о кнопках системных проверок («Кнопки системных проверок, как `service.bat`, удаляют службы GoodbyeDPI, …») вторую строку

```markdown
  WinDivert и WinDivert14.
```

заменить на

```markdown
  WinDivert и WinDivert14, а также службу `zapret2`, если рядом установлена `zapret`.
```

9. В списке возможностей в пункте «**Командная строка:**» его первую строку (вторая, `  Включается в настройках.`, не меняется)

```markdown
- **Командная строка:** `zapret-hub-cli.exe` для ИИ-агентов и скриптов — те же действия, что в окне, вывод в JSON.
```

заменить на

```markdown
- **Командная строка:** `zapret-hub-cli.exe` для ИИ-агентов и скриптов — те же действия, что в окне, кроме смены папок zapret и zapret2, вывод в JSON.
```

- [ ] **Шаг 2: Поправки к core и к спецификации CLI**

`docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md`, раздел 13, пункт

```markdown
- **zapret2 (подпроект 5):** второй движок рядом с Flowseal: своя папка, `winws2`, стратегии на
  Lua. Крейт `zapret` остаётся про Flowseal; общий интерфейс движка (стратегии, запуск, служба,
  тесты) выделяется, когда появится второй движок. В подпроекте 1 поддержки нет.
```

заменить на

```markdown
- **zapret2 (подпроект 5):** второй движок рядом с Flowseal: своя папка, `winws2`, стратегии на
  Lua. В подпроекте 1 поддержки нет.

  Поправка (спецификация zapret2, 3.1): общего интерфейса движка нет, трейт не выделяется.
  - Стратегия zapret2 везде называется `zapret2:<имя>`; в именах `.bat` двоеточия не бывает.
  - `zapret::service::active()` даёт пару (служба, процесс) активного движка: `zapret2` и
    `winws2.exe`, если служба `zapret2` установлена, иначе `zapret` и `winws.exe`.
  - По префиксу ветвятся три места: запуск стратегии в тестере, карточка стратегии, применение.
  - Крейт `zapret` знает второй движок только по именам `zapret2` и `winws2.exe` (единый снимок,
    исключительность служб, быстрая и системные проверки); остальное — в крейте `zapret2`.
  - Трейт выделяется при третьем движке или если веток станет больше пяти.
```

Так core перестаёт обещать то, чего код не делает (спецификация zapret2, 3.1): решение принято, трейта нет, и условие, при котором он появится, записано рядом с исходным обещанием.

`docs/superpowers/specs/2026-10-11-zapret-hub-cli-design.md`, раздел 4, две строки

```markdown
Сам CLI ничего в системе не меняет. Все изменения идут через очередь и снимок приложения
(core 10): zapret по-прежнему меняет один процесс.
```

заменить на

```markdown
Сам CLI ничего в системе не меняет, кроме запуска задачи «Zapret Hub» командой `start` (ниже).
Все изменения идут через очередь и снимок приложения (core 10): zapret по-прежнему меняет один процесс.
```

Так раздел 4 перестаёт противоречить своему следующему абзацу и `docs/cli.md`: `start` — единственное, что CLI делает в системе сам.

- [ ] **Шаг 3: CHANGELOG**

В `CHANGELOG.md` между строкой `Версии по [semver](https://semver.org/lang/ru/).` и `## 0.2.0`:

```markdown
## 0.3.0

- zapret2 (bol-van/zapret2) — второй движок рядом с Flowseal: папка в настройках с проверкой версии и
  совместимости Lua, две встроенные стратегии, свои стратегии в текстовом редакторе с проверкой
  `winws2 --dry-run` и `--intercept=0`, служба `zapret2` в один клик, переключение движков, смешанные прогоны
  тестов. Команды CLI `zapret2 info|validate|save|delete`; папку zapret2, как и папку Flowseal, меняют только в окне.

```

В записи `## 0.2.0` первую строку

```markdown
- `zapret-hub-cli.exe` — командная строка для ИИ-агентов и скриптов: все действия окна, кроме чисто оконных,
```

заменить на

```markdown
- `zapret-hub-cli.exe` — командная строка для ИИ-агентов и скриптов: все действия окна, кроме чисто оконных и смены папки zapret,
```

- [ ] **Шаг 4: Версия 0.3.0**

Заменить `0.2.0` на `0.3.0`:
- `Cargo.toml`, `[workspace.package]`: `version = "0.2.0"` → `version = "0.3.0"`;
- `package.json` и `frontend/package.json`: `"version": "0.2.0",` → `"version": "0.3.0",`;
- `package-lock.json`: три строки `"version": "0.2.0",` — у корня файла, у `packages[""]` и у `packages["frontend"]` (строки 3, 9 и 23);
- `docs/cli.md` (по плану CLI; сверить при выполнении): `{"app": "0.2.0"}` в разделе о `start` и в пункте 10 живой проверки, `{"app": "0.2.0", "cli": "0.2.0", "protocol": 1}` в пункте 3.

Run: `grep -n '0\.2\.0' Cargo.toml package.json frontend/package.json package-lock.json docs/cli.md`
Expected: пусто.

Run: `cargo check --workspace`
Expected: код 0; `Cargo.lock` обновился сам.

Run: `git diff Cargo.lock`
Expected: только версии семи пакетов workspace: `cli`, `exec`, `probe`, `tester`, `zapret`, `zapret-hub`, `zapret2`.

- [ ] **Шаг 5: Автоматические проверки целиком**

В обычном терминале, на ненагруженной машине, по очереди:

Run: `cargo fmt --all -- --check`
Expected: код 0, вывода нет.

Run: `cargo clippy --workspace --all-targets -- -D warnings`
Expected: без замечаний.

Run: `cargo test --workspace --exclude zapret-hub`
Expected: все тесты проходят, среди них `zapret2` (`engine` 8, `service` 3, `strategies` 11), `tester` (`catalog` 2, `history` 4, `rank` 9, `run` 6) и `cli` (8 + 18 + 11); тесты, которым нужны права администратора, служба, `E:\Zapret` или `ZAPRET2_DIR`, перечислены как `ignored`.

Run: `cargo test -p zapret-hub --lib`
Expected: PASS, 18 тестов.

Run: `npm run check`
Expected: 0 ошибок, 0 предупреждений.

Run: `npm test`
Expected: PASS, 11 тестов.

- [ ] **Шаг 6: Сборка архива**

Run: `powershell -NoProfile -ExecutionPolicy Bypass -File tools/package.ps1`
Expected: последняя строка `packed …\dist\zapret-hub-0.3.0.zip`. Релизная сборка с LTO идёт несколько минут.

Run: `dist/zapret-hub-0.3.0/zapret-hub-cli.exe version; echo "exit $?"`
Expected: `"cli": "0.3.0"`, `"protocol": 1`, `exit 0`.

Run: `dist/zapret-hub-0.3.0/zapret-hub-cli.exe zapret2 delete mine; echo "exit $?"`
Expected: `"error": "the window asks to confirm this: add --yes"`, `exit 2`: команда отказывает до отправки.

- [ ] **Шаг 7: Коммит**

```bash
git add README.md CHANGELOG.md docs/cli.md docs/superpowers/specs/2026-10-10-zapret-hub-core-design.md docs/superpowers/specs/2026-10-11-zapret-hub-cli-design.md Cargo.toml Cargo.lock package.json frontend/package.json package-lock.json
git commit -m "build(app): release 0.3.0 with zapret2 as a second engine"
```

Проверка с приложением и настоящим zapret2 — пункт 12 «Проверки вживую» в `docs/cli.md` и `ignored`-тесты с `ZAPRET2_DIR` (`crates/zapret2/tests/engine.rs`, `service.rs`, `crates/tester/tests/run.rs`). Им нужны права администратора и распакованный релиз zapret2, поэтому их проводит пользователь после слияния, а не исполнитель.

---

## Расхождения со спецификацией

Ниже — уточнения там, где спецификация молчит или где план выбрал вариант. Всё это уже в коде плана.

1. **Папка Flowseal нужна и для zapret2 (решение 4, раздел 5.1).** Как в спецификации: каждое изменение идёт через `AppState::change`/`change_safely`, которым нужна открытая папка Flowseal, и список стратегий строится от неё; без неё — прежняя ошибка `the zapret folder is not set: choose it in Settings`. Крейт `zapret2` папку не требует: `tail(engine, None, …)` отказывает только на подстановке Flowseal и называет её.
2. **Когда читается версия (раздел 4).** Не при запуске приложения отдельным шагом, а при первом запросе: обзор, «Настройки», `zapret2_info`, «Применить», тестовый прогон. Запуск из автозагрузки не тратит запуск winws2 впустую, а первый обзор и так приходит в фоновом потоке. Кеш — до смены времени изменения `winws2.exe` или `lua\zapret-lib.lua`: число совместимости берётся из второго, и обновлённый Lua без нового exe тоже должен перечитываться. Ошибка кешируется так же (`ponytail:` в коде: перезапуск приложения читает заново).
3. **Негодная папка zapret2.** Строк zapret2 в списке нет, Flowseal работает как раньше, причина — в «Настройках» и на обзоре. Прогон со стратегиями zapret2 отказывает до снимка с причиной от `Engine::open` (или «the zapret2 folder is not set»); «Применить» и «Проверить» — так же. «Сохранить» в «Настройках» проверяет только файлы папки и кириллицу: папку с Lua другого релиза можно сохранить, несовместимость покажут «Настройки» и обзор, а «Применить» и тест на ней остановятся.
4. **Статическая проверка перед каждой стратегией zapret2 в прогоне.** Раздел 7 её не называет; без неё стратегия, которую нельзя применить, всё равно бы запустилась в тесте. Она получает ошибку запуска с причиной.
5. **«Клонировать» (разделы 5.2, 8).** Кнопка открывает редактор с текстом исходной стратегии под новым именем; записывает только «Сохранить» (`save_zapret2` с `create`). Доступна у любой стратегии zapret2, и у своей, и у той, что не прошла статическую проверку: копию как раз можно поправить. «Удалить» — в карточке своей стратегии, как у своих стратегий Flowseal, с подтверждением.
6. **Сохранение активной стратегии (5.2).** Одна полная проверка в `zapret2::service::save`, затем установка службы той же строкой, под снимком с идентификатором операции `custom` (как у `save_custom`, тексты журнала уже есть), через 2 с — быстрая проверка; ответ — её итог, иначе `null`.
7. **Обзор (раздел 8).** Заголовок карточки службы называет активную службу («Служба zapret2»); версия zapret2 или её ошибка — отдельной карточкой рядом с карточкой Flowseal, только когда папка задана. Тексты быстрой проверки стали нейтральными («Служба не запущена», «Служба запущена, но её процесс не работает»): служба может быть любой из двух.
8. **Голос Discord во встроенных стратегиях.** Пример из документации zapret2 ловит его фильтрами WinDivert из своих файлов, которых у Flowseal нет; встроенные ловят его по портам, как Flowseal. Встроенные проверены только статически (по `functions.txt`) и по путям Flowseal 1.10.3; живьём — `ignored`-тестом пользователя.
9. **Имена в CLI.** `zapret2 save|delete` берут имя без префикса (`mine`), как `custom delete`; `apply`, `strategies show` и `tests run` — полные имена из `strategies list`.

Ограничения, которые следуют из спецификации:
- Мастер первого запуска находит Flowseal по `ImagePath` службы `zapret` и по запущенному `winws.exe`; пока активна `zapret2`, нет ни того, ни другого, и папку выбирают вручную. Касается только пустых настроек.
- Версия 0.2.0, прочитавшая `pending.json` новой версии, не видит полей второго движка и откатывает только Flowseal. Откат делает та версия, что записала файл, если её не заменили посреди операции.
