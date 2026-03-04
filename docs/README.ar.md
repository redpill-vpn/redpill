[English](../README.md) | [Русский](README.ru.md) | [فارسی](README.fa.md) | **العربية** | [中文](README.zh-CN.md)

# RedpillVPN

شبكة VPN سريعة ومقاومة للرقابة مبنية على QUIC. مكتوبة بلغة Rust.

> **الحالة: MVP / نسخة ألفا مبكرة.** النفق الأساسي يعمل ومُختبر على macOS/Linux، لكن توقع بعض المشاكل وتغييرات جذرية وميزات ناقصة. راجع [خارطة الطريق](#خارطة-الطريق) للخطط المستقبلية.

ينقل حزم IP الخام عبر إطارات QUIC DATAGRAM (RFC 9221) على UDP:443 مع TLS 1.3. يستخدم QUIC الخارجي متحكم ازدحام معطّل (نافذة ثابتة 16 MB) لأن TCP الداخلي يتولى التحكم بالازدحام. يدعم أوضاع نقل متعددة لمختلف ظروف الشبكة.

## الميزات

### النقل
- إطارات QUIC DATAGRAM (RFC 9221) عبر UDP:443
- TLS 1.3 عبر quinn/rustls، شهادات موقعة ذاتياً (تُنشأ تلقائياً عند أول تشغيل)
- تحكم ازدحام معطّل (نافذة ثابتة 16 MB) - TCP الداخلي يتولى CC
- تجميع الـ datagrams للحزم الصغيرة (<300 B: استعلامات DNS، TCP ACKs)
- MTU ديناميكي - السيرفر يراقب PMTU ويرسل التحديثات للعميل لحظياً

### الأمان
- مصادقة PSK (HMAC-SHA256، تحقق بزمن ثابت)
- دعم متعدد المستخدمين - كل مستخدم يحصل على PSK خاص (`add-user` / `remove-user` CLI)
- التحقق من عنوان IP المصدر لمنع التزييف
- Kill-switch على macOS (pf)، Windows (Windows Firewall) - يمنع التسريب عند قطع الاتصال
- سيرفر HTTP/3 وهمي (صفحة nginx مزيفة لمقاومة الفحص النشط)

### مقاومة الحجب
- **5 أوضاع نقل** - QUIC مباشر، QUIC + تمويه، TCP Reality، WebSocket CDN، تلقائي
- تمويه SNI مع تدوير الدومينات
- محاكاة بصمة TLS للمتصفحات - ملفات Chrome، Firefox، Safari (مقاومة JA3/JA4)
- توحيد أحجام الحزم لتتوافق مع أحجام HTTP/3 القياسية (128 / 256 / 512 / 1024 / 1200 / 1400)
- حشو حركة المرور في أوقات الخمول (حزم وهمية أثناء عدم النشاط)
- TCP Reality - اتصالات TLS غير المتعلقة بالـ VPN تُمرر بشفافية لموقع حقيقي
- WebSocket CDN - النفق عبر CDN reverse proxies (مثل Cloudflare)

### السيرفر
- متعدد العملاء مع طوابير أولوية لكل عميل وتوزيع IP (10.0.1.2-254، حتى 253 عميل)
- أولوية حركة المرور - الحزم اللحظية (UDP صغير، DNS، DSCP EF) تُعالج قبل الحزم الكبيرة
- ضغط خلفي من جانب السيرفر لمنع تضخم الفقد
- تشكيل حركة مرور تكيفي مبني على RTT + سقف نطاق ترددي لكل عميل
- XDP conntrack bypass للأداء العالي (Linux، اختياري)
- نقطة Prometheus metrics (`/metrics`)
- إعادة تحميل بـ SIGHUP (PSK، المستخدمين، max_connections، صفحة الخداع، مستوى السجل)
- NAT masquerade + MSS clamping (تلقائي)

### العميل
- إعادة اتصال تلقائية مع تراجع أسّي (1s-30s) وتذبذب عشوائي
- مراقبة صحية مع تبديل تلقائي للنقل وترقية
- وضع daemon (Linux/macOS) - أوامر `up` / `down` / `status` مع IPC
- تنظيف الحالة القديمة عند بدء التشغيل (المسارات، DNS، قواعد الجدار الناري من جلسات متعطلة)

## المنصات المدعومة

| الدور | نظام التشغيل | المعمارية |
|-------|--------------|-----------|
| سيرفر | Linux | x86_64, arm64 |
| عميل | macOS, Linux, Windows | arm64, x86_64 |

> **ملاحظة:** عميل Windows يُترجم ويتضمن دعم كامل لـ wintun/routing/firewall، لكنه لم يُختبر على عتاد حقيقي بعد. عملاء macOS وLinux مُختبرون بالكامل.

---

## البداية السريعة

### 1. البناء

```bash
git clone https://github.com/redpill-vpn/redpill.git
cd redpill
cargo build --release
```

الملفات التنفيذية تظهر في `target/release/`:
- `redpill-server` - سيرفر VPN
- `redpill-client` - عميل VPN

ميزات بناء اختيارية:

```bash
cargo build --release --features xdp    # XDP conntrack bypass (سيرفر Linux فقط)
cargo build --release --features acme   # ACME/Let's Encrypt stub
```

### 2. توليد PSK

```bash
openssl rand -hex 32
# Example output: a3f7b2c1d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0
```

احفظ هذا النص الست عشري المكون من 64 حرف - ستحتاجه للسيرفر والعميل.

### 3. إعداد السيرفر

```bash
# Create config directory
sudo mkdir -p /etc/redpill

# Copy example config
sudo cp config/server.example.toml /etc/redpill/server.toml

# Save the PSK
echo "YOUR_64_CHAR_HEX_PSK" | sudo tee /etc/redpill/psk

# Install binary
sudo cp target/release/redpill-server /usr/local/bin/

# Start the server
sudo redpill-server -c /etc/redpill/server.toml
```

**مهم:** عدّل `/etc/redpill/server.toml` واضبط `nat.interface` على واجهة WAN للسيرفر (مثلاً `ens1`، `eth0`). اعثر عليها بـ `ip route show default`.

عند أول تشغيل، السيرفر يولّد تلقائياً شهادة TLS موقعة ذاتياً في المسارات المحددة بالإعدادات (`cert_file` / `key_file`). **انسخ ملف الشهادة إلى جهاز العميل.**

### 4. إعداد العميل

#### بسيط (أعلام CLI)

```bash
sudo redpill-client \
  --server YOUR_SERVER_IP:443 \
  --cert path/to/cert.pem \
  --psk YOUR_64_CHAR_HEX_PSK
```

#### بملف إعدادات (مُستحسن)

```bash
sudo cp config/client.example.toml ~/client.toml
# Edit ~/client.toml - set server address, cert path, and PSK
sudo redpill-client --config ~/client.toml
```

#### وضع الاختبار (بدون مسارات، يحافظ على اتصالك الحالي)

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk <hex> --test-mode
```

في وضع الاختبار يُنشأ النفق لكن لا تتغير مسارات النظام أو DNS. مفيد لاختبارات iperf3 أو تصحيح الأخطاء.

### 5. التحقق

```bash
# Should show your server's IP
curl -s https://api.ipify.org

# Ping through the tunnel
ping 10.0.1.1
```

---

## إدارة المستخدمين المتعددين

بدلاً من PSK مشترك واحد، يمكنك إعطاء كل مستخدم مفتاحه الخاص.

### تفعيل الوضع متعدد المستخدمين

أضف `users_dir` لإعدادات السيرفر:

```toml
users_dir = "/etc/redpill/users"
```

### إضافة مستخدم

```bash
# Generate a random PSK for the user
openssl rand -hex 32 | sudo tee /etc/redpill/users/alice.key
```

أعطِ `alice` مفتاح PSK الخاص بها وشهادة السيرفر. تتصل بـ:

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk $(cat alice.key)
```

### إزالة مستخدم

```bash
sudo rm /etc/redpill/users/alice.key
sudo kill -HUP $(pidof redpill-server)   # Reload without restart
```

### عرض المستخدمين

```bash
ls /etc/redpill/users/
# alice.key  bob.key  charlie.key
```

كل ملف `.key` هو PSK ست عشري من 64 حرف. اسم الملف (بدون `.key`) هو اسم المستخدم الذي يظهر في السجلات والمقاييس.

> **التوافق مع الإصدارات السابقة:** إذا كان `psk_file` مضبوطاً أيضاً، يُقبل مفتاحه كبديل احتياطي (يظهر كمستخدم `legacy` في السجلات). للانتقال: انسخ PSK القديم إلى `users_dir/legacy.key` واحذف `psk_file`.

---

## أوضاع النقل

يدعم RedpillVPN خمسة أوضاع نقل. اختر الأنسب لشبكتك:

| الوضع | البروتوكول | متى تستخدمه |
|-------|-----------|-------------|
| `quic` | QUIC DATAGRAM مباشر عبر UDP:443 | الافتراضي. أفضل أداء على الشبكات غير المقيدة |
| `quic-camouflaged` | QUIC + تدوير SNI + حشو + بصمة متصفح | شبكات بها DPI خفيف (تصفية مبنية على SNI) |
| `tcp-reality` | TLS-over-TCP مع صد الفحص النشط | QUIC محجوب، TCP+TLS لا يزال يعمل |
| `websocket` | إطارات WebSocket ثنائية عبر CDN | فقط الوصول عبر CDN يعمل |
| `auto` | يفحص جميع وسائل النقل ويختار الأفضل | مُستحسن للشبكات المحجوبة أو غير المعروفة |

### ضبط وسائل النقل

اختيار النقل يتم في ملف إعدادات العميل (قسم `[transport]`):

#### QUIC مباشر (الافتراضي)

```toml
[server]
address = "1.2.3.4:443"
cert = "cert.pem"
psk = "your-psk-hex"

[transport]
mode = "quic"
```

#### QUIC مموّه

يجعل حركة QUIC تبدو كحركة HTTPS عادية للمتصفح:

```toml
[transport]
mode = "quic-camouflaged"

[camouflage]
# Domains to rotate through (SNI field in ClientHello)
sni_pool = ["dl.google.com", "www.google.com", "fonts.gstatic.com", "www.youtube.com"]
# Pad packets to standard HTTP/3 sizes
padding = true
# Mimic a real browser's TLS fingerprint
chrome_fingerprint = true
# Browser to mimic: "chrome", "firefox", "safari", or "random"
browser_profile = "chrome"
```

#### TCP Reality

عندما يكون QUIC/UDP محجوباً بالكامل. السيرفر يقبل TCP+TLS على منفذ منفصل ويحوّل فحوصات غير الـ VPN لموقع حقيقي:

إعدادات السيرفر:
```toml
[reality]
enabled = true
listen = "0.0.0.0:8443"
target = "www.google.com:443"   # Probes see this real website
```

إعدادات العميل:
```toml
[transport]
mode = "tcp-reality"

[reality]
target = "www.google.com:443"
address = "1.2.3.4:8443"       # Server's Reality port
```

#### WebSocket CDN

توجيه الحركة عبر CDN (مثل Cloudflare) لإخفاء IP السيرفر:

إعدادات السيرفر:
```toml
[websocket]
enabled = true
listen = "127.0.0.1:8080"      # Behind CDN reverse proxy
```

إعدادات العميل:
```toml
[transport]
mode = "websocket"

[websocket]
url = "wss://cdn.example.com/ws"
host = "cdn.example.com"
```

#### الوضع التلقائي (مُستحسن للشبكات المحجوبة)

يفحص وسائل النقل بترتيب الأولوية ويختار أول واحد يعمل:

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

العميل يجرب: QUIC ثم QUIC مموّه ثم TCP Reality ثم WebSocket. إذا تدهور النقل النشط، مراقب الصحة يبدأ التبديل.

---

## وضع Daemon (Linux / macOS)

تشغيل العميل كخدمة في الخلفية:

```bash
# Start VPN in background
sudo redpill-client --config client.toml up

# Check status
redpill-client status
# Output:
# Redpill VPN Client
#   Status:    connected
#   Server:    1.2.3.4:443
#   Transport: QuicRaw
#   Client IP: 10.0.1.2
#   Uptime:    3600s
#   TX: 150.3 MB (125000 pkts)
#   RX: 1200.5 MB (1000000 pkts)

# Stop VPN
sudo redpill-client down
```

السجلات تُكتب في `/tmp/redpill-client.log`. ملف PID في `/tmp/redpill-client.pid`.

> **ملاحظة:** وضع daemon غير متاح على Windows. استخدم `redpill-client connect` (في المقدمة) بدلاً منه.

---

## نشر السيرفر

### خدمة systemd

أنشئ `/etc/systemd/system/redpill-quic.service`:

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

# View logs
journalctl -u redpill-quic -f
```

### إعادة التحميل بـ SIGHUP

إعادة تحميل الإعدادات بدون إعادة التشغيل:

```bash
sudo kill -HUP $(pidof redpill-server)
```

**قابل لإعادة التحميل:** PSK، مفاتيح المستخدمين، max_connections، صفحة الخداع، مستوى السجل.
**يتطلب إعادة تشغيل:** عنوان الاستماع، إعدادات TUN، الشهادات، واجهة NAT.

### مقاييس Prometheus

```bash
curl http://127.0.0.1:9093/metrics
```

المقاييس المتاحة:

| المقياس | النوع | الوصف |
|---------|-------|-------|
| `redpill_active_sessions` | gauge | العملاء المتصلون حالياً |
| `redpill_sessions_by_user{user}` | gauge | الجلسات لكل مستخدم |
| `redpill_bytes_in` / `_out` | counter | إجمالي البايتات المستلمة / المرسلة |
| `redpill_datagrams_in` / `_out` | counter | إجمالي الـ datagrams المستلمة / المرسلة |
| `redpill_handshakes_total` | counter | إجمالي محاولات المصافحة |
| `redpill_handshakes_failed` | counter | المصافحات الفاشلة (PSK خاطئ) |
| `redpill_drops_backpressure` | counter | الحزم المسقطة بسبب الضغط الخلفي |
| `redpill_drops_rate_limit` | counter | الحزم المسقطة بسبب تحديد المعدل |
| `redpill_drops_stale` | counter | الحزم اللحظية القديمة المسقطة |
| `redpill_rtt_ms` | histogram | توزيع زمن الرحلة ذهاباً وإياباً |

### أداء XDP (Linux)

ابنِ مع ميزة `xdp` لتجاوز conntrack على السيرفرات ذات الإنتاجية العالية:

```bash
cargo build --release --features xdp
```

هذا يضيف قواعد `iptables -t raw -j NOTRACK` لـ UDP:443، متجاوزاً تتبع الاتصالات في النواة. القواعد تُنظف تلقائياً عند الإغلاق المنظم.

XDP أيضاً يضبط مخازن الـ socket (8 MB) ويعطل UDP GRO لزمن استجابة ثابت.

---

## مرجع الإعدادات

### إعدادات السيرفر

المثال الكامل: [`config/server.example.toml`](../config/server.example.toml)

| المفتاح | الافتراضي | الوصف |
|---------|-----------|-------|
| `listen` | `0.0.0.0:443` | عنوان الاستماع (UDP) |
| `tun_name` | `redpill1` | اسم جهاز TUN |
| `tun_address` | `10.0.1.1` | عنوان IP للسيرفر في النفق |
| `tun_prefix_len` | `24` | طول بادئة الشبكة الفرعية |
| `mtu` | `1200` | MTU أولي لـ TUN (يُحدّث تلقائياً عبر PMTU) |
| `max_connections` | `64` | أقصى عدد عملاء متزامنين |
| `max_bandwidth_mbps` | `0` | سقف النطاق الترددي لكل عميل (0 = بلا حدود) |
| `metrics_listen` | `127.0.0.1:9093` | عنوان مقاييس Prometheus |
| `psk_file` | - | مسار ملف PSK المشترك (وضع مستخدم واحد) |
| `users_dir` | - | مجلد ملفات `.key` لكل مستخدم (وضع متعدد المستخدمين) |
| `cert_file` | `cert.pem` | مسار شهادة TLS |
| `key_file` | `key.pem` | مسار المفتاح الخاص TLS |
| `dns` | `1.1.1.1` | سيرفر DNS المُرسل للعملاء |
| `log_level` | `info` | مستوى السجل (`trace`, `debug`, `info`, `warn`, `error`) |
| `nat.enabled` | `true` | تفعيل NAT masquerade |
| `nat.interface` | `ens1` | واجهة WAN للـ NAT |
| `decoy.enabled` | `true` | تفعيل خداع HTTP/3 لمقاومة الفحص |
| `decoy.page` | - | مسار ملف HTML يُقدم كصفحة وهمية |
| `reality.enabled` | `false` | تفعيل مستمع TCP Reality |
| `reality.listen` | `0.0.0.0:8443` | عنوان استماع TCP Reality |
| `reality.target` | `www.google.com:443` | الموقع الحقيقي لصد الفحص |
| `websocket.enabled` | `false` | تفعيل مستمع WebSocket |
| `websocket.listen` | `127.0.0.1:8080` | عنوان استماع WebSocket |

### إعدادات العميل

المثال الكامل: [`config/client.example.toml`](../config/client.example.toml)

| القسم | المفتاح | الافتراضي | الوصف |
|-------|---------|-----------|-------|
| `[server]` | `address` | - | عنوان السيرفر IP:port |
| | `cert` | - | مسار شهادة السيرفر |
| | `psk` | - | PSK ست عشري من 64 حرف |
| | `domain` | - | دومين للتحقق عبر WebPKI (بدلاً من تثبيت الشهادة) |
| `[transport]` | `mode` | `auto` | وضع النقل (انظر أعلاه) |
| `[camouflage]` | `sni_pool` | دومينات Google | دومينات SNI للتدوير |
| | `padding` | `true` | حشو الحزم لأحجام قياسية |
| | `chrome_fingerprint` | `true` | محاكاة بصمة TLS للمتصفح |
| | `browser_profile` | `chrome` | ملف المتصفح: `chrome`, `firefox`, `safari`, `random` |
| `[reality]` | `target` | `www.google.com:443` | هدف SNI لـ Reality |
| | `address` | - | تجاوز عنوان السيرفر لمنفذ Reality |
| `[websocket]` | `url` | - | رابط WebSocket |
| | `host` | - | ترويسة Host للـ CDN |

### أعلام CLI للعميل

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

## البنية

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

### مسار البيانات في السيرفر

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

- **قارئ TUN واحد** - مهمة واحدة تقرأ جميع الحزم وتوزعها حسب عنوان IP الوجهة
- **تصنيف الأولوية** - DSCP EF، UDP صغير (<300 B)، DNS تُصنف لحظية؛ الباقي كتلي
- **الضغط الخلفي** - لحظي: فحص مساحة المخزن أولاً، إسقاط إذا ممتلئ؛ كتلي: `send_datagram_wait` مع مهلة 25 ms

### البروتوكول

#### مجرى التحكم (أول مجرى QUIC ثنائي الاتجاه)

```
Client → Server: [0x01][32B nonce][32B HMAC-SHA256(psk, nonce)][1B version]   (66 bytes)
Server → Client: [0x02][4B client_ip][4B server_ip][4B dns][2B mtu][1B keepalive][1B flags]  (17 bytes)
```

الأعلام: bit 0 = دعم التجميع. فشل المصادقة يغلق الاتصال برمز خطأ `0x01`.

#### إطارات DATAGRAM

حزم IP خام، أو حزم صغيرة مجمّعة:
```
[2B length BE][payload][2B length BE][payload]...
```

استدلال: حزم IP تبدأ بـ nibble 4/6، الـ datagrams المجمّعة تبدأ بـ 0x00/0x01.

#### ALPN

- `redpill-vpn-1` - نفق VPN
- `h3` - خداع HTTP/3

---

## الاختبار

```bash
# Unit + integration tests
cargo test -p redpill-quic

# Throughput (requires running VPN + iperf3 on server)
iperf3 -c 10.0.1.1 -t 10 -R    # download
iperf3 -c 10.0.1.1 -t 10        # upload

# IP leak check
curl -s https://api.ipify.org    # should show server IP

# DNS leak check
dig +short myip.opendns.com @resolver1.opendns.com
```

---

## خارطة الطريق

الميزات التالية مخطط لها لكنها لم تُنفذ بعد:

**الشبكات:**
- [ ] IPv6 dual-stack (`[::]:443`، نفق `fd00:rpll::/64`)
- [ ] هجرة اتصال QUIC استباقية (كشف تغيّر الشبكة)
- [ ] تجاوز فشل متعدد السيرفرات (قائمة سيرفرات بأولوية)
- [ ] مسارات متعددة (Wi-Fi + خلوي في نفس الوقت)

**الأداء:**
- [ ] واجهة UDP خلفية عبر io_uring (Linux 5.10+)
- [ ] تجاوز نواة كامل عبر AF_XDP (Linux 5.9+)

**الأمان:**
- [ ] تحديد معدل المصافحة لكل IP (حماية من هجمات القوة الغاشمة)
- [ ] شهادات Let's Encrypt تلقائية (الكود الحالي يولّد شهادات موقعة ذاتياً)

**المراقبة:**
- [ ] دعم qlog (RFC 9443)
- [ ] قوالب لوحات Grafana

**العملاء:**
- [ ] عميل iOS (NEPacketTunnelProvider)
- [ ] عميل Android
- [ ] واجهة إدارة ويب

---

## الترخيص

[PolyForm Noncommercial 1.0.0](../LICENSE) - مجاني للاستخدام الشخصي وغير التجاري.
هذا المشروع source-available (وليس OSI Open Source): الاستخدام التجاري يتطلب ترخيصًا منفصلًا.

للترخيص التجاري، تواصل عبر: **gegam.m92@gmail.com**
