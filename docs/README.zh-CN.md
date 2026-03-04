[English](../README.md) | [Русский](README.ru.md) | [فارسی](README.fa.md) | [العربية](README.ar.md) | **中文**

# RedpillVPN

快速、抗审查的 VPN，基于 QUIC 构建，Rust 实现。

> **状态：MVP / 早期 alpha。** 核心隧道功能已可用，在 macOS/Linux 上测试通过，但可能存在粗糙的地方、不兼容的改动和缺失的功能。详见[路线图](#路线图)。

通过 QUIC DATAGRAM 帧（RFC 9221）在 UDP:443 上传输原始 IP 数据包，使用 TLS 1.3。外层 QUIC 使用无操作拥塞控制器（固定 16 MB 窗口），因为内层 TCP 已经处理了拥塞控制。支持多种传输模式以适应不同网络环境。

## 功能

### 传输层
- QUIC DATAGRAM 帧（RFC 9221），走 UDP:443
- TLS 1.3，基于 quinn/rustls，自签名证书（首次运行时自动生成）
- 无操作拥塞控制（16 MB 恒定窗口）—— 内层 TCP 自行处理拥塞控制
- 小包批量发送（<300 B：DNS 查询、TCP ACK 等）
- 动态 MTU —— 服务器监测 PMTU 并实时推送给客户端

### 安全
- PSK 认证（HMAC-SHA256，常量时间校验）
- 多用户支持 —— 每个用户独立 PSK（`add-user` / `remove-user` CLI）
- 源 IP 防伪造校验
- Kill-switch：macOS（pf）、Windows（Windows 防火墙）—— 断开时防止流量泄漏
- HTTP/3 诱饵服务器（伪装 nginx 页面，抵御主动探测）

### 抗审查
- **5 种传输模式** —— 直连 QUIC、QUIC + 伪装、TCP Reality、WebSocket CDN、自动
- SNI 伪装，轮询域名轮换
- 浏览器 TLS 指纹模拟 —— Chrome、Firefox、Safari 配置文件（对抗 JA3/JA4 检测）
- 数据包大小归一化到标准 HTTP/3 尺寸（128 / 256 / 512 / 1024 / 1200 / 1400）
- 空闲流量填充（不活跃时发送虚假数据包）
- TCP Reality —— 非 VPN 的 TLS 连接被透明代理到真实网站
- WebSocket CDN —— 通过 CDN 反向代理（Cloudflare 等）建立隧道

### 服务器
- 多客户端，每客户端独立优先级队列和 IP 分配（10.0.1.2-254，最多 253 个客户端）
- 流量优先级 —— 实时流量（小 UDP、DNS、DSCP EF）优先于大流量
- 服务端背压，防止丢包放大
- 自适应 RTT 流量整形 + 单客户端带宽限制
- XDP conntrack 绕过，提高高包速率性能（Linux，可选）
- Prometheus 指标端点（`/metrics`）
- SIGHUP 热重载（PSK、用户、max_connections、诱饵页面、日志级别）
- NAT 伪装 + MSS clamping（自动配置）

### 客户端
- 自动重连，指数退避（1s-30s）+ 抖动
- 健康监测 + 自动传输降级/升级
- 守护进程模式（Linux/macOS）—— `up` / `down` / `status` 命令，IPC 通信
- 启动时自动清理残留状态（路由、DNS、防火墙规则，来自崩溃的会话）

## 支持的平台

| 角色   | 操作系统              | 架构          |
|--------|-----------------------|---------------|
| 服务器 | Linux                 | x86_64, arm64 |
| 客户端 | macOS, Linux, Windows | arm64, x86_64 |

> **注意：** Windows 客户端可以编译，包含完整的 wintun/路由/防火墙支持，但尚未在真实硬件上测试。macOS 和 Linux 客户端已充分测试。

---

## 快速开始

### 1. 编译

```bash
git clone https://github.com/redpill-vpn/redpill.git
cd redpill
cargo build --release
```

编译产物在 `target/release/` 目录：
- `redpill-server` —— VPN 服务器
- `redpill-client` —— VPN 客户端

可选编译特性：

```bash
cargo build --release --features xdp    # XDP conntrack 绕过（仅 Linux 服务器）
cargo build --release --features acme   # ACME/Let's Encrypt stub
```

### 2. 生成 PSK

```bash
openssl rand -hex 32
# 示例输出: a3f7b2c1d4e5f6a7b8c9d0e1f2a3b4c5d6e7f8a9b0c1d2e3f4a5b6c7d8e9f0
```

保存这个 64 位十六进制字符串，服务器和客户端都需要用到。

### 3. 服务器配置

```bash
# 创建配置目录
sudo mkdir -p /etc/redpill

# 复制示例配置
sudo cp config/server.example.toml /etc/redpill/server.toml

# 保存 PSK
echo "YOUR_64_CHAR_HEX_PSK" | sudo tee /etc/redpill/psk

# 安装二进制文件
sudo cp target/release/redpill-server /usr/local/bin/

# 启动服务器
sudo redpill-server -c /etc/redpill/server.toml
```

**重要：** 编辑 `/etc/redpill/server.toml`，把 `nat.interface` 设置为服务器的外网接口（如 `ens1`、`eth0`）。可通过 `ip route show default` 查看。

首次运行时，服务器会自动生成自签名 TLS 证书，路径由配置文件中的 `cert_file` / `key_file` 指定。**把证书文件复制到客户端机器上。**

### 4. 客户端配置

#### 简单方式（CLI 参数）

```bash
sudo redpill-client \
  --server YOUR_SERVER_IP:443 \
  --cert path/to/cert.pem \
  --psk YOUR_64_CHAR_HEX_PSK
```

#### 使用配置文件（推荐）

```bash
sudo cp config/client.example.toml ~/client.toml
# 编辑 ~/client.toml —— 填入服务器地址、证书路径和 PSK
sudo redpill-client --config ~/client.toml
```

#### 测试模式（不修改路由，保持现有网络连接）

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk <hex> --test-mode
```

测试模式下隧道会正常建立，但不会修改系统路由和 DNS。适合跑 iperf3 测速或排查问题。

### 5. 验证

```bash
# 应该显示服务器的 IP
curl -s https://api.ipify.org

# 通过隧道 ping
ping 10.0.1.1
```

---

## 多用户管理

可以给每个用户分配独立的密钥，而不是共享一个 PSK。

### 开启多用户模式

在服务器配置中添加 `users_dir`：

```toml
users_dir = "/etc/redpill/users"
```

### 添加用户

```bash
# 为用户生成随机 PSK
openssl rand -hex 32 | sudo tee /etc/redpill/users/alice.key
```

把 PSK 和服务器证书发给 `alice`，她这样连接：

```bash
sudo redpill-client --server YOUR_SERVER_IP --cert cert.pem --psk $(cat alice.key)
```

### 删除用户

```bash
sudo rm /etc/redpill/users/alice.key
sudo kill -HUP $(pidof redpill-server)   # 不重启，热重载
```

### 列出用户

```bash
ls /etc/redpill/users/
# alice.key  bob.key  charlie.key
```

每个 `.key` 文件是一个 64 位十六进制 PSK。文件名（去掉 `.key`）就是日志和指标中显示的用户名。

> **向后兼容：** 如果同时设置了 `psk_file`，其密钥作为兜底使用（日志中显示为 `legacy` 用户）。迁移方法：把旧 PSK 复制为 `users_dir/legacy.key`，然后删除 `psk_file`。

---

## 传输模式

RedpillVPN 支持 5 种传输模式，根据网络情况选择：

| 模式 | 协议 | 适用场景 |
|------|------|----------|
| `quic` | 直连 QUIC DATAGRAM，走 UDP:443 | 默认模式。网络无限制时性能最好 |
| `quic-camouflaged` | QUIC + SNI 轮换 + 填充 + 浏览器指纹 | 有轻度 DPI（基于 SNI 过滤）的网络 |
| `tcp-reality` | TLS-over-TCP + 主动探测偏转 | QUIC 被封，TCP+TLS 还能用 |
| `websocket` | 通过 CDN 的 WebSocket 二进制帧 | 只有经过 CDN 的流量能通 |
| `auto` | 探测所有传输方式，选最优 | 推荐用于审查严格或未知的网络 |

### 配置传输模式

传输模式在客户端配置文件的 `[transport]` 部分设置：

#### 直连 QUIC（默认）

```toml
[server]
address = "1.2.3.4:443"
cert = "cert.pem"
psk = "your-psk-hex"

[transport]
mode = "quic"
```

#### QUIC 伪装模式

让 QUIC 流量看起来像普通浏览器 HTTPS：

```toml
[transport]
mode = "quic-camouflaged"

[camouflage]
# 轮换的域名（ClientHello 中的 SNI 字段）
sni_pool = ["dl.google.com", "www.google.com", "fonts.gstatic.com", "www.youtube.com"]
# 将数据包填充到标准 HTTP/3 大小
padding = true
# 模拟真实浏览器 TLS 指纹
chrome_fingerprint = true
# 模拟的浏览器: "chrome", "firefox", "safari", 或 "random"
browser_profile = "chrome"
```

#### TCP Reality

当 QUIC/UDP 被完全封锁时使用。服务器在另一个端口监听 TCP+TLS，非 VPN 探测会被偏转到真实网站：

服务器配置：
```toml
[reality]
enabled = true
listen = "0.0.0.0:8443"
target = "www.google.com:443"   # 探测方看到的是这个真实网站
```

客户端配置：
```toml
[transport]
mode = "tcp-reality"

[reality]
target = "www.google.com:443"
address = "1.2.3.4:8443"       # 服务器的 Reality 端口
```

#### WebSocket CDN

通过 CDN（如 Cloudflare）转发流量，隐藏服务器真实 IP：

服务器配置：
```toml
[websocket]
enabled = true
listen = "127.0.0.1:8080"      # 在 CDN 反向代理后面
```

客户端配置：
```toml
[transport]
mode = "websocket"

[websocket]
url = "wss://cdn.example.com/ws"
host = "cdn.example.com"
```

#### 自动模式（推荐用于受审查网络）

按优先级探测各传输方式，选第一个能用的：

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

客户端的探测顺序：QUIC → QUIC 伪装 → TCP Reality → WebSocket。如果当前传输质量下降，健康监测会触发切换。

---

## 守护进程模式（Linux / macOS）

以后台服务方式运行客户端：

```bash
# 后台启动 VPN
sudo redpill-client --config client.toml up

# 查看状态
redpill-client status
# 输出:
# Redpill VPN Client
#   Status:    connected
#   Server:    1.2.3.4:443
#   Transport: QuicRaw
#   Client IP: 10.0.1.2
#   Uptime:    3600s
#   TX: 150.3 MB (125000 pkts)
#   RX: 1200.5 MB (1000000 pkts)

# 停止 VPN
sudo redpill-client down
```

日志写入 `/tmp/redpill-client.log`，PID 文件在 `/tmp/redpill-client.pid`。

> **注意：** Windows 不支持守护进程模式，请使用 `redpill-client connect`（前台运行）。

---

## 服务器部署

### systemd 服务

创建 `/etc/systemd/system/redpill-quic.service`：

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

# 查看日志
journalctl -u redpill-quic -f
```

### SIGHUP 热重载

不重启即可重新加载配置：

```bash
sudo kill -HUP $(pidof redpill-server)
```

**可热重载：** PSK、用户密钥、max_connections、诱饵页面、日志级别。
**需要重启：** 监听地址、TUN 配置、证书、NAT 接口。

### Prometheus 指标

```bash
curl http://127.0.0.1:9093/metrics
```

可用指标：

| 指标 | 类型 | 说明 |
|------|------|------|
| `redpill_active_sessions` | gauge | 当前连接的客户端数 |
| `redpill_sessions_by_user{user}` | gauge | 每用户会话数 |
| `redpill_bytes_in` / `_out` | counter | 总接收/发送字节数 |
| `redpill_datagrams_in` / `_out` | counter | 总接收/发送数据报数 |
| `redpill_handshakes_total` | counter | 总握手次数 |
| `redpill_handshakes_failed` | counter | 失败的握手（错误 PSK） |
| `redpill_drops_backpressure` | counter | 背压丢弃的包 |
| `redpill_drops_rate_limit` | counter | 限速丢弃的包 |
| `redpill_drops_stale` | counter | 过期的实时包丢弃数 |
| `redpill_rtt_ms` | histogram | 往返时间分布 |

### XDP 性能优化（Linux）

编译时启用 `xdp` 特性，可在高吞吐服务器上绕过 conntrack：

```bash
cargo build --release --features xdp
```

这会添加 `iptables -t raw -j NOTRACK` 规则（针对 UDP:443），跳过内核的连接跟踪。正常关闭时规则会自动清理。

XDP 还会调整 socket 缓冲区（8 MB）并禁用 UDP GRO 以获得一致的延迟。

---

## 配置参考

### 服务器配置

完整示例：[`config/server.example.toml`](../config/server.example.toml)

| 键 | 默认值 | 说明 |
|----|--------|------|
| `listen` | `0.0.0.0:443` | 监听地址（UDP） |
| `tun_name` | `redpill1` | TUN 设备名 |
| `tun_address` | `10.0.1.1` | 服务器隧道 IP |
| `tun_prefix_len` | `24` | 子网前缀长度 |
| `mtu` | `1200` | 初始 TUN MTU（通过 PMTU 自动更新） |
| `max_connections` | `64` | 最大同时连接客户端数 |
| `max_bandwidth_mbps` | `0` | 单客户端带宽上限（0 = 不限） |
| `metrics_listen` | `127.0.0.1:9093` | Prometheus 指标地址 |
| `psk_file` | - | PSK 文件路径（单用户模式） |
| `users_dir` | - | 用户 `.key` 文件目录（多用户模式） |
| `cert_file` | `cert.pem` | TLS 证书路径 |
| `key_file` | `key.pem` | TLS 私钥路径 |
| `dns` | `1.1.1.1` | 推送给客户端的 DNS 服务器 |
| `log_level` | `info` | 日志级别（`trace`, `debug`, `info`, `warn`, `error`） |
| `nat.enabled` | `true` | 启用 NAT 伪装 |
| `nat.interface` | `ens1` | NAT 外网接口 |
| `decoy.enabled` | `true` | 启用 HTTP/3 诱饵（抗探测） |
| `decoy.page` | - | 诱饵 HTML 文件路径 |
| `reality.enabled` | `false` | 启用 TCP Reality 监听 |
| `reality.listen` | `0.0.0.0:8443` | TCP Reality 监听地址 |
| `reality.target` | `www.google.com:443` | 探测偏转的真实网站 |
| `websocket.enabled` | `false` | 启用 WebSocket 监听 |
| `websocket.listen` | `127.0.0.1:8080` | WebSocket 监听地址 |

### 客户端配置

完整示例：[`config/client.example.toml`](../config/client.example.toml)

| 部分 | 键 | 默认值 | 说明 |
|------|-----|--------|------|
| `[server]` | `address` | - | 服务器 IP:端口 |
| | `cert` | - | 服务器证书路径 |
| | `psk` | - | 64 位十六进制 PSK |
| | `domain` | - | WebPKI 验证域名（替代证书固定） |
| `[transport]` | `mode` | `auto` | 传输模式（见上文） |
| `[camouflage]` | `sni_pool` | Google 域名 | SNI 轮换域名 |
| | `padding` | `true` | 将数据包填充到标准大小 |
| | `chrome_fingerprint` | `true` | 模拟浏览器 TLS 指纹 |
| | `browser_profile` | `chrome` | 浏览器配置：`chrome`, `firefox`, `safari`, `random` |
| `[reality]` | `target` | `www.google.com:443` | Reality 的 SNI 目标 |
| | `address` | - | 覆盖 Reality 端口的服务器地址 |
| `[websocket]` | `url` | - | WebSocket URL |
| | `host` | - | CDN 的 Host 头 |

### 客户端 CLI 参数

```
redpill-client [OPTIONS] [COMMAND]

Commands:
  connect   前台连接（默认）
  up        后台守护进程启动（仅 Linux/macOS）
  down      停止守护进程
  status    查询守护进程状态

Options:
  -s, --server <IP:PORT>   服务器地址
  -c, --cert <PATH>        服务器证书
      --psk <HEX>          PSK（64 位十六进制）
      --config <PATH>      TOML 配置文件（CLI 参数优先于配置文件）
      --test-mode           不设置路由和 kill-switch
  -q, --quiet              禁止输出周期性统计
```

---

## 架构

```
客户端                              服务器
+---------------+   QUIC:443    +---------------+
| redpill-client|<=============>| redpill-server|
|               | DATAGRAM      |               |
|  TUN 设备     | (原始 IP 包)  |  TUN 设备     |
|  kill-switch  |               |  iptables NAT |
|  路由/DNS     |               |  MSS clamping |
+---------------+               +---------------+
```

### 服务器数据路径

```
Internet ← NAT ← TUN 设备 ← 全局 TUN 读取器
                                    ↓
                            提取目标 IP
                                    ↓
                         ClientRouter (DashMap)
                                    ↓
                          PriorityQueue (每客户端)
                           ├── 实时通道
                           └── 大流量通道
                                    ↓
                         QUIC DATAGRAM → 客户端
```

- **单 TUN 读取器** —— 一个任务读取所有数据包，按目标 IP 分发
- **优先级分类** —— DSCP EF、小 UDP（<300 B）、DNS → 实时；其余 → 大流量
- **背压** —— 实时：先检查缓冲区空间，满则丢弃；大流量：`send_datagram_wait` + 25 ms 超时

### 协议

#### 控制流（第一条双向 QUIC 流）

```
客户端 → 服务器: [0x01][32B nonce][32B HMAC-SHA256(psk, nonce)][1B version]   (66 bytes)
服务器 → 客户端: [0x02][4B client_ip][4B server_ip][4B dns][2B mtu][1B keepalive][1B flags]  (17 bytes)
```

Flags: bit 0 = 支持批量发送。认证失败时以错误码 `0x01` 关闭连接。

#### DATAGRAM 帧

原始 IP 包，或批量小包：
```
[2B length BE][payload][2B length BE][payload]...
```

判断方式：IP 包以 nibble 4/6 开头，批量数据报以 0x00/0x01 开头。

#### ALPN

- `redpill-vpn-1` —— VPN 隧道
- `h3` —— HTTP/3 诱饵

---

## 测试

```bash
# 单元测试 + 集成测试
cargo test -p redpill-quic

# 吞吐量测试（需要 VPN 运行中 + 服务端运行 iperf3）
iperf3 -c 10.0.1.1 -t 10 -R    # 下载
iperf3 -c 10.0.1.1 -t 10        # 上传

# IP 泄漏检查
curl -s https://api.ipify.org    # 应显示服务器 IP

# DNS 泄漏检查
dig +short myip.opendns.com @resolver1.opendns.com
```

---

## 路线图

以下功能已计划但尚未实现：

**网络：**
- [ ] IPv6 双栈（`[::]:443`，隧道 `fd00:rpll::/64`）
- [ ] 主动 QUIC 连接迁移（网络切换检测）
- [ ] 多服务器故障转移（带优先级的服务器列表）
- [ ] 多路径（Wi-Fi + 蜂窝同时使用）

**性能：**
- [ ] io_uring UDP 后端（Linux 5.10+）
- [ ] 完整 AF_XDP 内核绕过（Linux 5.9+）

**安全：**
- [ ] 单 IP 握手速率限制（防暴力破解）
- [ ] Let's Encrypt 自动证书（目前的 stub 只生成自签名证书）

**可观测性：**
- [ ] qlog 支持（RFC 9443）
- [ ] Grafana 仪表盘模板

**客户端：**
- [ ] iOS 客户端（NEPacketTunnelProvider）
- [ ] Android 客户端
- [ ] Web 管理界面

---

## 许可证

[PolyForm Noncommercial 1.0.0](../LICENSE) —— 个人和非商业使用免费。
本项目属于 source-available（不是 OSI Open Source）：商业用途需要单独授权。

商业授权请联系：**gegam.m92@gmail.com**
