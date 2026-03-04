**English** | [Русский](docs/README.ru.md) | [فارسی](docs/README.fa.md) | [العربية](docs/README.ar.md) | [中文](docs/README.zh-CN.md)

# RedpillVPN

Fast, censorship-resistant VPN built on QUIC. Written in Rust.

> **Status: MVP / early alpha.** Core tunneling works and is tested on macOS/Linux, but expect rough edges, breaking changes, and missing features. See [Roadmap](#roadmap) for what's planned.

Tunnels raw IP packets over QUIC DATAGRAM frames (RFC 9221) on UDP:443, TLS 1.3. The outer QUIC uses a no-op congestion controller (fixed 16 MB window) since inner TCP already handles CC. Supports multiple transport modes for different network conditions.

## Features

### Transport
- QUIC DATAGRAM frames (RFC 9221) over UDP:443
- TLS 1.3 via quinn/rustls, self-signed certificates (auto-generated on first run)
- No-op congestion control (16 MB constant window) - inner TCP handles CC
- Datagram batching for small packets (<300 B: DNS queries, TCP ACKs)
- Dynamic MTU - server monitors PMTU and pushes updates to the client in real time

### Security
- PSK authentication (HMAC-SHA256, constant-time verification)
- Multi-user support - each user gets their own PSK (`add-user` / `remove-user` CLI)
- Source IP anti-spoofing validation
- Kill-switch on macOS (pf), Windows (Windows Firewall) - prevents leaks on disconnect
- HTTP/3 decoy server (fake nginx page for active probe resistance)

### Anti-Censorship
- **5 transport modes** - direct QUIC, QUIC + camouflage, TCP Reality, WebSocket CDN, auto
- SNI camouflage with round-robin domain rotation
- Browser TLS fingerprint mimicry - Chrome, Firefox, Safari profiles (JA3/JA4 resistance)
- Packet size normalization to standard HTTP/3 sizes (128 / 256 / 512 / 1024 / 1200 / 1400)
- Idle traffic padding (dummy packets during inactivity)
- TCP Reality - non-VPN TLS connections are transparently proxied to a real website
- WebSocket CDN - tunnel through CDN reverse proxies (Cloudflare, etc.)

### Server
- Multi-client with per-client priority queues and IP allocation (10.0.1.2-254, up to 253 clients)
- Traffic prioritization - realtime (small UDP, DNS, DSCP EF) dequeued before bulk
- Server-side backpressure to prevent loss amplification
- Adaptive RTT-based traffic shaping + per-client bandwidth cap
- XDP conntrack bypass for high packet-rate performance (Linux, optional)
- Prometheus metrics endpoint (`/metrics`)
- SIGHUP hot-reload (PSK, users, max_connections, decoy page, log level)
- NAT masquerade + MSS clamping (automatic)

### Client
- Auto-reconnect with exponential backoff (1s-30s) and jitter
- Health monitoring with automatic transport fallback and upgrade
- Daemon mode (Linux/macOS) - `up` / `down` / `status` commands with IPC
- Stale state cleanup on startup (routes, DNS, firewall rules from crashed sessions)

## Supported Platforms

| Role   | OS                    | Arch          |
|--------|-----------------------|---------------|
| Server | Linux                 | x86_64, arm64 |
| Client | macOS, Linux, Windows | arm64, x86_64 |

> **Note:** The Windows client compiles and includes full wintun/routing/firewall support, but has not been tested on real hardware yet. macOS and Linux clients are fully tested.

---

## Quick Start

### 1. Build

```bash
git clone https://github.com/redpill-vpn/redpill.git
cd redpill
cargo build --release
```

Binaries appear in `target/release/`:
- `redpill-server` - VPN server
- `redpill-client` - VPN client

Optional build features:

```bash
cargo build --release --features xdp    # XDP conntrack bypass (Linux server only)
cargo build --release --features acme   # ACME/Let's Encrypt stub
```

### 2. Generate a PSK

```bash
openssl rand -hex 32
# Example output: a3f7b2c1d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0
```

Save this 64-character hex string - you will need it for both server and client.

### 3. Server Setup

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

**Important:** Edit `/etc/redpill/server.toml` and set `nat.interface` to your server's WAN interface (e.g. `ens1`, `eth0`). Find it with `ip route show default`.

On first run the server auto-generates a self-signed TLS certificate at the paths specified in the config (`cert_file` / `key_file`). **Copy the certificate file to the client machine.**

### 4. Client Setup

#### Simple (CLI flags)

```bash
sudo redpill-client \
  --server YOUR_SERVER_IP:443 \
  --cert path/to/cert.pem \
  --psk YOUR_64_CHAR_HEX_PSK
```

#### With config file (recommended)

```bash
sudo cp config/client.example.toml ~/client.toml
# Edit ~/client.toml - set server address, cert path, and PSK
sudo redpill-client --config ~/client.toml
```

#### Test mode (no routes, keeps your existing connection)

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk <hex> --test-mode
```

In test mode the tunnel is established but no system routes or DNS are changed. Useful for iperf3 benchmarks or debugging.

### 5. Verify

```bash
# Should show your server's IP
curl -s https://api.ipify.org

# Ping through the tunnel
ping 10.0.1.1
```

---

## Multi-User Management

Instead of a single shared PSK, you can give each user their own key.

### Enable multi-user mode

Add `users_dir` to the server config:

```toml
users_dir = "/etc/redpill/users"
```

### Add a user

```bash
# Generate a random PSK for the user
openssl rand -hex 32 | sudo tee /etc/redpill/users/alice.key
```

Give `alice` her PSK and the server certificate. She connects with:

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk $(cat alice.key)
```

### Remove a user

```bash
sudo rm /etc/redpill/users/alice.key
sudo kill -HUP $(pidof redpill-server)   # Reload without restart
```

### List users

```bash
ls /etc/redpill/users/
# alice.key  bob.key  charlie.key
```

Each `.key` file is a 64-character hex PSK. The filename (without `.key`) is the username shown in logs and metrics.

> **Backward compatibility:** If `psk_file` is also set, its key is accepted as a fallback (shown as user `legacy` in logs). To migrate: copy the old PSK into `users_dir/legacy.key` and remove `psk_file`.

---

## Transport Modes

RedpillVPN supports 5 transport modes. Choose the one that fits your network:

| Mode | Protocol | When to use |
|------|----------|-------------|
| `quic` | Direct QUIC DATAGRAM over UDP:443 | Default. Best performance on unrestricted networks |
| `quic-camouflaged` | QUIC + SNI rotation + padding + browser fingerprint | Networks with light DPI (SNI-based filtering) |
| `tcp-reality` | TLS-over-TCP with active probe deflection | QUIC is blocked, TCP+TLS still works |
| `websocket` | WebSocket binary frames through a CDN | Only CDN-fronted access works |
| `auto` | Probes all transports, picks the best one | Recommended for censored or unknown networks |

### Configuring transports

Transport selection is done in the client config file (`[transport]` section):

#### Direct QUIC (default)

```toml
[server]
address = "1.2.3.4:443"
cert = "cert.pem"
psk = "your-psk-hex"

[transport]
mode = "quic"
```

#### QUIC Camouflaged

Makes your QUIC traffic look like regular browser HTTPS:

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

When QUIC/UDP is completely blocked. The server accepts TCP+TLS on a separate port and deflects non-VPN probes to a real website:

Server config:
```toml
[reality]
enabled = true
listen = "0.0.0.0:8443"
target = "www.google.com:443"   # Probes see this real website
```

Client config:
```toml
[transport]
mode = "tcp-reality"

[reality]
target = "www.google.com:443"
address = "1.2.3.4:8443"       # Server's Reality port
```

#### WebSocket CDN

Route traffic through a CDN (e.g. Cloudflare) to hide the server IP:

Server config:
```toml
[websocket]
enabled = true
listen = "127.0.0.1:8080"      # Behind CDN reverse proxy
```

Client config:
```toml
[transport]
mode = "websocket"

[websocket]
url = "wss://cdn.example.com/ws"
host = "cdn.example.com"
```

#### Auto Mode (recommended for censored networks)

Probes transports in priority order and picks the first one that works:

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

The client tries: QUIC → QUIC Camouflaged → TCP Reality → WebSocket. If the active transport degrades, the health monitor triggers a switch.

---

## Daemon Mode (Linux / macOS)

Run the client as a background service:

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

Logs are written to `/tmp/redpill-client.log`. The PID file is at `/tmp/redpill-client.pid`.

> **Note:** Daemon mode is not available on Windows. Use `redpill-client connect` (foreground) instead.

---

## Server Deployment

### systemd Service

Create `/etc/systemd/system/redpill-quic.service`:

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

### SIGHUP Reload

Reload configuration without restarting:

```bash
sudo kill -HUP $(pidof redpill-server)
```

**Reloadable:** PSK, user keys, max_connections, decoy page, log level.
**Not reloadable (requires restart):** listen address, TUN config, certificates, NAT interface.

### Prometheus Metrics

```bash
curl http://127.0.0.1:9093/metrics
```

Available metrics:

| Metric | Type | Description |
|--------|------|-------------|
| `redpill_active_sessions` | gauge | Currently connected clients |
| `redpill_sessions_by_user{user}` | gauge | Sessions per user |
| `redpill_bytes_in` / `_out` | counter | Total bytes received / sent |
| `redpill_datagrams_in` / `_out` | counter | Total datagrams received / sent |
| `redpill_handshakes_total` | counter | Total handshake attempts |
| `redpill_handshakes_failed` | counter | Failed handshakes (bad PSK) |
| `redpill_drops_backpressure` | counter | Packets dropped by backpressure |
| `redpill_drops_rate_limit` | counter | Packets dropped by rate limiter |
| `redpill_drops_stale` | counter | Stale realtime packets dropped |
| `redpill_rtt_ms` | histogram | Round-trip time distribution |

### XDP Performance (Linux)

Build with the `xdp` feature for conntrack bypass on high-throughput servers:

```bash
cargo build --release --features xdp
```

This adds `iptables -t raw -j NOTRACK` rules for UDP:443, skipping connection tracking in the kernel. The rules are automatically cleaned up on graceful shutdown.

XDP also tunes socket buffers (8 MB) and disables UDP GRO for consistent latency.

---

## Configuration Reference

### Server Config

Full example: [`config/server.example.toml`](config/server.example.toml)

| Key | Default | Description |
|-----|---------|-------------|
| `listen` | `0.0.0.0:443` | Listen address (UDP) |
| `tun_name` | `redpill1` | TUN device name |
| `tun_address` | `10.0.1.1` | Server tunnel IP |
| `tun_prefix_len` | `24` | Subnet prefix length |
| `mtu` | `1200` | Initial TUN MTU (auto-updated via PMTU) |
| `max_connections` | `64` | Max simultaneous clients |
| `max_bandwidth_mbps` | `0` | Per-client bandwidth cap (0 = unlimited) |
| `metrics_listen` | `127.0.0.1:9093` | Prometheus metrics address |
| `psk_file` | - | Path to shared PSK file (single-user mode) |
| `users_dir` | - | Directory with per-user `.key` files (multi-user mode) |
| `cert_file` | `cert.pem` | TLS certificate path |
| `key_file` | `key.pem` | TLS private key path |
| `dns` | `1.1.1.1` | DNS server pushed to clients |
| `log_level` | `info` | Log level (`trace`, `debug`, `info`, `warn`, `error`) |
| `nat.enabled` | `true` | Enable NAT masquerade |
| `nat.interface` | `ens1` | WAN interface for NAT |
| `decoy.enabled` | `true` | Enable HTTP/3 decoy for probe resistance |
| `decoy.page` | - | Path to HTML file served as decoy |
| `reality.enabled` | `false` | Enable TCP Reality listener |
| `reality.listen` | `0.0.0.0:8443` | TCP Reality listen address |
| `reality.target` | `www.google.com:443` | Real website for probe deflection |
| `websocket.enabled` | `false` | Enable WebSocket listener |
| `websocket.listen` | `127.0.0.1:8080` | WebSocket listen address |

### Client Config

Full example: [`config/client.example.toml`](config/client.example.toml)

| Section | Key | Default | Description |
|---------|-----|---------|-------------|
| `[server]` | `address` | - | Server IP:port |
| | `cert` | - | Path to server certificate |
| | `psk` | - | 64-char hex PSK |
| | `domain` | - | Domain for WebPKI verification (instead of cert pinning) |
| `[transport]` | `mode` | `auto` | Transport mode (see above) |
| `[camouflage]` | `sni_pool` | Google domains | SNI domains to rotate |
| | `padding` | `true` | Pad packets to standard sizes |
| | `chrome_fingerprint` | `true` | Mimic browser TLS fingerprint |
| | `browser_profile` | `chrome` | Browser profile: `chrome`, `firefox`, `safari`, `random` |
| `[reality]` | `target` | `www.google.com:443` | SNI target for Reality |
| | `address` | - | Override server address for Reality port |
| `[websocket]` | `url` | - | WebSocket URL |
| | `host` | - | Host header for CDN |

### Client CLI Flags

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

## Architecture

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

### Server data path

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

- **Single TUN reader** - one task reads all packets, demuxes by destination IP
- **Priority classification** - DSCP EF, small UDP (<300 B), DNS → realtime; everything else → bulk
- **Backpressure** - realtime: check buffer space first, drop if full; bulk: `send_datagram_wait` with 25 ms timeout

### Protocol

#### Control stream (first bidirectional QUIC stream)

```
Client → Server: [0x01][32B nonce][32B HMAC-SHA256(psk, nonce)][1B version]   (66 bytes)
Server → Client: [0x02][4B client_ip][4B server_ip][4B dns][2B mtu][1B keepalive][1B flags]  (17 bytes)
```

Flags: bit 0 = batching supported. Auth failure closes connection with error code `0x01`.

#### DATAGRAM frames

Raw IP packets, or batched small packets:
```
[2B length BE][payload][2B length BE][payload]...
```

Heuristic: IP packets start with nibble 4/6, batched datagrams start with 0x00/0x01.

#### ALPN

- `redpill-vpn-1` - VPN tunnel
- `h3` - HTTP/3 decoy

---

## Testing

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

## Roadmap

The following features are planned but not yet implemented:

**Networking:**
- [ ] IPv6 dual-stack (`[::]:443`, tunnel `fd00:rpll::/64`)
- [ ] Proactive QUIC connection migration (network change detection)
- [ ] Multi-server failover (server list with priority)
- [ ] Multi-path (Wi-Fi + cellular simultaneously)

**Performance:**
- [ ] io_uring UDP backend (Linux 5.10+)
- [ ] Full AF_XDP kernel bypass (Linux 5.9+)

**Security:**
- [ ] Per-IP handshake rate limiting (brute-force protection)
- [ ] Let's Encrypt autocert (current stub generates self-signed certs)

**Observability:**
- [ ] qlog support (RFC 9443)
- [ ] Grafana dashboard templates

**Clients:**
- [ ] iOS client (NEPacketTunnelProvider)
- [ ] Android client
- [ ] Web-based management UI

---

## License

[PolyForm Noncommercial 1.0.0](LICENSE) - free for personal and noncommercial use.
This project is source-available (not OSI Open Source): commercial use requires a separate license.

For commercial licensing, contact: **gegam.m92@gmail.com**
