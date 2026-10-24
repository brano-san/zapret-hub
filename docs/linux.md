# Zapret Hub на Linux

Сборка для Linux x86_64 управляет движком zapret2 (`nfqws2`) через nftables и systemd. Flowseal и WinDivert
на Linux не используются. Порт на Linux начал [Mai-kun](https://github.com/Mai-kun)
([Mai-kun/zapret-hub-linux](https://github.com/Mai-kun/zapret-hub-linux)).

## Установка

Нужны: `webkit2gtk-4.1`, `gtk3`, `libayatana-appindicator3`, `nftables` (`nft`), `curl`, `iputils` (`ping`),
systemd и polkit (`pkexec`); для маршрутизации ещё `iproute2` (`ip`) и `setcap` (`libcap2-bin` в Debian/Ubuntu,
`libcap` в Arch).

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
  `docs/cli.md`. Для Linux есть `service`, `nft`, `dns-flush`, `exclusions`, `probe`, `tg-ws`, `routes grant`.
- **Пути XDG.** Настройки: `$XDG_CONFIG_HOME/zapret-hub` (`~/.config/zapret-hub`); данные, журналы, свои
  стратегии и история тестов: `$XDG_DATA_HOME/zapret-hub` (`~/.local/share/zapret-hub`). Автозапуск хаба —
  файл `.desktop` в `~/.config/autostart`.
- **TG WS Proxy.** Бинарник `tg-ws-proxy` ([valnesfjord/tg-ws-proxy-rs](https://github.com/valnesfjord/tg-ws-proxy-rs))
  качается кнопкой, SHA-256 проверяется. Работает как пользовательский юнит `zapret-hub-tg-ws.service`
  (`systemctl --user`, файл в `~/.config/systemd/user`), без root; журнал — `journalctl --user -u zapret-hub-tg-ws`.
- **Журнал службы:** вкладка «Инструменты» и `zapret-hub-cli service logs` (`journalctl -u zapret2`).
- **Системные проверки** на Linux: состояние службы, бинарник и права `nfqws2`, фейки `bin/*.bin`, списки, очередь
  netfilter, таблица nft и её защита от петли, чужие обходы и таблицы.

## Маршрутизация

Правила `routes.toml` (протокол, порты, подсети, домены, программы) работают как на Windows: совпавший TCP и UDP
идёт через локальный SOCKS5-прокси, остальное — напрямую. Вкладка «Маршруты», команды `router` и `routes`,
счётчики и потоки те же. Файл правил — `~/.local/share/zapret-hub/routes.toml`, программы в правилах — по имени
(`discord`) или полному пути (`/usr/bin/discord`).

**Права.** Хабу нужны `CAP_NET_ADMIN` и `CAP_NET_RAW`. Без них вкладка «Маршруты» показывает карточку с кнопкой
«Разрешить маршрутизацию» (или `zapret-hub-cli routes grant`): один запрос пароля выполняет

```
setcap cap_net_admin,cap_net_raw+eip <путь к zapret-hub>
```

после чего хаб нужно перезапустить («Выход» в трее и запуск снова). Проверить: `getcap <путь к zapret-hub>`.
Обновление хаба (новый файл) снимает права — нажмите кнопку снова. Права файла действуют для любого, кто его
запускает: держите хаб в папке, куда не пишут другие пользователи. Права `setcap` не работают на AppImage
(squashfs только для чтения) и на разделах с `nosuid`: распакуйте архив туда, где их нет.

**Как это устроено.**

- Таблица `inet zapret_hub_route`: TCP по правилам перенаправляется (`redirect`) на локальный приёмник хаба,
  UDP получает метку `0x10000000` и через `ip rule fwmark 0x10000000 lookup 100` и маршрут `local 0.0.0.0/0 dev lo
  table 100` попадает в сокет TPROXY хаба. Хаб узнаёт исходный адрес и программу (`/proc`), решает по правилам и
  ведёт поток через SOCKS5 (`CONNECT`, `UDP ASSOCIATE`) или напрямую.
- Свои соединения хаба помечены `0x20000000` и в таблицу не попадают. Мимо неё идут также `lo`, адрес самого
  SOCKS5-прокси, многоадресный и широковещательный UDP и процессы группы `zapret-bypass` (если она есть; см. выше)
  — например, сам SOCKS5-прокси, если он ходит на адреса из правил.
- Домены: хаб читает ответы DNS сокетом `AF_PACKET` на всех интерфейсах (в том числе `lo`, где отвечает
  `systemd-resolved`) и добавляет адреса совпавших имён в таблицу на время TTL.
- Правила меняются одной транзакцией `nft -f`: открытые соединения не рвутся.
- Маршрутизирует один процесс: хаб или `zapret-hub-cli` через хаб; вторая копия получает ошибку.

**Очистка.** Выключение маршрутизации, «Выход», завершение сеанса и сигналы `SIGTERM`/`SIGINT`/`SIGHUP` удаляют
таблицу, правила `ip rule` и маршрут таблицы 100. Если хаб упал, всё это удаляется при следующем запуске (даже
с выключенной маршрутизацией, если права выданы). Вручную:

```
sudo nft delete table inet zapret_hub_route
while sudo ip -4 rule del fwmark 0x10000000/0x10000000 lookup 100 2>/dev/null; do :; done
sudo ip -4 route flush table 100
```

**Ограничения.**

- Только IPv4, как на Windows: IPv6 идёт напрямую.
- Правила по доменам видят только обычный DNS (UDP 53). DNS over TLS/HTTPS в `systemd-resolved` или в браузере
  ответов не показывает — такие правила не сработают; вкладка предупреждает, если в `resolved.conf` включён DNS over TLS.
- Только исходящий трафик этого компьютера, не других машин сети.
- Соединения, открытые до включения правила, идут как шли.

## Чего нет на Linux

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
