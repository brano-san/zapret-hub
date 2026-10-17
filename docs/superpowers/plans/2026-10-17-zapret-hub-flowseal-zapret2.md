# Стратегии Flowseal на zapret2 — план реализации

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** все 22 стратегии Flowseal `general*.bat` встроены в хаб как стратегии zapret2, проверены тестом хаба против оригинала и (пять лучших) голосом Discord.

**Architecture:** тексты копий — файлы `crates/zapret2/src/builtin/flowseal/*.txt`, встроенные `include_str!` во второй реестр `FLOWSEAL` рядом с `BUILTIN`; строка стратегии получает признак `flowseal_copy`, по нему копии не попадают в прогон тестов по умолчанию (CLI и окно). Перевод `.bat` → текст zapret2 делается вручную по образцу `zapret2:ALT11`; приёмка — прогоны пар через CLI работающего хаба.

**Tech Stack:** Rust (crates `zapret2`, `tester`, `cli`), Svelte 5, zapret2 1.0.5.2 (Lua desync).

**Spec:** `docs/superpowers/specs/2026-10-17-zapret-hub-flowseal-zapret2-design.md`

## Global Constraints

- Источник: Flowseal 1.10.3, папка `E:\Zapret` — только чтение; zapret2 `D:\zapret2-dl\zapret2-v1.0.5.2` — только чтение.
- 22 копии: все `general*.bat`, кроме `general (HUB …).bat`; `service.bat`, `discord-udp-proxy.bat` — не стратегии.
- Имена: `zapret2:<имя Flowseal>`; `general.bat` → `zapret2:general (Flowseal)`.
- Шапка каждой копии — комментарии `#`: `From Flowseal/zapret-discord-youtube 1.10.3, <файл>.bat (MIT License, Copyright (c) Flowseal)`, что изменено при переносе; при «хуже на тесте» — строка об этом.
- Голос и STUN — по портам в `--wf-udp-out` (`19294-19344,50000-50100`), без `--wf-raw-part`.
- `%BIN%`, `%LISTS%` — папка Flowseal; `%FILES%` — папка zapret2; `%GameFilter…%` — как сейчас.
- Встроенные — только для чтения; копия встроенной получает имя без скобок: `ALT5 copy` для `general (ALT5)`.
- Копии Flowseal не входят в прогон без имён (`tests run` в CLI) и в выбор окна «Тесты» по умолчанию.
- Протокол CLI: 3 → 4 (строка стратегии получает поле).
- Версия 0.7.0. Коммиты: одна строка, Conventional Commits, без трейлеров. Документация — по-русски.
- Агент не повышает права, не запускает winws/winws2 сам, не трогает систему; прогоны — только через `zapret-hub-cli` работающего хаба.

## Review Focus

1. Копия, которая не проходит статическую проверку zapret2 (`strategies::check`), — в набор не входит: тест на все встроенные копии.
2. Имя своей стратегии, совпадающее с копией, — пропускается с пояснением (уже так; тест сохраняется).
3. Копия встроенной без скобок в имени: окно предлагает `ALT5 copy`, имя проходит `validate_name`.
4. `tests run` без имён и выбор окна по умолчанию — без копий Flowseal; с именем копии — запускается.
5. Старый CLI (протокол 3) с новым хабом — отказ с понятной ошибкой.

---

### Task 1: Реестр копий Flowseal и исключение из прогона по умолчанию

**Files:**
- Create: `crates/zapret2/src/builtin/flowseal/general (ALT11).txt` (первая копия — из `D:\zapret2-dl\zapret2-v1.0.5.2\strategies\ALT11.txt`, переведённая на порты: голос/STUN в `--wf-udp-out`, без `--wf-raw-part`, шапка по Global Constraints)
- Modify: `crates/zapret2/src/strategies.rs` (`BUILTIN`, новый `FLOWSEAL`, `Strategy`), `crates/tester/src/catalog.rs` (`StrategyRow`), `crates/cli/src/lib.rs` (`PROTOCOL = 4`), `crates/cli/src/main.rs` (`runnable`), `frontend/src/tabs/Tests.svelte` (`usable`), `frontend/src/tabs/Strategies.svelte` (имя копии), `frontend/src/lib/ipc/bindings.ts` (регенерация)
- Test: `crates/zapret2/tests/strategies.rs`, `crates/cli/src/main.rs` (тесты модуля), `frontend/src/lib/format.test.ts` или ближайший тест имени копии

