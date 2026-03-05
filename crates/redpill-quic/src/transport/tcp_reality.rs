//! TCP Reality transport (Mode 2): IP packets framed over TLS stream.
//!
//! When QUIC is fully blocked, this provides a reliable fallback using
//! TCP + TLS 1.3. DPI sees a normal TLS connection to port 443.
//!
//! Wire format: `[2B length BE][IP datagram][2B length BE][IP datagram]...`
//! Each frame is a length-prefixed IP packet.

use std::sync::Arc;

use async_trait::async_trait;
use bytes::Bytes;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::info;

use crate::auth::PskAuthenticator;
use crate::camouflage::CamouflageCertVerifier;
use crate::cert::pem_to_cert_der;
use crate::config::{ClientCamouflageSection, ClientRealitySection};
use crate::control::{ClientAuth, SessionConfig, SESSION_CONFIG_LEN};
use crate::fingerprint::build_camouflaged_rustls_config;
use crate::PROTOCOL_VERSION;

use super::{SendResult, Transport, TransportError, TransportMode, TransportStats};

/// Maximum frame size (IP packet). Prevents malicious peers from sending huge frames.
const MAX_FRAME_SIZE: usize = 65535;

/// TCP Reality transport - length-framed IP packets over TLS stream.
///
/// Thread safety: the TLS stream is split into read/write halves
/// protected by separate mutexes to allow concurrent send/recv.
/// Writer uses BufWriter for efficient TLS record batching.
pub struct TcpRealityTransport {
    writer: Mutex<
        tokio::io::BufWriter<tokio::io::WriteHalf<tokio_rustls::client::TlsStream<TcpStream>>>,
    >,
    reader: Mutex<(
        tokio::io::BufReader<tokio::io::ReadHalf<tokio_rustls::client::TlsStream<TcpStream>>>,
        Vec<u8>,
    )>,
}

impl TcpRealityTransport {
    pub fn new(stream: tokio_rustls::client::TlsStream<TcpStream>) -> Self {
        let _ = stream.get_ref().0.set_nodelay(true);
        let (reader, writer) = tokio::io::split(stream);
        Self {
            writer: Mutex::new(tokio::io::BufWriter::with_capacity(64 * 1024, writer)),
            reader: Mutex::new((
                tokio::io::BufReader::with_capacity(64 * 1024, reader),
                vec![0u8; 1500],
            )),
        }
    }
}

#[async_trait]
impl Transport for TcpRealityTransport {
    async fn send(&self, data: Bytes) -> Result<SendResult, TransportError> {
        if data.len() > MAX_FRAME_SIZE {
            return Ok(SendResult::TooLarge);
        }

        let mut writer = self.writer.lock().await;

        let mut frame = Vec::with_capacity(2 + data.len());
        frame.extend_from_slice(&(data.len() as u16).to_be_bytes());
        frame.extend_from_slice(&data);
        writer
            .write_all(&frame)
            .await
            .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

        Ok(SendResult::Sent)
    }

    async fn recv(&self) -> Result<Bytes, TransportError> {
        let mut guard = self.reader.lock().await;
        let (reader, pkt_buf) = &mut *guard;

        let mut len_buf = [0u8; 2];
        reader
            .read_exact(&mut len_buf)
            .await
            .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

        let len = u16::from_be_bytes(len_buf) as usize;
        if len == 0 || len > MAX_FRAME_SIZE {
            return Err(TransportError::ConnectionLost(format!(
                "invalid frame length: {len}"
            )));
        }

        if len > pkt_buf.len() {
            pkt_buf.resize(len, 0);
        }
        reader
            .read_exact(&mut pkt_buf[..len])
            .await
            .map_err(|e| TransportError::ConnectionLost(e.to_string()))?;

        Ok(Bytes::copy_from_slice(&pkt_buf[..len]))
    }

    fn mode(&self) -> TransportMode {
        TransportMode::TcpReality
    }

    fn stats(&self) -> TransportStats {
        TransportStats::default()
    }

    fn max_datagram_size(&self) -> Option<usize> {
        Some(MAX_FRAME_SIZE)
    }

    async fn flush(&self) -> Result<(), TransportError> {
        let mut writer = self.writer.lock().await;
        writer
            .flush()
            .await
            .map_err(|e| TransportError::ConnectionLost(e.to_string()))
    }
}

/// Builder for TCP Reality connections.
pub struct TcpRealityConnector {
    cert_path: String,
    reality_config: ClientRealitySection,
    camouflage_config: ClientCamouflageSection,
}

impl TcpRealityConnector {
    pub fn new(
        cert_path: String,
        reality_config: ClientRealitySection,
        camouflage_config: ClientCamouflageSection,
    ) -> Self {
        Self {
            cert_path,
            reality_config,
            camouflage_config,
        }
    }

    /// Connect to the server using TCP + TLS Reality.
    ///
    /// 1. TCP connect to server
    /// 2. TLS handshake with fake SNI (target domain) + cert pinning
    /// 3. Send ClientAuth over the TLS stream
    /// 4. Read SessionConfig
    /// 5. Return (TcpRealityTransport, SessionConfig)
    pub async fn connect(
        &self,
        server_addr: &str,
        auth: &PskAuthenticator,
    ) -> anyhow::Result<(TcpRealityTransport, SessionConfig)> {
        let cert_pem = std::fs::read_to_string(&self.cert_path)?;
        let cert_der = pem_to_cert_der(&cert_pem)?;

        let mut root_store = rustls::RootCertStore::empty();
        root_store.add(cert_der.clone())?;

        let verifier = Arc::new(CamouflageCertVerifier::new(&cert_der, Arc::new(root_store)));

        let mut rustls_config =
            build_camouflaged_rustls_config(verifier, self.camouflage_config.chrome_fingerprint)?;
        // TCP Reality uses h2/http1.1 ALPN (not h3 - that's QUIC-only)
        rustls_config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];

