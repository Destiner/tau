//! Development-only OTLP HTTP/JSON export
//! (<https://opentelemetry.io/docs/specs/otel/protocol/exporter/>). Feature-gated and inert
//! without an endpoint; ordinary builds never connect. Reuses local OTLP JSON. Plain HTTP only,
//! for a local Collector; no TLS, redirects, or extra HTTP dependency.
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const IO_TIMEOUT: Duration = Duration::from_millis(1000);
const EXPORT_QUEUE_CAPACITY: usize = 64;

/// Resolved target without a persistent connection; each send connects and closes.
#[derive(Debug, Clone)]
pub struct OtlpTarget {
    host: String,
    port: u16,
    path: String,
    headers: Vec<(String, String)>,
    sender: Option<SyncSender<String>>,
}

impl PartialEq for OtlpTarget {
    fn eq(&self, other: &Self) -> bool {
        self.host == other.host
            && self.port == other.port
            && self.path == other.path
            && self.headers == other.headers
    }
}

impl Eq for OtlpTarget {}

impl OtlpTarget {
    /// Use signal-specific or generic `OTEL_EXPORTER_OTLP_*` settings. Require an endpoint and
    /// `http/json`; never half-implement gRPC/protobuf.
    pub fn from_env(signal_env_var: &str, default_path: &str) -> Option<Self> {
        let signal_protocol_var = signal_env_var.replace("_ENDPOINT", "_PROTOCOL");
        let protocol = std::env::var(signal_protocol_var)
            .or_else(|_| std::env::var("OTEL_EXPORTER_OTLP_PROTOCOL"))
            .unwrap_or_else(|_| "http/protobuf".to_string());
        if protocol != "http/json" {
            return None;
        }
        let signal_specific = std::env::var(signal_env_var).ok();
        let endpoint = signal_specific
            .clone()
            .or_else(|| std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok())?;
        Self::parse(&endpoint, default_path, signal_specific.is_some()).map(Self::with_worker)
    }

    fn parse(endpoint: &str, default_path: &str, signal_specific: bool) -> Option<Self> {
        let without_scheme = endpoint.trim().strip_prefix("http://")?;
        let (authority, path) = match without_scheme.split_once('/') {
            Some((authority, path)) => (authority, format!("/{path}")),
            None => (without_scheme, String::new()),
        };
        let (host, port_str) = match authority.split_once(':') {
            Some((host, port)) => (host, Some(port)),
            None => (authority, None),
        };
        if host.is_empty() {
            return None;
        }
        let port = port_str.and_then(|value| value.parse().ok()).unwrap_or(80);
        let path = if signal_specific {
            if path.is_empty() {
                "/".to_string()
            } else {
                path
            }
        } else if path.is_empty() {
            default_path.to_string()
        } else {
            format!("{}{default_path}", path.trim_end_matches('/'))
        };
        Some(OtlpTarget {
            host: host.to_string(),
            port,
            path,
            headers: parse_headers(),
            sender: None,
        })
    }

    fn with_worker(mut self) -> Self {
        let (sender, receiver) = sync_channel::<String>(EXPORT_QUEUE_CAPACITY);
        let worker_target = self.clone();
        let _ = std::thread::Builder::new()
            .name("tau-otlp-export".to_string())
            .spawn(move || {
                while let Ok(body) = receiver.recv() {
                    let _ = worker_target.send_body(&body);
                }
            });
        self.sender = Some(sender);
        self
    }

    /// Bounded background worker drops external copies when full; local persistence continues.
    /// Parsed test targets send synchronously.
    pub fn send(&self, payload: &serde_json::Value) {
        let body = payload.to_string();
        if let Some(sender) = &self.sender {
            let _ = sender.try_send(body);
        } else {
            let _ = self.send_body(&body);
        }
    }

    fn send_body(&self, body: &str) -> std::io::Result<()> {
        let address = (self.host.as_str(), self.port)
            .to_socket_addrs()?
            .next()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "no address resolved")
            })?;
        let mut stream = TcpStream::connect_timeout(&address, CONNECT_TIMEOUT)?;
        stream.set_write_timeout(Some(IO_TIMEOUT))?;
        stream.set_read_timeout(Some(IO_TIMEOUT))?;

        let mut request = format!(
            "POST {path} HTTP/1.1\r\nHost: {host}\r\nContent-Type: application/json\r\nContent-Length: {length}\r\nConnection: close\r\n",
            path = self.path,
            host = self.host,
            length = body.len(),
        );
        for (key, value) in &self.headers {
            request.push_str(&format!("{key}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes())?;
        stream.write_all(body.as_bytes())?;

        let mut buffer = [0u8; 256];
        let read = stream.read(&mut buffer)?;
        let status_line = std::str::from_utf8(&buffer[..read])
            .ok()
            .and_then(|response| response.lines().next())
            .unwrap_or_default();
        let accepted = status_line
            .split_whitespace()
            .nth(1)
            .is_some_and(|status| status.starts_with('2'));
        if accepted {
            Ok(())
        } else {
            Err(std::io::Error::other("OTLP endpoint rejected payload"))
        }
    }
}

/// Ignore malformed OTLP header entries rather than failing export setup.
fn parse_headers() -> Vec<(String, String)> {
    std::env::var("OTEL_EXPORTER_OTLP_HEADERS")
        .ok()
        .map(|raw| {
            raw.split(',')
                .filter_map(|pair| pair.split_once('='))
                .filter_map(|(key, value)| {
                    let key = key.trim();
                    let value = value.trim();
                    let valid_key = !key.is_empty()
                        && key.bytes().all(|byte| {
                            byte.is_ascii_alphanumeric()
                                || matches!(
                                    byte,
                                    b'!' | b'#'
                                        | b'$'
                                        | b'%'
                                        | b'&'
                                        | b'\''
                                        | b'*'
                                        | b'+'
                                        | b'-'
                                        | b'.'
                                        | b'^'
                                        | b'_'
                                        | b'`'
                                        | b'|'
                                        | b'~'
                                )
                        });
                    let valid_value = !value.bytes().any(|byte| byte == b'\r' || byte == b'\n');
                    (valid_key && valid_value).then(|| (key.to_string(), value.to_string()))
                })
                .collect()
        })
        .unwrap_or_default()
}
