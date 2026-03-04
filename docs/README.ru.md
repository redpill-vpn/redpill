[English](../README.md) | **Русский** | [فارسی](README.fa.md) | [العربية](README.ar.md) | [中文](README.zh-CN.md)

# RedpillVPN

Быстрый VPN с обходом цензуры на базе QUIC. Написан на Rust.

> **Статус: MVP / ранняя альфа.** Базовое туннелирование работает и протестировано на macOS/Linux, но возможны шероховатости, ломающие изменения и недостающий функционал. См. [Roadmap](#roadmap).

Туннелирует сырые IP-пакеты через QUIC DATAGRAM фреймы (RFC 9221) по UDP:443, TLS 1.3. Внешний QUIC использует no-op congestion controller (фиксированное окно 16 MB), т.к. внутренний TCP сам управляет перегрузкой. Поддерживает несколько транспортных режимов для разных сетевых условий.

## Возможности

### Транспорт
- QUIC DATAGRAM фреймы (RFC 9221) поверх UDP:443
- TLS 1.3 через quinn/rustls, самоподписанные сертификаты (генерируются автоматически при первом запуске)
- No-op congestion control (константное окно 16 MB) - внутренний TCP сам управляет CC
- Батчинг датаграмм для мелких пакетов (<300 B: DNS-запросы, TCP ACK)
- Динамический MTU - сервер мониторит PMTU и пушит обновления клиенту в реальном времени

### Безопасность
- PSK-аутентификация (HMAC-SHA256, constant-time проверка)
- Мультипользовательский режим - у каждого свой PSK (`add-user` / `remove-user` через CLI)
- Валидация source IP против спуфинга
- Kill-switch на macOS (pf), Windows (Windows Firewall) - предотвращает утечки при отключении
- HTTP/3 decoy-сервер (фейковая страница nginx для защиты от active probing)

### Обход цензуры
- **5 транспортных режимов** - прямой QUIC, QUIC + камуфляж, TCP Reality, WebSocket CDN, auto
- SNI-камуфляж с round-robin ротацией доменов
- Мимикрия под TLS-отпечатки браузеров - профили Chrome, Firefox, Safari (устойчивость к JA3/JA4)
- Нормализация размеров пакетов до стандартных HTTP/3 размеров (128 / 256 / 512 / 1024 / 1200 / 1400)
- Паддинг idle-трафика (dummy-пакеты в периоды неактивности)
- TCP Reality - не-VPN TLS-соединения прозрачно проксируются на реальный сайт
- WebSocket CDN - туннель через CDN reverse proxy (Cloudflare и т.д.)

### Сервер
- Несколько клиентов с per-client очередями приоритетов и выделением IP (10.0.1.2-254, до 253 клиентов)
- Приоритизация трафика - realtime (мелкий UDP, DNS, DSCP EF) обрабатывается раньше bulk
- Серверный backpressure для предотвращения loss amplification
- Адаптивный traffic shaping на основе RTT + per-client ограничение полосы
- XDP conntrack bypass для высокой производительности при большом потоке пакетов (Linux, опционально)
- Prometheus-эндпоинт метрик (`/metrics`)
- SIGHUP hot-reload (PSK, пользователи, max_connections, decoy-страница, уровень логирования)
- NAT masquerade + MSS clamping (автоматически)

### Клиент
- Автореконнект с экспоненциальным backoff (1s-30s) и jitter
- Мониторинг здоровья соединения с автоматическим fallback и upgrade транспорта
- Режим демона (Linux/macOS) - команды `up` / `down` / `status` через IPC
- Очистка устаревшего состояния при запуске (маршруты, DNS, правила файрвола от упавших сессий)

## Поддерживаемые платформы

| Роль   | ОС                    | Архитектура   |
|--------|-----------------------|---------------|
| Сервер | Linux                 | x86_64, arm64 |
| Клиент | macOS, Linux, Windows | arm64, x86_64 |

> **Примечание:** Windows-клиент компилируется и включает полную поддержку wintun/routing/firewall, но пока не тестировался на реальном железе. Клиенты на macOS и Linux полностью протестированы.

---

## Быстрый старт

### 1. Сборка

```bash
git clone https://github.com/redpill-vpn/redpill.git
cd redpill
cargo build --release
```

Бинарники появятся в `target/release/`:
- `redpill-server` - VPN-сервер
- `redpill-client` - VPN-клиент

Опциональные фичи сборки:

```bash
cargo build --release --features xdp    # XDP conntrack bypass (только Linux-сервер)
cargo build --release --features acme   # ACME/Let's Encrypt заглушка
```

### 2. Генерация PSK

```bash
openssl rand -hex 32
# Пример вывода: a3f7b2c1d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0
```

Сохраните эту 64-символьную hex-строку - она понадобится и для сервера, и для клиента.

### 3. Настройка сервера

```bash
# Создать директорию конфига
sudo mkdir -p /etc/redpill

# Скопировать пример конфига
sudo cp config/server.example.toml /etc/redpill/server.toml

# Сохранить PSK
echo "YOUR_64_CHAR_HEX_PSK" | sudo tee /etc/redpill/psk

# Установить бинарник
sudo cp target/release/redpill-server /usr/local/bin/

# Запустить сервер
sudo redpill-server -c /etc/redpill/server.toml
```

**Важно:** Отредактируйте `/etc/redpill/server.toml` и укажите `nat.interface` - WAN-интерфейс вашего сервера (напр. `ens1`, `eth0`). Узнать его можно через `ip route show default`.

При первом запуске сервер автоматически генерирует самоподписанный TLS-сертификат по путям из конфига (`cert_file` / `key_file`). **Скопируйте файл сертификата на клиентскую машину.**

### 4. Настройка клиента

#### Просто (через CLI-флаги)

```bash
sudo redpill-client \
  --server YOUR_SERVER_IP:443 \
  --cert path/to/cert.pem \
  --psk YOUR_64_CHAR_HEX_PSK
```

#### С конфиг-файлом (рекомендуется)

```bash
sudo cp config/client.example.toml ~/client.toml
# Отредактируйте ~/client.toml - укажите адрес сервера, путь к сертификату и PSK
sudo redpill-client --config ~/client.toml
```

#### Тестовый режим (без маршрутов, существующее соединение сохраняется)

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk <hex> --test-mode
```

В тестовом режиме туннель устанавливается, но системные маршруты и DNS не меняются. Удобно для бенчмарков iperf3 или отладки.

### 5. Проверка

```bash
# Должен показать IP вашего сервера
curl -s https://api.ipify.org

# Пинг через туннель
ping 10.0.1.1
```

---

## Управление пользователями

Вместо одного общего PSK можно дать каждому пользователю свой ключ.

### Включение мультипользовательского режима

Добавьте `users_dir` в конфиг сервера:

```toml
users_dir = "/etc/redpill/users"
```

### Добавление пользователя

```bash
# Сгенерировать случайный PSK для пользователя
openssl rand -hex 32 | sudo tee /etc/redpill/users/alice.key
```

Передайте `alice` её PSK и сертификат сервера. Она подключается так:

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk $(cat alice.key)
```

### Удаление пользователя

```bash
sudo rm /etc/redpill/users/alice.key
sudo kill -HUP $(pidof redpill-server)   # Перезагрузка без рестарта
```

### Список пользователей

```bash
ls /etc/redpill/users/
# alice.key  bob.key  charlie.key
```

Каждый файл `.key` содержит 64-символьный hex PSK. Имя файла (без `.key`) - это имя пользователя, которое отображается в логах и метриках.

> **Обратная совместимость:** Если `psk_file` тоже указан, его ключ принимается как fallback (в логах отображается как пользователь `legacy`). Для миграции: скопируйте старый PSK в `users_dir/legacy.key` и уберите `psk_file`.

---

## Транспортные режимы

RedpillVPN поддерживает 5 транспортных режимов. Выбирайте подходящий под вашу сеть:

| Режим | Протокол | Когда использовать |
|-------|----------|-------------------|
| `quic` | Прямой QUIC DATAGRAM через UDP:443 | По умолчанию. Лучшая производительность в неограниченных сетях |
| `quic-camouflaged` | QUIC + SNI-ротация + паддинг + браузерный fingerprint | Сети с лёгким DPI (фильтрация по SNI) |
| `tcp-reality` | TLS-over-TCP с отражением active probing | QUIC заблокирован, TCP+TLS ещё работает |
| `websocket` | Бинарные WebSocket-фреймы через CDN | Работает только доступ через CDN |
| `auto` | Пробует все транспорты, выбирает лучший | Рекомендуется для цензурируемых или незнакомых сетей |

### Настройка транспортов

Выбор транспорта делается в конфиге клиента (секция `[transport]`):

#### Прямой QUIC (по умолчанию)

```toml
[server]
address = "1.2.3.4:443"
cert = "cert.pem"
psk = "your-psk-hex"

[transport]
mode = "quic"
```

#### QUIC Camouflaged

Маскирует ваш QUIC-трафик под обычный браузерный HTTPS:

```toml
[transport]
mode = "quic-camouflaged"

[camouflage]
# Домены для ротации (поле SNI в ClientHello)
sni_pool = ["dl.google.com", "www.google.com", "fonts.gstatic.com", "www.youtube.com"]
# Паддинг пакетов до стандартных HTTP/3 размеров
padding = true
# Мимикрия под TLS-отпечаток реального браузера
chrome_fingerprint = true
# Браузер для мимикрии: "chrome", "firefox", "safari" или "random"
browser_profile = "chrome"
```

#### TCP Reality

Когда QUIC/UDP полностью заблокирован. Сервер принимает TCP+TLS на отдельном порту и перенаправляет не-VPN пробы на реальный сайт:

Конфиг сервера:
```toml
[reality]
enabled = true
listen = "0.0.0.0:8443"
target = "www.google.com:443"   # Пробы видят этот реальный сайт
```

Конфиг клиента:
```toml
[transport]
mode = "tcp-reality"

[reality]
target = "www.google.com:443"
address = "1.2.3.4:8443"       # Reality-порт сервера
```

#### WebSocket CDN

Маршрутизация трафика через CDN (напр. Cloudflare) для скрытия IP сервера:

Конфиг сервера:
```toml
[websocket]
enabled = true
listen = "127.0.0.1:8080"      # За CDN reverse proxy
```

Конфиг клиента:
```toml
[transport]
mode = "websocket"

[websocket]
url = "wss://cdn.example.com/ws"
host = "cdn.example.com"
```

#### Auto-режим (рекомендуется для цензурируемых сетей)

Пробует транспорты в порядке приоритета и берёт первый рабочий:

```toml
[transport]
mode = "auto"

[camouflage]
sni_pool = ["dl.google.com", "www.google.com"]
padding = true
browser_profile = "chrome"

[reality]
target = "www.google.com:443"
address = "1.2.3.4:8443"
```

Клиент пробует: QUIC → QUIC Camouflaged → TCP Reality → WebSocket. Если активный транспорт деградирует, health monitor инициирует переключение.

---

## Режим демона (Linux / macOS)

Запуск клиента как фонового сервиса:

```bash
# Запустить VPN в фоне
sudo redpill-client --config client.toml up

# Проверить статус
redpill-client status
# Вывод:
# Redpill VPN Client
#   Status:    connected
#   Server:    1.2.3.4:443
#   Transport: QuicRaw
#   Client IP: 10.0.1.2
#   Uptime:    3600s
#   TX: 150.3 MB (125000 pkts)
#   RX: 1200.5 MB (1000000 pkts)

# Остановить VPN
sudo redpill-client down
```

Логи пишутся в `/tmp/redpill-client.log`. PID-файл - `/tmp/redpill-client.pid`.

> **Примечание:** Режим демона недоступен на Windows. Используйте `redpill-client connect` (foreground) вместо этого.

---

## Развёртывание сервера

### systemd-сервис

Создайте `/etc/systemd/system/redpill-quic.service`:

```ini
[Unit]
Description=RedpillVPN Server
After=network.target

[Service]
Type=simple
ExecStart=/usr/local/bin/redpill-server -c /etc/redpill/server.toml
Restart=on-failure
RestartSec=5
LimitNOFILE=65535

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl daemon-reload
sudo systemctl enable redpill-quic
sudo systemctl start redpill-quic

# Просмотр логов
journalctl -u redpill-quic -f
```

### SIGHUP Reload

Перезагрузка конфигурации без перезапуска:

```bash
sudo kill -HUP $(pidof redpill-server)
```

**Перезагружаемое:** PSK, ключи пользователей, max_connections, decoy-страница, уровень логирования.
**Требует перезапуска:** listen-адрес, конфигурация TUN, сертификаты, NAT-интерфейс.

### Prometheus-метрики

```bash
curl http://127.0.0.1:9093/metrics
```

Доступные метрики:

| Метрика | Тип | Описание |
|---------|-----|----------|
| `redpill_active_sessions` | gauge | Текущее количество подключённых клиентов |
| `redpill_sessions_by_user{user}` | gauge | Сессии по пользователям |
| `redpill_bytes_in` / `_out` | counter | Всего байт получено / отправлено |
| `redpill_datagrams_in` / `_out` | counter | Всего датаграмм получено / отправлено |
| `redpill_handshakes_total` | counter | Всего попыток хэндшейка |
| `redpill_handshakes_failed` | counter | Неудачные хэндшейки (неверный PSK) |
| `redpill_drops_backpressure` | counter | Пакеты, сброшенные из-за backpressure |
| `redpill_drops_rate_limit` | counter | Пакеты, сброшенные rate limiter-ом |
| `redpill_drops_stale` | counter | Устаревшие realtime-пакеты, сброшенные |
| `redpill_rtt_ms` | histogram | Распределение round-trip time |

### XDP-производительность (Linux)

Соберите с фичей `xdp` для conntrack bypass на высоконагруженных серверах:

```bash
cargo build --release --features xdp
```

Это добавляет правила `iptables -t raw -j NOTRACK` для UDP:443, пропуская connection tracking в ядре. Правила автоматически удаляются при корректном завершении.

XDP также настраивает буферы сокетов (8 MB) и отключает UDP GRO для стабильной задержки.

---

## Справка по конфигурации

### Конфиг сервера

Полный пример: [`config/server.example.toml`](../config/server.example.toml)

| Ключ | По умолчанию | Описание |
|------|-------------|----------|
| `listen` | `0.0.0.0:443` | Адрес прослушивания (UDP) |
| `tun_name` | `redpill1` | Имя TUN-устройства |
| `tun_address` | `10.0.1.1` | IP сервера в туннеле |
| `tun_prefix_len` | `24` | Длина префикса подсети |
| `mtu` | `1200` | Начальный MTU TUN (автообновляется через PMTU) |
| `max_connections` | `64` | Макс. одновременных клиентов |
| `max_bandwidth_mbps` | `0` | Per-client ограничение полосы (0 = без ограничений) |
| `metrics_listen` | `127.0.0.1:9093` | Адрес Prometheus-метрик |
| `psk_file` | - | Путь к файлу PSK (однопользовательский режим) |
| `users_dir` | - | Директория с per-user `.key` файлами (мультипользовательский режим) |
| `cert_file` | `cert.pem` | Путь к TLS-сертификату |
| `key_file` | `key.pem` | Путь к приватному ключу TLS |
| `dns` | `1.1.1.1` | DNS-сервер, передаваемый клиентам |
| `log_level` | `info` | Уровень логирования (`trace`, `debug`, `info`, `warn`, `error`) |
| `nat.enabled` | `true` | Включить NAT masquerade |
| `nat.interface` | `ens1` | WAN-интерфейс для NAT |
| `decoy.enabled` | `true` | Включить HTTP/3 decoy для защиты от probe |
| `decoy.page` | - | Путь к HTML-файлу, отдаваемому как decoy |
| `reality.enabled` | `false` | Включить TCP Reality listener |
| `reality.listen` | `0.0.0.0:8443` | Адрес прослушивания TCP Reality |
| `reality.target` | `www.google.com:443` | Реальный сайт для перенаправления проб |
| `websocket.enabled` | `false` | Включить WebSocket listener |
| `websocket.listen` | `127.0.0.1:8080` | Адрес прослушивания WebSocket |

### Конфиг клиента

Полный пример: [`config/client.example.toml`](../config/client.example.toml)

| Секция | Ключ | По умолчанию | Описание |
|--------|------|-------------|----------|
| `[server]` | `address` | - | IP:порт сервера |
| | `cert` | - | Путь к сертификату сервера |
| | `psk` | - | 64-символьный hex PSK |
| | `domain` | - | Домен для WebPKI-верификации (вместо пиннинга сертификата) |
| `[transport]` | `mode` | `auto` | Транспортный режим (см. выше) |
| `[camouflage]` | `sni_pool` | Домены Google | SNI-домены для ротации |
| | `padding` | `true` | Паддинг пакетов до стандартных размеров |
| | `chrome_fingerprint` | `true` | Мимикрия под TLS-отпечаток браузера |
| | `browser_profile` | `chrome` | Профиль браузера: `chrome`, `firefox`, `safari`, `random` |
| `[reality]` | `target` | `www.google.com:443` | SNI target для Reality |
| | `address` | - | Переопределение адреса сервера для Reality-порта |
| `[websocket]` | `url` | - | URL WebSocket |
| | `host` | - | Заголовок Host для CDN |

### CLI-флаги клиента

```
redpill-client [OPTIONS] [COMMAND]

Commands:
  connect   Connect in foreground (default)
  up        Start as background daemon (Linux/macOS only)
  down      Stop the background daemon
  status    Query daemon status

Options:
  -s, --server <IP:PORT>   Server address
  -c, --cert <PATH>        Server certificate
      --psk <HEX>          PSK (64 hex chars)
      --config <PATH>      TOML config file (CLI flags override config values)
      --test-mode           Don't set up routes or kill-switch
  -q, --quiet              Suppress periodic stats output
```

---

## Архитектура

```
Client                              Server
+---------------+   QUIC:443    +---------------+
| redpill-client|<=============>| redpill-server|
|               | DATAGRAM      |               |
|  TUN device   | (raw IP pkts) |  TUN device   |
|  kill-switch  |               |  iptables NAT |
|  route/DNS    |               |  MSS clamping |
+---------------+               +---------------+
```

### Серверный data path

```
Internet ← NAT ← TUN device ← Global TUN reader
                                    ↓
                            extract dst IP
                                    ↓
                         ClientRouter (DashMap)
                                    ↓
                          PriorityQueue (per-client)
                           ├── Realtime lane
                           └── Bulk lane
                                    ↓
                         QUIC DATAGRAM → Client
```

- **Один TUN reader** - один таск читает все пакеты, демультиплексирует по destination IP
- **Классификация приоритетов** - DSCP EF, мелкий UDP (<300 B), DNS → realtime; всё остальное → bulk
- **Backpressure** - realtime: проверка буфера, дроп если полон; bulk: `send_datagram_wait` с таймаутом 25 мс

### Протокол

#### Control stream (первый двунаправленный QUIC stream)

```
Client → Server: [0x01][32B nonce][32B HMAC-SHA256(psk, nonce)][1B version]   (66 bytes)
Server → Client: [0x02][4B client_ip][4B server_ip][4B dns][2B mtu][1B keepalive][1B flags]  (17 bytes)
```

Flags: бит 0 = поддержка батчинга. При неудачной аутентификации соединение закрывается с кодом ошибки `0x01`.

#### DATAGRAM-фреймы

Сырые IP-пакеты или батчированные мелкие пакеты:
```
[2B length BE][payload][2B length BE][payload]...
```

Эвристика: IP-пакеты начинаются с ниббла 4/6, батчированные датаграммы - с 0x00/0x01.

#### ALPN

- `redpill-vpn-1` - VPN-туннель
- `h3` - HTTP/3 decoy

---

## Тестирование

```bash
# Unit + интеграционные тесты
cargo test -p redpill-quic

# Пропускная способность (требуется работающий VPN + iperf3 на сервере)
iperf3 -c 10.0.1.1 -t 10 -R    # скачивание
iperf3 -c 10.0.1.1 -t 10        # загрузка

# Проверка утечки IP
curl -s https://api.ipify.org    # должен показать IP сервера

# Проверка утечки DNS
dig +short myip.opendns.com @resolver1.opendns.com
```

---

## Roadmap

Запланировано, но пока не реализовано:

**Сеть:**
- [ ] IPv6 dual-stack (`[::]:443`, туннель `fd00:rpll::/64`)
- [ ] Проактивная QUIC connection migration (детекция смены сети)
- [ ] Multi-server failover (список серверов с приоритетами)
- [ ] Multi-path (Wi-Fi + сотовая сеть одновременно)

**Производительность:**
- [ ] io_uring UDP бэкенд (Linux 5.10+)
- [ ] Полный AF_XDP kernel bypass (Linux 5.9+)

**Безопасность:**
- [ ] Per-IP rate limiting хэндшейков (защита от брутфорса)
- [ ] Let's Encrypt autocert (сейчас заглушка генерирует самоподписанные сертификаты)

**Наблюдаемость:**
- [ ] Поддержка qlog (RFC 9443)
- [ ] Шаблоны дашбордов Grafana

**Клиенты:**
- [ ] iOS-клиент (NEPacketTunnelProvider)
- [ ] Android-клиент
- [ ] Веб-интерфейс управления

---

## Лицензия

[PolyForm Noncommercial 1.0.0](../LICENSE) - бесплатно для личного и некоммерческого использования.
Этот проект относится к source-available (не OSI Open Source): для коммерческого использования нужна отдельная лицензия.

По вопросам коммерческого лицензирования: **gegam.m92@gmail.com**
