//! A very small HTTP/1.1 server for the local console.
//!
//! Hand-rolled rather than pulled from a framework: the console needs to serve
//! one page and a handful of JSON endpoints on the loopback interface, and the
//! run state it exposes is `!Send`, so a framework built around `Send` futures
//! would need working around anyway.
//!
//! Deliberately not general purpose. It binds loopback only, speaks one request
//! per connection, and caps what it will read.

use std::future::Future;
use std::io;
use std::net::SocketAddr;
use std::pin::Pin;
use std::rc::Rc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, warn};

/// Largest request head accepted, headers included.
const MAX_HEAD: usize = 16 * 1024;
/// Largest body accepted. A brief is prose, not an upload.
const MAX_BODY: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Request {
    pub method: String,
    /// Path with the query string removed.
    pub path: String,
    /// Raw query string, without the leading `?`.
    pub query: String,
    pub body: String,
}

impl Request {
    /// Reads one query parameter, percent-decoding the value.
    pub fn query_param(&self, name: &str) -> Option<String> {
        self.query.split('&').find_map(|pair| {
            let (key, value) = pair.split_once('=')?;
            (key == name).then(|| percent_decode(value))
        })
    }

    /// The path segment after `prefix`, when the path starts with it.
    ///
    /// Used for `/api/runs/<id>/log` style routing without a router.
    pub fn segment_after(&self, prefix: &str) -> Option<&str> {
        let rest = self.path.strip_prefix(prefix)?;
        let segment = rest.split('/').next()?;
        (!segment.is_empty()).then_some(segment)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Response {
    pub status: u16,
    pub content_type: String,
    pub body: Vec<u8>,
}

impl Response {
    pub fn html(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            content_type: "text/html; charset=utf-8".into(),
            body: body.into(),
        }
    }

    pub fn json(body: impl Into<Vec<u8>>) -> Self {
        Self {
            status: 200,
            content_type: "application/json; charset=utf-8".into(),
            body: body.into(),
        }
    }

    /// A JSON error the console can display verbatim.
    pub fn error(status: u16, message: &str) -> Self {
        let body = serde_json::json!({ "error": message }).to_string();
        Self {
            status,
            content_type: "application/json; charset=utf-8".into(),
            body: body.into_bytes(),
        }
    }

    fn reason(&self) -> &'static str {
        match self.status {
            200 => "OK",
            400 => "Bad Request",
            404 => "Not Found",
            405 => "Method Not Allowed",
            409 => "Conflict",
            413 => "Payload Too Large",
            _ => "Internal Server Error",
        }
    }

    fn encode(&self) -> Vec<u8> {
        let head = format!(
            "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
            self.status,
            self.reason(),
            self.content_type,
            self.body.len()
        );
        let mut out = head.into_bytes();
        out.extend_from_slice(&self.body);
        out
    }
}

/// Handles one request. Synchronous: everything the console touches is in
/// memory, and keeping it sync means handlers can hold `!Send` state directly.
pub type Handler = Rc<dyn Fn(Request) -> Response>;

/// Serves until the process ends. Loopback only.
pub fn serve(addr: SocketAddr, handler: Handler) -> Pin<Box<dyn Future<Output = io::Result<()>>>> {
    Box::pin(async move {
        if !addr.ip().is_loopback() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "the console binds loopback only",
            ));
        }

        let listener = TcpListener::bind(addr).await?;
        debug!("console listening on http://{addr}");

        loop {
            let (stream, _) = listener.accept().await?;
            let handler = handler.clone();
            tokio::task::spawn_local(async move {
                if let Err(e) = handle_connection(stream, handler).await {
                    debug!("console connection ended: {e}");
                }
            });
        }
    })
}

async fn handle_connection(mut stream: TcpStream, handler: Handler) -> io::Result<()> {
    let response = match read_request(&mut stream).await {
        Ok(Some(request)) => handler(request),
        Ok(None) => return Ok(()),
        Err(status) => Response::error(status, "malformed request"),
    };

    stream.write_all(&response.encode()).await?;
    stream.flush().await?;
    Ok(())
}

/// Reads one request. `Err(status)` means the request was refused.
async fn read_request(stream: &mut TcpStream) -> Result<Option<Request>, u16> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];

    let head_end = loop {
        let read = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if read == 0 {
            // Connection closed before a complete head arrived.
            return if buffer.is_empty() {
                Ok(None)
            } else {
                Err(400)
            };
        }
        buffer.extend_from_slice(&chunk[..read]);

        if let Some(position) = find_head_end(&buffer) {
            break position;
        }
        if buffer.len() > MAX_HEAD {
            return Err(413);
        }
    };

    let head = String::from_utf8_lossy(&buffer[..head_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().ok_or(400u16)?;
    let mut parts = request_line.split_whitespace();
    let method = parts.next().ok_or(400u16)?.to_string();
    let target = parts.next().ok_or(400u16)?;

    let content_length = lines
        .filter_map(|line| line.split_once(':'))
        .find(|(name, _)| name.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, value)| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    if content_length > MAX_BODY {
        return Err(413);
    }

    let mut body = buffer[head_end + 4..].to_vec();
    while body.len() < content_length {
        let read = stream.read(&mut chunk).await.map_err(|_| 400u16)?;
        if read == 0 {
            break;
        }
        body.extend_from_slice(&chunk[..read]);
    }
    body.truncate(content_length);

    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path.to_string(), query.to_string()),
        None => (target.to_string(), String::new()),
    };

    Ok(Some(Request {
        method,
        path,
        query,
        body: String::from_utf8_lossy(&body).to_string(),
    }))
}