        let connector = tokio_rustls::TlsConnector::from(Arc::new(rustls_config));

        let sni_domain = self
            .reality_config
            .target
            .split(':')
            .next()
            .unwrap_or("www.google.com");
        let server_name = rustls::pki_types::ServerName::try_from(sni_domain.to_string())
            .map_err(|e| anyhow::anyhow!("invalid SNI: {e}"))?;

        // Set large buffers BEFORE connect so the SYN carries the correct
        // TCP window scale option. On macOS, explicit SO_RCVBUF is needed
        // because auto-tuning max is only 1MB.
        info!("TCP Reality: connecting to {server_addr}...");
        let addr: std::net::SocketAddr = server_addr
            .parse()
            .map_err(|e| anyhow::anyhow!("invalid address '{server_addr}': {e}"))?;
        let socket = if addr.is_ipv4() {
            tokio::net::TcpSocket::new_v4()?
        } else {
            tokio::net::TcpSocket::new_v6()?
        };
        socket.set_recv_buffer_size(4 * 1024 * 1024)?;
        socket.set_send_buffer_size(4 * 1024 * 1024)?;
        let tcp_stream = socket.connect(addr).await?;
        tcp_stream.set_nodelay(true)?;

        let mut tls_stream = connector.connect(server_name, tcp_stream).await?;
        info!("TCP Reality: TLS handshake complete");

        let mut nonce = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce);
        let mac = auth.compute_mac(&nonce);

        let client_auth = ClientAuth {
            nonce,
            mac,
            version: PROTOCOL_VERSION,
        };
        tls_stream.write_all(&client_auth.encode()).await?;

        let mut config_buf = vec![0u8; SESSION_CONFIG_LEN];
        tls_stream.read_exact(&mut config_buf).await?;

        let session_config = SessionConfig::decode(&config_buf)
            .ok_or_else(|| anyhow::anyhow!("Invalid session config from server"))?;

        info!(
            "TCP Reality: authenticated, assigned IP {}",
            session_config.client_ip
        );

        Ok((TcpRealityTransport::new(tls_stream), session_config))
    }
}

/// Server-side: run VPN tunnel over a TLS stream (TCP Reality mode).
///
/// Simpler than the QUIC tunnel - no batching, no classify, no priority queue.
/// Just length-framed IP packets in both directions.
pub async fn run_tcp_vpn_tunnel(
    stream: tokio_rustls::server::TlsStream<TcpStream>,
    tun_fd: i32,
    client_ip: std::net::Ipv4Addr,
) -> anyhow::Result<()> {
    use crate::datagram::{validate_source_ip, write_to_tun};
    use tokio::io::unix::AsyncFd;

    let (tls_read, tls_write) = tokio::io::split(stream);

    let tun_async_fd =
        AsyncFd::new(unsafe { std::os::fd::BorrowedFd::borrow_raw(tun_fd) }.try_clone_to_owned()?)?;

    #[cfg(target_os = "macos")]
    const TUN_HDR: usize = 4;
    #[cfg(not(target_os = "macos"))]
    const TUN_HDR: usize = 0;

    let tun2tls: tokio::task::JoinHandle<anyhow::Result<()>> = tokio::spawn(async move {
        use std::os::fd::AsRawFd;
        let mut buf_writer = tokio::io::BufWriter::with_capacity(64 * 1024, tls_write);
        let mut tun_buf = vec![0u8; crate::TUN_MTU as usize + 4];
        let mut frame_buf = Vec::with_capacity(2 + 1400);
        let flush_interval = std::time::Duration::from_millis(1);
        let mut needs_flush = false;

        loop {
            let mut guard = if needs_flush {
                tokio::select! {
                    biased;
                    result = tun_async_fd.readable() => result?,
                    _ = tokio::time::sleep(flush_interval) => {
                        buf_writer.flush().await?;
                        needs_flush = false;
                        continue;
                    }
                }
            } else {
                tun_async_fd.readable().await?
            };

            loop {
                let n = match nix::unistd::read(tun_async_fd.as_raw_fd(), &mut tun_buf) {
                    Ok(n) if n > TUN_HDR => n,
                    Ok(_) => break,
                    Err(nix::errno::Errno::EAGAIN) => {
                        guard.clear_ready();
                        break;
                    }
                    Err(e) => return Err(e.into()),
                };

                let ip_pkt = &tun_buf[TUN_HDR..n];
                frame_buf.clear();
                frame_buf.extend_from_slice(&(ip_pkt.len() as u16).to_be_bytes());
                frame_buf.extend_from_slice(ip_pkt);
                buf_writer.write_all(&frame_buf).await?;
                needs_flush = true;
            }
        }
    });

    let tls2tun_result: anyhow::Result<()> = async {
        let mut tls_read = tokio::io::BufReader::with_capacity(64 * 1024, tls_read);
        let mut len_buf = [0u8; 2];
        let mut pkt_buf = vec![0u8; 1500];
        loop {
            tls_read.read_exact(&mut len_buf).await?;
            let len = u16::from_be_bytes(len_buf) as usize;

            if len == 0 || len > MAX_FRAME_SIZE {
                anyhow::bail!("invalid frame length: {len}");
            }

            if len > pkt_buf.len() {
                pkt_buf.resize(len, 0);
            }
            tls_read.read_exact(&mut pkt_buf[..len]).await?;

            if !validate_source_ip(&pkt_buf[..len], client_ip) {
                continue;
            }

            let _ = write_to_tun(tun_fd, &pkt_buf[..len]);
        }
    }
    .await;

    tun2tls.abort();
    tls2tun_result
}