**Interfaces:**
- Produces: `zapret2::strategies::FLOWSEAL: [(&str, &str); N]` (имя без префикса, текст); `Strategy { name, text, builtin, flowseal_copy: bool }`; `StrategyRow.flowseal_copy: bool` (DTO); `is_builtin(name)` учитывает обе таблицы; список встроенных: `general`, `general-sni`, затем копии Flowseal в порядке `zapret::strategies::natural_key`.

- [ ] **Step 1: Тесты.** В `crates/zapret2/tests/strategies.rs`:
  - `without_a_folder_there_are_the_built_in_ones`: первые два — `("general", true)`, `("general-sni", true)`, далее есть `("general (ALT11)", true)`.
  - `every_built_in_one_passes_the_check`: для каждой встроенной `check(&engine, &tail(...))` — `Ok`, шапка содержит `MIT` и одно из `bol-van`/`Flowseal`; у копий Flowseal `flowseal_copy == true`, у `general`/`general-sni` — `false`. Заменяет утверждения `(1..=3).contains(&builtin.len())` и «bol-van» для всех.
  - `a_file_named_like_a_flowseal_copy_is_skipped`: файл `general (ALT11).txt` в папке пользователя — в `skipped`, не в `list`.
  - В CLI: `runnable` пропускает строки с `flowseal_copy: true`.
