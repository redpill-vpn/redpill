[English](../README.md) | [Русский](README.ru.md) | **فارسی** | [العربية](README.ar.md) | [中文](README.zh-CN.md)

# RedpillVPN

VPN سریع و ضد سانسور مبتنی بر QUIC. نوشته‌شده با Rust.

> **وضعیت: MVP / آلفای اولیه.** تانلینگ اصلی کار می‌کنه و روی macOS/Linux تست شده، ولی انتظار باگ‌های احتمالی، تغییرات ناسازگار و فیچرهای ناقص رو داشته باشید. برای لیست کارهای آینده [نقشه راه](#نقشه-راه) رو ببینید.

پکت‌های IP خام رو از طریق QUIC DATAGRAM frames (RFC 9221) روی UDP:443 با TLS 1.3 تانل می‌کنه. QUIC بیرونی از یه congestion controller غیرفعال (پنجره ثابت 16 MB) استفاده می‌کنه چون TCP داخلی خودش CC رو هندل می‌کنه. از چندین حالت transport برای شرایط مختلف شبکه پشتیبانی می‌کنه.

## امکانات

### ترنسپورت
- QUIC DATAGRAM frames (RFC 9221) روی UDP:443
- TLS 1.3 از طریق quinn/rustls، گواهی‌نامه self-signed (تولید خودکار در اولین اجرا)
- Congestion control غیرفعال (پنجره ثابت 16 MB) - TCP داخلی خودش CC رو هندل می‌کنه
- Batching دیتاگرام برای پکت‌های کوچک (<300 B: کوئری DNS، TCP ACK)
- MTU داینامیک - سرور PMTU رو مانیتور و به‌روزرسانی‌ها رو بلادرنگ به کلاینت پوش می‌کنه

### امنیت
- احراز هویت PSK (HMAC-SHA256، مقایسه constant-time)
- پشتیبانی چندکاربره - هر کاربر PSK مخصوص خودش رو داره (`add-user` / `remove-user` در CLI)
- اعتبارسنجی Source IP برای جلوگیری از جعل
- Kill-switch روی macOS (pf) و Windows (Windows Firewall) - جلوگیری از نشت ترافیک هنگام قطع اتصال
- سرور decoy با HTTP/3 (صفحه جعلی nginx برای مقاومت در برابر active probe)

### ضد سانسور
- **۵ حالت transport** - QUIC مستقیم، QUIC + کاموفلاژ، TCP Reality، WebSocket CDN، خودکار
- کاموفلاژ SNI با چرخش دامنه‌ها
- تقلید فینگرپرینت TLS مرورگر - پروفایل Chrome، Firefox، Safari (مقاومت در برابر JA3/JA4)
- نرمال‌سازی سایز پکت به سایزهای استاندارد HTTP/3 (128 / 256 / 512 / 1024 / 1200 / 1400)
- Padding ترافیک خنثی (پکت‌های ساختگی در زمان بیکاری)
- TCP Reality - اتصالات TLS غیر-VPN به‌صورت شفاف به یه وبسایت واقعی پروکسی می‌شن
- WebSocket CDN - تانل از طریق CDN reverse proxy (مثل Cloudflare)

### سرور
- چندکلاینته با صف اولویت‌دار و تخصیص IP به ازای هر کلاینت (10.0.1.2-254، تا ۲۵۳ کلاینت)
- اولویت‌بندی ترافیک - بلادرنگ (UDP کوچک، DNS، DSCP EF) قبل از ترافیک سنگین پردازش می‌شه
- Backpressure سمت سرور برای جلوگیری از تقویت packet loss
- شکل‌دهی ترافیک تطبیقی مبتنی بر RTT + سقف پهنای باند به ازای هر کلاینت
- XDP conntrack bypass برای عملکرد بالا (Linux، اختیاری)
- اندپوینت Prometheus metrics (`/metrics`)
- بارگذاری مجدد با SIGHUP (PSK، کاربران، max_connections، صفحه decoy، سطح لاگ)
- NAT masquerade + MSS clamping (خودکار)

### کلاینت
- اتصال مجدد خودکار با exponential backoff (1 تا 30 ثانیه) و jitter
- مانیتورینگ سلامت با fallback و ارتقای خودکار transport
- حالت Daemon (Linux/macOS) - دستورات `up` / `down` / `status` با IPC
- پاک‌سازی state قدیمی هنگام شروع (routeها، DNS، قوانین فایروال از نشست‌های کرش‌کرده)

## پلتفرم‌های پشتیبانی‌شده

| نقش | سیستم‌عامل | معماری |
|------|------------|--------|
| سرور | Linux | x86_64, arm64 |
| کلاینت | macOS, Linux, Windows | arm64, x86_64 |

> **نکته:** کلاینت Windows کامپایل می‌شه و شامل پشتیبانی کامل از wintun/routing/firewall هست، ولی هنوز روی سخت‌افزار واقعی تست نشده. کلاینت‌های macOS و Linux به‌طور کامل تست شدن.

---

## شروع سریع

### ۱. بیلد

```bash
git clone https://github.com/redpill-vpn/redpill.git
cd redpill
cargo build --release
```

باینری‌ها در `target/release/` ساخته می‌شن:
- `redpill-server` - سرور VPN
- `redpill-client` - کلاینت VPN

فیچرهای اختیاری بیلد:

```bash
cargo build --release --features xdp    # XDP conntrack bypass (فقط سرور Linux)
cargo build --release --features acme   # ACME/Let's Encrypt stub
```

### ۲. ساخت PSK

```bash
openssl rand -hex 32
# Example output: a3f7b2c1d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0
```

این رشته hex ۶۴ کاراکتری رو ذخیره کنید - هم برای سرور و هم برای کلاینت لازمش دارید.

### ۳. راه‌اندازی سرور

```bash
# ساخت دایرکتوری کانفیگ
sudo mkdir -p /etc/redpill

# کپی کانفیگ نمونه
sudo cp config/server.example.toml /etc/redpill/server.toml

# ذخیره PSK
echo "YOUR_64_CHAR_HEX_PSK" | sudo tee /etc/redpill/psk

# نصب باینری
sudo cp target/release/redpill-server /usr/local/bin/

# اجرای سرور
sudo redpill-server -c /etc/redpill/server.toml
```

**مهم:** فایل `/etc/redpill/server.toml` رو ادیت کنید و `nat.interface` رو به اینترفیس WAN سرورتون (مثلاً `ens1`، `eth0`) تنظیم کنید. با `ip route show default` پیداش کنید.

در اولین اجرا سرور به‌صورت خودکار یه گواهی TLS خودامضا در مسیرهای مشخص‌شده در کانفیگ (`cert_file` / `key_file`) تولید می‌کنه. **فایل گواهی رو به ماشین کلاینت کپی کنید.**

### ۴. راه‌اندازی کلاینت

#### ساده (با فلگ‌های CLI)

```bash
sudo redpill-client \
  --server YOUR_SERVER_IP:443 \
  --cert path/to/cert.pem \
  --psk YOUR_64_CHAR_HEX_PSK
```

#### با فایل کانفیگ (توصیه‌شده)

```bash
sudo cp config/client.example.toml ~/client.toml
# فایل ~/client.toml رو ادیت کنید - آدرس سرور، مسیر گواهی و PSK رو تنظیم کنید
sudo redpill-client --config ~/client.toml
```

#### حالت تست (بدون تغییر route، اتصال فعلی حفظ می‌شه)

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk <hex> --test-mode
```

در حالت تست تانل برقرار می‌شه ولی هیچ تغییری در routeها یا DNS سیستم داده نمی‌شه. برای بنچمارک با iperf3 یا دیباگ مفیده.

### ۵. تأیید

```bash
# باید IP سرورتون رو نشون بده
curl -s https://api.ipify.org

# پینگ از طریق تانل
ping 10.0.1.1
```

---

## مدیریت چندکاربره

به‌جای یه PSK مشترک، می‌تونید به هر کاربر کلید اختصاصی بدید.

### فعال‌سازی حالت چندکاربره

`users_dir` رو به کانفیگ سرور اضافه کنید:

```toml
users_dir = "/etc/redpill/users"
```

### اضافه کردن کاربر

```bash
# تولید PSK تصادفی برای کاربر
openssl rand -hex 32 | sudo tee /etc/redpill/users/alice.key
```

PSK و گواهی سرور رو به `alice` بدید. اینطوری وصل می‌شه:

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk $(cat alice.key)
```

### حذف کاربر

```bash
sudo rm /etc/redpill/users/alice.key
sudo kill -HUP $(pidof redpill-server)   # بارگذاری مجدد بدون ریستارت
```

### لیست کاربران

```bash
ls /etc/redpill/users/
# alice.key  bob.key  charlie.key
```

هر فایل `.key` یه PSK هگز ۶۴ کاراکتری هست. اسم فایل (بدون `.key`) به‌عنوان نام کاربر در لاگ‌ها و metricsها نشون داده می‌شه.

> **سازگاری با نسخه قبل:** اگه `psk_file` هم تنظیم شده باشه، کلیدش به‌عنوان فال‌بک قبول می‌شه (در لاگ‌ها به‌عنوان کاربر `legacy` نشون داده می‌شه). برای مهاجرت: PSK قدیمی رو در `users_dir/legacy.key` کپی کنید و `psk_file` رو حذف کنید.

---

## حالت‌های Transport

RedpillVPN از ۵ حالت transport پشتیبانی می‌کنه. اونی رو انتخاب کنید که به شبکه‌تون می‌خوره:

| حالت | پروتکل | کی استفاده بشه |
|------|---------|----------------|
| `quic` | QUIC DATAGRAM مستقیم روی UDP:443 | پیش‌فرض. بهترین عملکرد در شبکه‌های بدون محدودیت |
| `quic-camouflaged` | QUIC + چرخش SNI + padding + فینگرپرینت مرورگر | شبکه‌هایی با DPI سبک (فیلتر مبتنی بر SNI) |
| `tcp-reality` | TLS-over-TCP با دفع active probe | QUIC بلاک شده، TCP+TLS هنوز کار می‌کنه |
| `websocket` | فریم‌های باینری WebSocket از طریق CDN | فقط دسترسی CDN-fronted کار می‌کنه |
| `auto` | همه transportها رو تست می‌کنه، بهترین رو انتخاب می‌کنه | توصیه‌شده برای شبکه‌های سانسورشده یا ناشناخته |

### پیکربندی transportها

انتخاب transport در فایل کانفیگ کلاینت (بخش `[transport]`) انجام می‌شه:

#### QUIC مستقیم (پیش‌فرض)

```toml
[server]
address = "1.2.3.4:443"
cert = "cert.pem"
psk = "your-psk-hex"

[transport]
mode = "quic"
```

#### QUIC Camouflaged

ترافیک QUIC رو شبیه HTTPS معمولی مرورگر می‌کنه:

```toml
[transport]
mode = "quic-camouflaged"

[camouflage]
# دامنه‌ها برای چرخش (فیلد SNI در ClientHello)
sni_pool = ["dl.google.com", "www.google.com", "fonts.gstatic.com", "www.youtube.com"]
# پکت‌ها رو به سایزهای استاندارد HTTP/3 پد می‌کنه
padding = true
# تقلید فینگرپرینت TLS یه مرورگر واقعی
chrome_fingerprint = true
# مرورگر مورد تقلید: "chrome", "firefox", "safari", یا "random"
browser_profile = "chrome"
```

#### TCP Reality

وقتی QUIC/UDP کاملاً بلاک شده. سرور TCP+TLS رو روی یه پورت جداگانه قبول می‌کنه و probeهای غیر-VPN رو به یه وبسایت واقعی هدایت می‌کنه:

کانفیگ سرور:
```toml
[reality]
enabled = true
listen = "0.0.0.0:8443"
target = "www.google.com:443"   # Probeها این وبسایت واقعی رو می‌بینن
```

کانفیگ کلاینت:
```toml
[transport]
mode = "tcp-reality"

[reality]
target = "www.google.com:443"
address = "1.2.3.4:8443"       # پورت Reality سرور
```

#### WebSocket CDN

مسیریابی ترافیک از طریق CDN (مثل Cloudflare) برای مخفی کردن IP سرور:

کانفیگ سرور:
```toml
[websocket]
enabled = true
listen = "127.0.0.1:8080"      # پشت CDN reverse proxy
```

کانفیگ کلاینت:
```toml
[transport]
mode = "websocket"

[websocket]
url = "wss://cdn.example.com/ws"
host = "cdn.example.com"
```

#### حالت خودکار (توصیه‌شده برای شبکه‌های سانسورشده)

transportها رو به ترتیب اولویت تست و اولین موردی که کار می‌کنه رو انتخاب می‌کنه:

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

کلاینت به ترتیب تلاش می‌کنه: QUIC → QUIC Camouflaged → TCP Reality → WebSocket. اگه transport فعال ضعیف بشه، مانیتور سلامت یه سوئیچ رو فعال می‌کنه.

---

## حالت Daemon (Linux / macOS)

اجرای کلاینت به‌عنوان سرویس پس‌زمینه:

```bash
# شروع VPN در پس‌زمینه
sudo redpill-client --config client.toml up

# بررسی وضعیت
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

# توقف VPN
sudo redpill-client down
```

لاگ‌ها در `/tmp/redpill-client.log` نوشته می‌شن. فایل PID در `/tmp/redpill-client.pid` هست.

> **نکته:** حالت Daemon در Windows در دسترس نیست. به‌جاش از `redpill-client connect` (فورگراند) استفاده کنید.

---

## دیپلوی سرور

### سرویس systemd

فایل `/etc/systemd/system/redpill-quic.service` رو بسازید:

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

# مشاهده لاگ‌ها
journalctl -u redpill-quic -f
```

### بارگذاری مجدد با SIGHUP

بارگذاری مجدد کانفیگ بدون ریستارت:

```bash
sudo kill -HUP $(pidof redpill-server)
```

**قابل بارگذاری مجدد:** PSK، کلیدهای کاربر، max_connections، صفحه decoy، سطح لاگ.
**نیاز به ریستارت:** آدرس listen، کانفیگ TUN، گواهی‌نامه‌ها، اینترفیس NAT.

### Prometheus Metrics

```bash
curl http://127.0.0.1:9093/metrics
```

متریک‌های موجود:

| متریک | نوع | توضیحات |
|--------|------|---------|
| `redpill_active_sessions` | gauge | کلاینت‌های متصل فعلی |
| `redpill_sessions_by_user{user}` | gauge | تعداد نشست به ازای هر کاربر |
| `redpill_bytes_in` / `_out` | counter | کل بایت‌های دریافتی / ارسالی |
| `redpill_datagrams_in` / `_out` | counter | کل دیتاگرام‌های دریافتی / ارسالی |
| `redpill_handshakes_total` | counter | کل تلاش‌های handshake |
| `redpill_handshakes_failed` | counter | handshakeهای ناموفق (PSK اشتباه) |
| `redpill_drops_backpressure` | counter | پکت‌های دراپ‌شده توسط backpressure |
| `redpill_drops_rate_limit` | counter | پکت‌های دراپ‌شده توسط rate limiter |
| `redpill_drops_stale` | counter | پکت‌های بلادرنگ قدیمی دراپ‌شده |
| `redpill_rtt_ms` | histogram | توزیع زمان رفت و برگشت |

### عملکرد XDP (Linux)

برای conntrack bypass روی سرورهای با ترافیک بالا با فیچر `xdp` بیلد کنید:

```bash
cargo build --release --features xdp
```

این قوانین `iptables -t raw -j NOTRACK` برای UDP:443 اضافه می‌کنه و connection tracking کرنل رو دور می‌زنه. قوانین هنگام خاموشی سالم به‌صورت خودکار پاک می‌شن.

XDP همچنین بافرهای سوکت رو تنظیم (8 MB) و UDP GRO رو برای تأخیر ثابت غیرفعال می‌کنه.

---

## مرجع پیکربندی

### کانفیگ سرور

نمونه کامل: [`config/server.example.toml`](../config/server.example.toml)

| کلید | پیش‌فرض | توضیحات |
|------|---------|---------|
| `listen` | `0.0.0.0:443` | آدرس گوش‌دادن (UDP) |
| `tun_name` | `redpill1` | نام دیوایس TUN |
| `tun_address` | `10.0.1.1` | IP تانل سرور |
| `tun_prefix_len` | `24` | طول پیشوند سابنت |
| `mtu` | `1200` | MTU اولیه TUN (به‌روزرسانی خودکار از طریق PMTU) |
| `max_connections` | `64` | حداکثر کلاینت‌های همزمان |
| `max_bandwidth_mbps` | `0` | سقف پهنای باند به ازای هر کلاینت (0 = بدون محدودیت) |
| `metrics_listen` | `127.0.0.1:9093` | آدرس Prometheus metrics |
| `psk_file` | - | مسیر فایل PSK مشترک (حالت تک‌کاربره) |
| `users_dir` | - | دایرکتوری فایل‌های `.key` کاربران (حالت چندکاربره) |
| `cert_file` | `cert.pem` | مسیر گواهی TLS |
| `key_file` | `key.pem` | مسیر کلید خصوصی TLS |
| `dns` | `1.1.1.1` | سرور DNS ارسالی به کلاینت‌ها |
| `log_level` | `info` | سطح لاگ (`trace`, `debug`, `info`, `warn`, `error`) |
| `nat.enabled` | `true` | فعال‌سازی NAT masquerade |
| `nat.interface` | `ens1` | اینترفیس WAN برای NAT |
| `decoy.enabled` | `true` | فعال‌سازی HTTP/3 decoy برای مقاومت در برابر probe |
| `decoy.page` | - | مسیر فایل HTML برای صفحه decoy |
| `reality.enabled` | `false` | فعال‌سازی TCP Reality listener |
| `reality.listen` | `0.0.0.0:8443` | آدرس گوش‌دادن TCP Reality |
| `reality.target` | `www.google.com:443` | وبسایت واقعی برای دفع probe |
| `websocket.enabled` | `false` | فعال‌سازی WebSocket listener |
| `websocket.listen` | `127.0.0.1:8080` | آدرس گوش‌دادن WebSocket |

### کانفیگ کلاینت

نمونه کامل: [`config/client.example.toml`](../config/client.example.toml)

| بخش | کلید | پیش‌فرض | توضیحات |
|------|------|---------|---------|
| `[server]` | `address` | - | آدرس سرور IP:port |
| | `cert` | - | مسیر گواهی سرور |
| | `psk` | - | PSK هگز ۶۴ کاراکتری |
| | `domain` | - | دامنه برای تأیید WebPKI (به‌جای cert pinning) |
| `[transport]` | `mode` | `auto` | حالت transport (بالا رو ببینید) |
| `[camouflage]` | `sni_pool` | دامنه‌های Google | دامنه‌های SNI برای چرخش |
| | `padding` | `true` | پد کردن پکت‌ها به سایزهای استاندارد |
| | `chrome_fingerprint` | `true` | تقلید فینگرپرینت TLS مرورگر |
| | `browser_profile` | `chrome` | پروفایل مرورگر: `chrome`, `firefox`, `safari`, `random` |
| `[reality]` | `target` | `www.google.com:443` | هدف SNI برای Reality |
| | `address` | - | بازنویسی آدرس سرور برای پورت Reality |
| `[websocket]` | `url` | - | آدرس WebSocket |
| | `host` | - | هدر Host برای CDN |

### فلگ‌های CLI کلاینت

```
redpill-client [OPTIONS] [COMMAND]

Commands:
  connect   اتصال در فورگراند (پیش‌فرض)
  up        اجرا به‌عنوان daemon پس‌زمینه (فقط Linux/macOS)
  down      توقف daemon پس‌زمینه
  status    استعلام وضعیت daemon

Options:
  -s, --server <IP:PORT>   آدرس سرور
  -c, --cert <PATH>        گواهی سرور
      --psk <HEX>          PSK (۶۴ کاراکتر هگز)
      --config <PATH>      فایل کانفیگ TOML (فلگ‌های CLI مقادیر کانفیگ رو بازنویسی می‌کنن)
      --test-mode           بدون تنظیم route یا kill-switch
  -q, --quiet              عدم نمایش آمار دوره‌ای
```

---

## معماری

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

### مسیر داده سرور

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

- **یک TUN reader** - یه تسک همه پکت‌ها رو می‌خونه و بر اساس IP مقصد دسته‌بندی می‌کنه
- **طبقه‌بندی اولویت** - DSCP EF، UDP کوچک (<300 B)، DNS → بلادرنگ؛ بقیه → سنگین
- **Backpressure** - بلادرنگ: اول فضای بافر چک می‌شه، اگه پر باشه دراپ می‌شه؛ سنگین: `send_datagram_wait` با تایم‌اوت ۲۵ میلی‌ثانیه

### پروتکل

#### Control stream (اولین جریان دوطرفه QUIC)

```
Client → Server: [0x01][32B nonce][32B HMAC-SHA256(psk, nonce)][1B version]   (66 bytes)
Server → Client: [0x02][4B client_ip][4B server_ip][4B dns][2B mtu][1B keepalive][1B flags]  (17 bytes)
```

فلگ‌ها: بیت 0 = پشتیبانی از batching. در صورت شکست احراز هویت، اتصال با کد خطای `0x01` بسته می‌شه.

#### فریم‌های DATAGRAM

پکت‌های IP خام، یا پکت‌های کوچک batch‌شده:
```
[2B length BE][payload][2B length BE][payload]...
```

روش تشخیص: پکت‌های IP با نیبل 4/6 شروع می‌شن، دیتاگرام‌های batch‌شده با 0x00/0x01.

#### ALPN

- `redpill-vpn-1` - تانل VPN
- `h3` - HTTP/3 decoy

---

## تست

```bash
# تست‌های واحد + یکپارچگی
cargo test -p redpill-quic

# تست سرعت (نیاز به VPN فعال + iperf3 روی سرور)
iperf3 -c 10.0.1.1 -t 10 -R    # دانلود
iperf3 -c 10.0.1.1 -t 10        # آپلود

# بررسی نشت IP
curl -s https://api.ipify.org    # باید IP سرور رو نشون بده

# بررسی نشت DNS
dig +short myip.opendns.com @resolver1.opendns.com
```

---

## نقشه راه

فیچرهای زیر برنامه‌ریزی شدن ولی هنوز پیاده‌سازی نشدن:

**شبکه:**
- [ ] IPv6 dual-stack (`[::]:443`، تانل `fd00:rpll::/64`)
- [ ] مهاجرت فعال اتصال QUIC (تشخیص تغییر شبکه)
- [ ] فیل‌اور چندسروره (لیست سرور با اولویت)
- [ ] Multi-path (Wi-Fi + سلولار همزمان)

**عملکرد:**
- [ ] بکند io_uring برای UDP (Linux 5.10+)
- [ ] AF_XDP کامل برای دور زدن کرنل (Linux 5.9+)

**امنیت:**
- [ ] محدودیت نرخ handshake به ازای هر IP (محافظت در برابر brute-force)
- [ ] Let's Encrypt autocert (stub فعلی گواهی خودامضا می‌سازه)

**مشاهده‌پذیری:**
- [ ] پشتیبانی از qlog (RFC 9443)
- [ ] قالب‌های داشبورد Grafana

**کلاینت‌ها:**
- [ ] کلاینت iOS (NEPacketTunnelProvider)
- [ ] کلاینت Android
- [ ] رابط مدیریت تحت وب

---

## لایسنس

[PolyForm Noncommercial 1.0.0](../LICENSE) - رایگان برای استفاده شخصی و غیرتجاری.
این پروژه source-available است (نه OSI Open Source): استفاده تجاری نیازمند لایسنس جداگانه است.

برای لایسنس تجاری تماس بگیرید: **gegam.m92@gmail.com**