fn find_head_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// Decodes `%XX` escapes and `+`, leaving anything malformed as-is.
fn percent_decode(value: &str) -> String {
    let bytes = value.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'+' => {
                out.push(b' ');
                index += 1;
            }
            b'%' if index + 2 < bytes.len() => {
                let hex = std::str::from_utf8(&bytes[index + 1..index + 3]).unwrap_or("");
                match u8::from_str_radix(hex, 16) {
                    Ok(byte) => {
                        out.push(byte);
                        index += 3;
                    }
                    Err(_) => {
                        out.push(bytes[index]);
                        index += 1;
                    }
                }
            }
            byte => {
                out.push(byte);
                index += 1;
            }
        }
    }

    String::from_utf8_lossy(&out).to_string()
}

/// Reports a bind failure in terms the operator can act on.
pub fn describe_bind_error(addr: SocketAddr, error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::AddrInUse {
        warn!("console port {} already in use", addr.port());
        return format!(
            "port {} is already in use — another console is probably running",
            addr.port()
        );
    }
    format!("cannot listen on {addr}: {error}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(path: &str, query: &str) -> Request {
        Request {
            method: "GET".into(),
            path: path.into(),
            query: query.into(),
            body: String::new(),
        }
    }

    #[test]
    fn query_parameters_are_percent_decoded() {
        let request = request("/api/runs", "after=12&q=a%20b%2Fc&flag=on");

        assert_eq!(request.query_param("after").as_deref(), Some("12"));
        assert_eq!(request.query_param("q").as_deref(), Some("a b/c"));
        assert_eq!(request.query_param("missing"), None);
    }

    #[test]
    fn plus_decodes_to_space() {
        assert_eq!(percent_decode("a+b"), "a b");
    }

    #[test]
    fn a_truncated_escape_is_left_alone() {
        // Better a literal than a panic on input we did not write.
        assert_eq!(percent_decode("100%"), "100%");
        assert_eq!(percent_decode("%zz"), "%zz");
    }

    #[test]
    fn multibyte_escapes_survive() {
        assert_eq!(percent_decode("%E4%B8%AD%E6%96%87"), "中文");
    }

    #[test]
    fn segment_after_extracts_the_run_id() {
        let request = request("/api/runs/run-123/log", "");

        assert_eq!(request.segment_after("/api/runs/"), Some("run-123"));
        assert_eq!(request.segment_after("/api/other/"), None);
    }

    #[test]
    fn segment_after_rejects_an_empty_id() {
        assert_eq!(request("/api/runs/", "").segment_after("/api/runs/"), None);
    }

    #[test]
    fn responses_carry_a_content_length() {
        let encoded = Response::json(r#"{"ok":true}"#).encode();
        let text = String::from_utf8(encoded).unwrap();

        assert!(text.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(text.contains("Content-Length: 11\r\n"));
        assert!(text.ends_with("\r\n\r\n{\"ok\":true}"));
    }

    #[test]
    fn errors_are_json_so_the_console_can_show_them() {
        let response = Response::error(409, "already running");

        assert_eq!(response.status, 409);
        assert_eq!(
            String::from_utf8(response.body).unwrap(),
            r#"{"error":"already running"}"#
        );
    }

    #[test]
    fn head_end_is_found_and_the_body_starts_after_it() {
        let buffer = b"GET / HTTP/1.1\r\n\r\nbody";
        let head_end = find_head_end(buffer).expect("head terminator present");

        assert_eq!(&buffer[..head_end], b"GET / HTTP/1.1");
        assert_eq!(&buffer[head_end + 4..], b"body");
        assert_eq!(find_head_end(b"GET / HTTP/1.1\r\n"), None);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_non_loopback_bind_is_refused() {
        let addr: SocketAddr = "0.0.0.0:0".parse().unwrap();
        let handler: Handler = Rc::new(|_| Response::json("{}"));

        let error = serve(addr, handler).await.unwrap_err();

        assert_eq!(error.kind(), io::ErrorKind::InvalidInput);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_request_round_trips_over_a_socket() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();

                let server = tokio::task::spawn_local(async move {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    read_request(&mut stream).await
                });

                let mut client = TcpStream::connect(addr).await.unwrap();
                client
                    .write_all(
                        b"POST /api/runs?after=3 HTTP/1.1\r\nHost: x\r\nContent-Length: 7\r\n\r\n{\"a\":1}",
                    )
                    .await
                    .unwrap();

                let request = server.await.unwrap().unwrap().unwrap();
                assert_eq!(request.method, "POST");
                assert_eq!(request.path, "/api/runs");
                assert_eq!(request.query_param("after").as_deref(), Some("3"));
                assert_eq!(request.body, r#"{"a":1}"#);
            })
            .await;
    }

    #[tokio::test(flavor = "current_thread")]
    async fn a_body_split_across_packets_is_reassembled() {
        let local = tokio::task::LocalSet::new();
        local
            .run_until(async {
                let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
                let addr = listener.local_addr().unwrap();

                let server = tokio::task::spawn_local(async move {
                    let (mut stream, _) = listener.accept().await.unwrap();
                    read_request(&mut stream).await
                });

                let mut client = TcpStream::connect(addr).await.unwrap();
                client
                    .write_all(b"POST /x HTTP/1.1\r\nContent-Length: 10\r\n\r\n12345")
                    .await
                    .unwrap();
                client.flush().await.unwrap();
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
                client.write_all(b"67890").await.unwrap();

                let request = server.await.unwrap().unwrap().unwrap();
                assert_eq!(request.body, "1234567890");
            })
            .await;
    }
}