- [ ] **Step 2:** `cargo test -p zapret2 --test strategies` и `cargo test -p cli` — новые тесты падают.
- [ ] **Step 3:** Реализация: `FLOWSEAL` через `include_str!("builtin/flowseal/general (ALT11).txt")`; `builtin()` выдаёт `BUILTIN`, затем `FLOWSEAL`; `flowseal_copy` проставляется из таблицы; `catalog::zapret2_row` переносит его в `StrategyRow`; `runnable` фильтрует `!row.flowseal_copy`; `Tests.svelte` `usable` — `row.error === null && !row.flowseal_copy`; `Strategies.svelte` имя копии — `uniqueName(\`${stem} copy\`)`, где `stem` для имени со скобками — текст внутри скобок (`general (ALT5)` → `ALT5`), иначе имя без префикса; `PROTOCOL = 4` (тест отказа старого протокола — на 3); `cargo run -p zapret-hub --bin export-bindings`.
- [ ] **Step 4:** `cargo test --workspace`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`, `npm run check -w frontend -- --output human` (0/0), `npm --prefix frontend test` — зелёные.
- [ ] **Step 5:** Commit `feat(zapret2): built-in Flowseal copies, left out of default test runs`.

### Task 2: Перевод стратегий Flowseal, часть 1

**Files:**
- Create: `crates/zapret2/src/builtin/flowseal/` — `general (Flowseal).txt` (из `general.bat`), `general (ALT).txt`, `general (ALT2).txt` … `general (ALT10).txt`, `general (ALT12).txt`, `general (ALT13).txt`
- Modify: `crates/zapret2/src/strategies.rs` (`FLOWSEAL` — добавить файлы)

**Interfaces:**
- Consumes: `FLOWSEAL`, тест `every_built_in_one_passes_the_check` (Task 1).

- [ ] **Step 1:** Для каждого `.bat`: каждый профиль (`--new`) → профиль zapret2: `--filter-*`, списки и `--ip-id` — как есть; `--wf-tcp`/`--wf-udp` → `--wf-tcp-out`/`--wf-udp-out`; `--dpi-desync=<приёмы>` с `--dpi-desync-*` → `--payload=… --lua-desync=…` по функциям `lua/zapret-antidpi.lua` (fake, multisplit, multidisorder_legacy для `multidisorder`, fakedsplit, hostfakesplit, syndata, `tcp_seq`/`tcp_md5`/`ts` для fooling, `--out-range=-d<N>` для cutoff, `repeats=`); блобы — `--blob=<имя>:@"%BIN%<файл>"`; образец — копия `general (ALT11)` из Task 1. Шапка — по Global Constraints, со списком решений перевода.
- [ ] **Step 2:** `cargo test -p zapret2 --test strategies` — `every_built_in_one_passes_the_check` проходит для всех добавленных.
- [ ] **Step 3:** Commit `feat(zapret2): Flowseal general and ALT strategies on zapret2`.

### Task 3: Перевод стратегий Flowseal, часть 2

**Files:**
- Create: `crates/zapret2/src/builtin/flowseal/` — `general (EXP).txt`, `general (FAKE TLS AUTO).txt`, `general (FAKE TLS AUTO ALT).txt`, `general (FAKE TLS AUTO ALT2).txt`, `general (FAKE TLS AUTO ALT3).txt`, `general (SIMPLE FAKE).txt`, `general (SIMPLE FAKE ALT).txt`, `general (SIMPLE FAKE ALT2).txt`
- Modify: `crates/zapret2/src/strategies.rs`

- [ ] **Step 1:** Перевод — как Task 2, Step 1. `FAKE TLS AUTO` — `tls_mod` для автоматического фейка TLS (как `fake_default_tls = tls_mod(...)` во встроенной `general`).
- [ ] **Step 2:** `cargo test -p zapret2 --test strategies`; в `without_a_folder_there_are_the_built_in_ones` — всего 24 встроенные (2 + 22).
- [ ] **Step 3:** Commit `feat(zapret2): Flowseal EXP, FAKE TLS AUTO and SIMPLE FAKE strategies on zapret2`.

### Task 4: Приёмка на живом хабе (контроллер, не исполнитель)

Делает контроллер с пользователем: исполнителю без работающего хаба нечего запускать.

- [ ] **Step 1:** Сборка `tools/package.ps1`; пользователь закрывает хаб; exe в `C:\Users\brano\Downloads` заменяется (старые — в `zapret-hub-old-bak`); `zapret-hub-cli start`; `version` → протокол 4. Снимок: `status` (стратегия), `quick-check`.
- [ ] **Step 2:** Для каждой из 22 пар: `zapret-hub-cli zapret2 validate` текста копии → код 0; `tests run --wait "<оригинал>" "zapret2:<копия>"`; сравнение по правилу спецификации (раздел 4); пограничный — `--repeats 2`. Хуже → до трёх правок перевода (коммит `fix(zapret2): …` на копию, пересборка, повтор пары). Не запускается → копия убирается из набора.
- [ ] **Step 3:** `docs/flowseal-zapret2.md` — таблица приёмки (стратегия; `ok` и медианы по discord/youtube/dbd у оригинала и копии; вердикт; что правилось; id прогонов).
- [ ] **Step 4:** Голос Discord у пяти лучших копий по рейтингу: `apply "zapret2:<копия>"`, пользователь в голосовом канале отвечает «работает / нет»; итог — в таблицу. После — `apply` исходной стратегии пользователя, `status` и `quick-check` как в Step 1.
- [ ] **Step 5:** Commit `docs: acceptance of the Flowseal copies on zapret2`.

### Task 5: Документация и версия

**Files:**
- Modify: `README.md` (раздел zapret2: встроенные копии Flowseal, имена, источник, не в прогоне по умолчанию), `docs/cli.md` (протокол 4, `tests run` без имён — без копий, пример имени; чек-лист: пункт о копии и пункт о протоколе), `docs/superpowers/specs/2026-10-11-zapret-hub-zapret2-design.md` (поправка: встроенные копии Flowseal), `docs/superpowers/specs/2026-10-17-zapret-hub-diag-design.md` (протокол CLI диагностики — 5), `CHANGELOG.md`, `Cargo.toml`, `package.json`, `frontend/package.json`, `package-lock.json`, `Cargo.lock` (0.7.0)

- [ ] **Step 1:** Правки текста; версии — через инструменты (`cargo`, `npm version --no-git-tag-version`), не руками в lock-файлах.
- [ ] **Step 2:** Полный набор проверок из Task 1, Step 4.
- [ ] **Step 3:** Commit `docs: Flowseal strategies on zapret2; version 0.7.0`.
