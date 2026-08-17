//! Optional, development-only OTLP export over plain HTTP/JSON, honoring the
//! standard `OTEL_EXPORTER_OTLP_*` environment variables
//! (<https://opentelemetry.io/docs/specs/otel/protocol/exporter/>). Compiled
//! only behind the `otlp_export` Cargo feature — off by default, so an
//! ordinary build never links this module, and never depends on it at
//! runtime — and inert even when compiled in unless an endpoint is actually
//! configured: nothing here ever attempts a network connection on its own.
//!
//! Reuses the exact OTLP JSON `exporter.rs` already builds for local
//! storage, so there is no second payload format to maintain. Uses a raw
//! `std::net::TcpStream` HTTP/1.1 client rather than a new HTTP crate
//! dependency: `POST`ing one already-built JSON body to a local development
//! Collector does not need a full HTTP stack, TLS, redirects, or connection
//! pooling, and adding one would be exactly the kind of dependency this
//! feature must not become. `https://` endpoints are therefore not
//! supported; a local Collector's default OTLP/HTTP endpoint is plain HTTP
//! (`http://localhost:4318`).
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::mpsc::{sync_channel, SyncSender};
use std::time::Duration;

const CONNECT_TIMEOUT: Duration = Duration::from_millis(500);
const IO_TIMEOUT: Duration = Duration::from_millis(1000);
const EXPORT_QUEUE_CAPACITY: usize = 64;

/// One resolved OTLP/HTTP export target: host, port, request path, and any
/// configured headers. Never holds an open connection — `send` connects,
/// writes, and closes per call, since export happens at most once per
/// metrics collection interval or log/span batch, not per record.
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
    /// Resolves the target for one signal from `signal_env_var` (e.g.
    /// `OTEL_EXPORTER_OTLP_LOGS_ENDPOINT`) or the generic
    /// `OTEL_EXPORTER_OTLP_ENDPOINT`, `OTEL_EXPORTER_OTLP_PROTOCOL`, and
    /// `OTEL_EXPORTER_OTLP_HEADERS`. Returns `None` — never sending anything
    /// — unless an endpoint is configured and the protocol is exactly
    /// `http/json`, the only protocol this minimal client implements;
    /// `grpc`/protobuf (the more common OTLP defaults) are deliberately
    /// unsupported rather than half-implemented.
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
        // A signal-specific endpoint is used exactly as given (per spec, it
        // already names a full signal path); the generic base endpoint gets
        // the signal's own default path appended, also per spec.
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

    /// Sends `payload` best-effort without blocking product work. Targets
    /// built from environment configuration use one bounded background
    /// worker; a full queue drops the external copy while local persistence
    /// continues. Directly parsed targets remain synchronous for focused
    /// protocol tests only.
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

