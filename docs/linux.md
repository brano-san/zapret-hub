# Zapret Hub на Linux

Сборка для Linux x86_64 управляет движком zapret2 (`nfqws2`) через nftables и systemd. Flowseal и WinDivert
на Linux не используются. Порт на Linux начал [Mai-kun](https://github.com/Mai-kun)
([Mai-kun/zapret-hub-linux](https://github.com/Mai-kun/zapret-hub-linux)).

## Установка

Нужны: `webkit2gtk-4.1`, `gtk3`, `libayatana-appindicator3`, `nftables` (`nft`), `curl`, `iputils` (`ping`),
systemd и polkit (`pkexec`).

```
# Debian/Ubuntu
sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev libayatana-appindicator3-dev nftables curl iputils-ping policykit-1
# Arch
sudo pacman -S webkit2gtk-4.1 gtk3 libayatana-appindicator nftables curl iputils polkit
```

Для сборки из исходников — ещё Rust (версию задаёт `rust-toolchain.toml`), Node.js 24 LTS и
`build-essential`/`base-devel`:

```
npm install
npm run tauri build
cargo build --release -p cli
```

Запускайте `zapret-hub` от обычного пользователя: права root хаб просит сам, только на нужные действия.
Движок zapret2 (`binaries/linux-x86_64/nfqws2`) скачивается кнопкой «Скачать последний релиз…» в мастере и в
«Настройках»; «Найти автоматически» ищет уже скачанный.

### Группа `zapret-bypass`

Процесс из группы `zapret-bypass` идёт мимо очереди nfqws2, то есть без обхода (например, VPN или клиент, который
сам умеет обходить блокировки):

```
sudo groupadd -f zapret-bypass
sudo usermod -aG zapret-bypass $USER   # затем выйти и войти в сессию
sg zapret-bypass "<команда>"
```

### Запросы пароля: pkexec и sudo

Запись в `/etc` и управление службой требуют root. Хаб пробует по порядку: уже root, `sudo -n` (без пароля), затем
`pkexec` с графическим запросом. Одно «Применить» стратегии собирается в один скрипт, то есть один запрос на
применение. Прогон тестов ставит одну таблицу nftables на весь прогон, но без правила `sudo` спрашивает
несколько раз: остановка служб, таблица, её снятие и возврат службы — отдельные шаги.

Чтобы запросов не было, дайте своему пользователю правило `sudo`. Каждый шаг хаб выполняет как `sh -c <скрипт>`,
поэтому правило с `/bin/sh` (как и правило polkit ниже) — это root без пароля для любой программы, запущенной
от вашего имени. Давайте его, только если это вас устраивает; хаб правил не ставит:

```
# /etc/sudoers.d/zapret-hub  (visudo -f, права 0440)
USER ALL=(ALL) NOPASSWD: /usr/sbin/nft, /usr/bin/systemctl, /usr/bin/journalctl, /bin/sh, /usr/bin/tee, /bin/rm
```

Пути зависят от дистрибутива (`command -v nft`: в Debian/Ubuntu `/usr/sbin/nft`). Список зависит от того, что в вашем дистрибутиве делает скрипт применения (см. журнал `data/logs`: там видны
команды). Для `pkexec` аналог — правило polkit в `/etc/polkit-1/rules.d/` с `polkit.Result.YES` для вашей группы и
действия `org.freedesktop.policykit.exec`.

## Как это устроено

- **nftables.** Хаб пишет `/etc/zapret-hub/nfqws2.nft` и ставит таблицу `inet zapret_hub`: исходящие пакеты
  портов стратегии уходят в очередь `200` (`--qnum=200`). Порты берутся из `--wf-tcp`/`--wf-udp` стратегии
  (плюс порты Discord голоса 50000-65535 во встроенных) за вычетом исключений. Трафик с меткой `0x40000000`,
  `lo` и группа `zapret-bypass` не перехватываются; пакеты nfqws2 помечены, чтобы не зациклиться.
- **systemd.** Служба `zapret2.service` в `/etc/systemd/system`: пользователь — тот, кто применял стратегию, с
  `AmbientCapabilities=CAP_NET_ADMIN CAP_NET_RAW`; `ExecStartPre=+nft -f …nfqws2.nft` ставит таблицу,
  `ExecStopPost=+nft delete table inet zapret_hub` убирает. Аргументы стратегии — в `ExecStart`. Автозапуск
  (`enable`) включает «Применить».
- **CLI.** `zapret-hub-cli` общается с приложением через Unix-сокет `$XDG_RUNTIME_DIR/zapret-hub.sock` (права
  `0600`); без `XDG_RUNTIME_DIR` — `/tmp/zapret-hub-<uid>/`. Протокол и команды те же, что на Windows, см.
  `docs/cli.md`. Для Linux есть `service`, `nft`, `dns-flush`, `exclusions`, `probe`, `tg-ws`.
- **Пути XDG.** Настройки: `$XDG_CONFIG_HOME/zapret-hub` (`~/.config/zapret-hub`); данные, журналы, свои
  стратегии и история тестов: `$XDG_DATA_HOME/zapret-hub` (`~/.local/share/zapret-hub`). Автозапуск хаба —
  файл `.desktop` в `~/.config/autostart`.
- **TG WS Proxy.** Бинарник `tg-ws-proxy` ([valnesfjord/tg-ws-proxy-rs](https://github.com/valnesfjord/tg-ws-proxy-rs))
  качается кнопкой, SHA-256 проверяется. Работает как пользовательский юнит `zapret-hub-tg-ws.service`
  (`systemctl --user`, файл в `~/.config/systemd/user`), без root; журнал — `journalctl --user -u zapret-hub-tg-ws`.
- **Журнал службы:** вкладка «Инструменты» и `zapret-hub-cli service logs` (`journalctl -u zapret2`).
- **Системные проверки** на Linux: состояние службы, бинарник и права `nfqws2`, фейки `bin/*.bin`, списки, очередь
  netfilter, таблица nft и её защита от петли, чужие обходы и таблицы.

## Чего нет на Linux

- **Маршрутизация через SOCKS5** (роутер, WinDivert) — только на Windows; на вкладке «Маршруты» на Linux только
  исключения. Пресеты `routes preset cloudflare|telegram|twitch` — для маршрутизации.
- **Наблюдение за потоками в «Диагностике»:** программа не видит потоки и DNS программы, поэтому вердикт не выше
  «неясно» (`inconclusive`), причины, зависящие от потоков, не выносятся.
- Реестр, Планировщик, Windows-уведомления, очистка кэша Discord Windows-путями. TCP timestamps на Linux
  включаются как в Windows: `net.ipv4.tcp_timestamps=1` при установке службы и перед тестами.
- Только `linux-x86_64`; другие архитектуры хаб не скачает.
- Только systemd: OpenRC, runit и другие не поддерживаются.

## Если что-то не работает

- **Служба не стартует, в журнале «Operation not permitted»:** проверьте `getcap`/`AmbientCapabilities` в юните
  (системная проверка «Права nfqws2») и что модуль `nfnetlink_queue` загружается (`sudo modprobe nfnetlink_queue`).
- **Трафик не идёт после «Применить»:** `sudo nft list table inet zapret_hub`. Если таблица есть, а служба
  остановлена, остановите обход кнопкой «Остановить» или `sudo nft delete table inet zapret_hub`.
- **Другой обход рядом** (`zapret`, `nfqws`, чужие таблицы nft): системная проверка «Другие обходы» покажет их;
  не запускайте их вместе с хабом, очередь `200` занимает один процесс. Юнит `zapret` bol-van/zapret хаб не
  отключает и не удаляет: «Удалить службы» его только останавливает, прогон тестов запускает его снова, если он
  работал. Удалите его сами (`uninstall_easy.sh` в его папке).
- **«No authentication agent»:** нет агента polkit в сессии. Запустите агент (`polkit-gnome-authentication-agent-1`
  и т. п.) или дайте правило `sudo`/polkit, как выше.
- **Пустое окно или артефакты прокрутки:** хаб сам ставит `WEBKIT_DISABLE_DMABUF_RENDERER=1`; если мешает ещё,
  запустите с `WEBKIT_DISABLE_COMPOSITING_MODE=1`.
- **Нет значка в трее** (GNOME): нужно расширение AppIndicator.
- **DNS:** «Сбросить DNS» (`dns-flush`) чистит кэш `resolvectl`, `systemd-resolve` или `nmcli`, если они активны.
- **Тест без пароля.** Каждый запрос root в прогоне тестов один на запуск таблицы; без `sudo -n` ждите диалог
  `pkexec`.