/// Parses `OTEL_EXPORTER_OTLP_HEADERS`'s standard `key1=value1,key2=value2`
/// format. Absent or malformed entries are simply omitted, never a reason
/// to fail export setup.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::BufRead;
    use std::net::TcpListener;
    use std::sync::mpsc;

    #[test]
    fn returns_none_when_no_endpoint_is_configured() {
        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        std::env::remove_var("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT");
        std::env::remove_var("OTEL_EXPORTER_OTLP_PROTOCOL");
        std::env::remove_var("OTEL_EXPORTER_OTLP_LOGS_PROTOCOL");

        assert_eq!(
            OtlpTarget::from_env("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT", "/v1/logs"),
            None
        );
    }

    #[test]
    fn returns_none_for_an_unsupported_protocol() {
        std::env::set_var("OTEL_EXPORTER_OTLP_ENDPOINT", "http://localhost:4318");
        std::env::set_var("OTEL_EXPORTER_OTLP_PROTOCOL", "grpc");

        assert_eq!(
            OtlpTarget::from_env("OTEL_EXPORTER_OTLP_LOGS_ENDPOINT", "/v1/logs"),
            None
        );

        std::env::remove_var("OTEL_EXPORTER_OTLP_ENDPOINT");
        std::env::remove_var("OTEL_EXPORTER_OTLP_PROTOCOL");
    }

    #[test]
    fn returns_none_for_an_https_endpoint() {
        assert_eq!(
            OtlpTarget::parse("https://localhost:4318", "/v1/logs", false),
            None
        );
    }

    #[test]
    fn appends_the_signal_default_path_to_a_generic_base_endpoint() {
        let target =
            OtlpTarget::parse("http://localhost:4318", "/v1/logs", false).expect("valid endpoint");
        assert_eq!(target.host, "localhost");
        assert_eq!(target.port, 4318);
        assert_eq!(target.path, "/v1/logs");

        let based = OtlpTarget::parse("http://localhost:4318/otel/", "/v1/logs", false)
            .expect("valid base path");
        assert_eq!(based.path, "/otel/v1/logs");
    }

    #[test]
    fn uses_a_signal_specific_endpoint_exactly_as_given() {
        let target = OtlpTarget::parse(
            "http://collector.internal:4318/custom/logs",
            "/v1/logs",
            true,
        )
        .expect("valid endpoint");
        assert_eq!(target.host, "collector.internal");
        assert_eq!(target.path, "/custom/logs");
    }

    #[test]
    fn defaults_to_port_80_when_none_is_given() {
        let target =
            OtlpTarget::parse("http://localhost", "/v1/logs", false).expect("valid endpoint");
        assert_eq!(target.port, 80);
    }

    /// The reproducible check standing in for "an externally managed
    /// OTel/Grafana stack", which this sandboxed environment cannot run: a
    /// real local `TcpListener` accepts the connection `send` makes, and
    /// this test asserts the bytes on the wire are a well-formed HTTP/1.1
    /// POST carrying exactly the given JSON body. This validates Tau's own
    /// wire-format and protocol correctness; it is not a substitute for
    /// having actually exercised a genuine OTel Collector, which this
    /// environment has no way to stand up.
    #[test]
    fn sends_a_well_formed_http_post_with_the_json_body() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback listener");
        let port = listener.local_addr().expect("local addr").port();
        let (tx, rx) = mpsc::channel();

        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept connection");
            let mut reader = std::io::BufReader::new(stream);
            let mut request_line = String::new();
            reader
                .read_line(&mut request_line)
                .expect("read request line");
            let mut headers = Vec::new();
            let mut content_length = 0usize;
            loop {
                let mut line = String::new();
                reader.read_line(&mut line).expect("read header line");
                let line = line.trim_end().to_string();
                if line.is_empty() {
                    break;
                }
                if let Some((key, value)) = line.split_once(':') {
                    if key.eq_ignore_ascii_case("content-length") {
                        content_length = value.trim().parse().unwrap_or(0);
                    }
                }
                headers.push(line);
            }
            let mut body = vec![0u8; content_length];
            std::io::Read::read_exact(&mut reader, &mut body).expect("read body");
            reader
                .get_mut()
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
                .expect("write response");
            tx.send((
                request_line,
                headers,
                String::from_utf8(body).expect("utf8 body"),
            ))
            .expect("send captured request");
        });

        let target = OtlpTarget {
            host: "127.0.0.1".to_string(),
            port,
            path: "/v1/logs".to_string(),
            headers: vec![("x-tau-test".to_string(), "1".to_string())],
            sender: None,
        };
        target
            .send_body(&serde_json::json!({ "resourceLogs": [] }).to_string())
            .expect("endpoint accepted payload");
        server.join().expect("server thread");

        let (request_line, headers, body) = rx.recv().expect("captured request");
        assert_eq!(request_line.trim_end(), "POST /v1/logs HTTP/1.1");
        assert!(headers
            .iter()
            .any(|line| line.eq_ignore_ascii_case("content-type: application/json")));
        assert!(headers
            .iter()
            .any(|line| line.eq_ignore_ascii_case("x-tau-test: 1")));
        assert_eq!(body, serde_json::json!({ "resourceLogs": [] }).to_string());
    }

    #[test]
    fn bounded_worker_never_blocks_the_telemetry_caller() {
        let target = OtlpTarget::parse("http://127.0.0.1:1", "/v1/logs", false)
            .expect("target")
            .with_worker();
        let started = std::time::Instant::now();
        for _ in 0..1000 {
            target.send(&serde_json::json!({ "resourceLogs": [] }));
        }
        assert!(
            started.elapsed() < Duration::from_millis(100),
            "enqueueing external telemetry blocked for {:?}",
            started.elapsed()
        );
    }

    #[test]
    fn send_never_panics_when_nothing_is_listening() {
        let target = OtlpTarget {
            host: "127.0.0.1".to_string(),
            // Port 0 never accepts a real connection; this proves a refused
            // connection is swallowed rather than propagated.
            port: 1,
            path: "/v1/logs".to_string(),
            headers: Vec::new(),
            sender: None,
        };
        target.send(&serde_json::json!({ "resourceLogs": [] }));
    }

    #[test]
    fn parses_comma_separated_headers() {
        std::env::set_var(
            "OTEL_EXPORTER_OTLP_HEADERS",
            "api-key=secret, x-team = tau,bad header=value,injected=ok%0d%0a,literal=bad\r\nx:1",
        );
        let headers = parse_headers();
        std::env::remove_var("OTEL_EXPORTER_OTLP_HEADERS");
        assert_eq!(
            headers,
            vec![
                ("api-key".to_string(), "secret".to_string()),
                ("x-team".to_string(), "tau".to_string()),
                ("injected".to_string(), "ok%0d%0a".to_string()),
            ]
        );
    }
}
