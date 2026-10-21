//! The run's network proxy (`docs/plans/harness.md`, HS3b): the only way out of the sandbox to the network.
//!
//! A sandboxed command reaches only this proxy on localhost; its `HTTP(S)_PROXY` points here. For every connection the
//! proxy asks the run's [`Gate`] about the host — `Network(crates.io)` rules, the mode, the owner when it is asked —
//! and only then connects. HTTPS goes through `CONNECT` as an opaque tunnel (the proxy sees the host, never the
//! content); plain HTTP is forwarded one request per connection, its request line made relative and its `Host` the one
//! judged. Names are resolved here, outside the sandbox, and an address on this machine, the local network or a
//! link-local one (a cloud's metadata service) is never connected to, whatever name led there.

use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Semaphore;
use tokio::task::{JoinHandle, JoinSet};

use crate::permissions::{Access, Gate};

/// The longest request head read: a request line and headers, not a body.
const MAX_HEAD_BYTES: usize = 64 * 1024;

/// How long a client has to send its request head, and the proxy to reach a host.
const HEAD_TIMEOUT: Duration = Duration::from_secs(30);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Connections served at once; a further client waits for a free one.
const MAX_CONNECTIONS: usize = 64;

/// A pause after a failed `accept` (out of file descriptors), so the loop does not spin.
const ACCEPT_BACKOFF: Duration = Duration::from_millis(100);

/// The tool name a host is judged under, in rules' and events' terms.
pub const NETWORK_TOOL: &str = "network";

/// A proxy on `127.0.0.1`, serving until dropped; dropping it also ends every connection it serves.
pub struct Proxy {
    port: u16,
    task: JoinHandle<()>,
}

/// Which addresses the proxy connects to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// Only public ones (the run's proxy).
    Public,
    /// Also this machine (the tests' origin servers).
    #[cfg(test)]
    Any,
}

impl Proxy {
    /// Starts a proxy that asks `gate` about each host; without a gate it lets every public host through (a library
    /// caller that judges on its own).
    ///
    /// # Errors
    ///
    /// The listening socket cannot be opened.
    pub async fn start(gate: Option<Arc<Gate>>) -> std::io::Result<Self> {
        Self::start_reaching(gate, Reach::Public).await
    }

    async fn start_reaching(gate: Option<Arc<Gate>>, reach: Reach) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let port = listener.local_addr()?.port();
        let task = tokio::spawn(async move {
            // The connections belong to this task: aborting it (the proxy dropped) drops the set, which aborts them.
            let mut connections = JoinSet::new();
            let slots = Arc::new(Semaphore::new(MAX_CONNECTIONS));
            loop {
                let Ok(slot) = Arc::clone(&slots).acquire_owned().await else {
                    return;
                };
                let client = loop {
                    match listener.accept().await {
                        Ok((client, _)) => break client,
                        // Out of descriptors or a connection reset before it was taken: try again, never give up.
                        Err(_) => tokio::time::sleep(ACCEPT_BACKOFF).await,
                    }
                };
                while connections.try_join_next().is_some() {}
                let gate = gate.clone();
                connections.spawn(async move {
                    let _ = serve(client, gate, reach).await;
                    drop(slot);
                });
            }
        });
        Ok(Proxy { port, task })
    }

    /// The port it listens on, on `127.0.0.1`.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// The environment a sandboxed command gets: every common spelling of the proxy variables. Nothing is exempt
    /// (`NO_PROXY`): the sandbox lets a command reach nothing but the proxy.
    pub fn environment(&self) -> Vec<(String, String)> {
        let url = format!("http://127.0.0.1:{}", self.port);
        [
            "HTTP_PROXY",
            "HTTPS_PROXY",
            "ALL_PROXY",
            "http_proxy",
            "https_proxy",
            "all_proxy",
        ]
        .iter()
        .map(|name| ((*name).to_owned(), url.clone()))
        .collect()
    }
}

impl Drop for Proxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// One client connection: read its request head, judge the host, then tunnel or forward.
async fn serve(
    mut client: TcpStream,
    gate: Option<Arc<Gate>>,
    reach: Reach,
) -> std::io::Result<()> {
    let (head, rest) = match tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut client)).await {
        Ok(Ok(Head::Complete(head, rest))) => (head, rest),
        Ok(Ok(Head::TooLarge)) => {
            return refuse(&mut client, 431, "the request head is too large").await;
        }
        Ok(Ok(Head::Closed)) | Ok(Err(_)) => return Ok(()),
        Err(_) => return refuse(&mut client, 408, "no request in time").await,
    };
    let Some(request) = Request::parse(&head) else {
        return refuse(&mut client, 400, "not a proxy request").await;
    };
    if let Some(gate) = &gate {
        let access = Access::Network {
            host: request.host.clone(),
        };
        // An ask may wait for the owner; a client that went away meanwhile (its command ended) is not asked about.
        let verdict = tokio::select! {
            verdict = gate.judge(NETWORK_TOOL, &access) => verdict,
            () = closed(&client) => return Ok(()),
        };
        if let Err(reason) = verdict {
            return refuse(&mut client, 403, &reason).await;
        }
    }
    if !request.tunnel && request.chunked {
        return refuse(&mut client, 501, "a chunked request body is not forwarded").await;
    }
    let mut upstream = match connect(&request.host, request.port, reach).await {
        Ok(upstream) => upstream,
        Err(reason) => return refuse(&mut client, 502, &reason).await,
    };
    if request.tunnel {
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        upstream.write_all(&rest).await?;
        tokio::io::copy_bidirectional(&mut client, &mut upstream).await?;
        return Ok(());
    }
    // One request, then the answer: what the client sends after its body never reaches the host unjudged.
    upstream.write_all(&request.forwarded_head(&head)).await?;
    let length = request.content_length as usize;
    let first = &rest[..rest.len().min(length)];
    upstream.write_all(first).await?;
    let remaining = (length - first.len()) as u64;
    let (mut from_client, _) = client.split();
    tokio::io::copy(&mut (&mut from_client).take(remaining), &mut upstream).await?;
    tokio::io::copy(&mut upstream, &mut client).await?;
    Ok(())
}

/// Resolves `host` and connects to the first of its addresses the proxy may reach.
async fn connect(host: &str, port: u16, reach: Reach) -> Result<TcpStream, String> {
    let addresses: Vec<SocketAddr> =
        tokio::time::timeout(CONNECT_TIMEOUT, tokio::net::lookup_host((host, port)))
            .await
            .map_err(|_| format!("{host} did not resolve in time"))?
            .map_err(|err| format!("cannot resolve {host}: {err}"))?
            .collect();
    let allowed: Vec<SocketAddr> = addresses
        .into_iter()
        .filter(|address| reach != Reach::Public || public(address.ip()))
        .collect();
    if allowed.is_empty() {
        return Err(format!(
            "{host} is on this machine or the local network, which the sandbox does not reach"
        ));
    }
    let mut last = String::new();
    for address in allowed {
        match tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(address)).await {
            Ok(Ok(stream)) => return Ok(stream),
            Ok(Err(err)) => last = err.to_string(),
            Err(_) => last = "timed out".into(),
        }
    }
    Err(format!("cannot reach {host}: {last}"))
}

/// Whether `ip` is an address on the internet — not this machine, the local network, link-local (a cloud's metadata
/// service), multicast or unspecified.
fn public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => {
            let [a, b, ..] = v4.octets();
            !(v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_unspecified()
                || v4.is_broadcast()
                || v4.is_multicast()
                || a == 0
                // Carrier-grade NAT, 100.64.0.0/10.
                || (a == 100 && (64..128).contains(&b)))
        }
        IpAddr::V6(v6) => {
            if let Some(v4) = v6.to_ipv4_mapped() {
                return public(IpAddr::V4(v4));
            }
            let first = v6.segments()[0];
            !(v6.is_loopback()
                || v6.is_unspecified()
                || v6.is_multicast()
                // NAT64 64:ff9b::/96, 6to4 2002::/16 and IPv4-compatible ::/96 can carry a private IPv4 address.
                || (first == 0x64 && v6.segments()[1] == 0xff9b)
                || first == 0x2002
                || v6.segments()[..6].iter().all(|segment| *segment == 0)
                // Unique local fc00::/7 and link-local fe80::/10.
                || (first & 0xfe00) == 0xfc00
                || (first & 0xffc0) == 0xfe80)
        }
    }
}

/// Resolves once the client has closed its side (it sent nothing more and went away); pending while it is connected.
async fn closed(client: &TcpStream) {
    let mut probe = [0u8; 1];
    loop {
        match client.peek(&mut probe).await {
            Ok(0) | Err(_) => return,
            // Bytes waiting (a body, a TLS hello sent early): the client is there; wait for it to go away instead.
            Ok(_) => std::future::pending::<()>().await,
        }
    }
}

/// How reading a request head ended.
enum Head {
    /// The head and the bytes after it.
    Complete(Vec<u8>, Vec<u8>),
    TooLarge,
    /// The client closed before sending a whole head.
    Closed,
}

async fn read_head(client: &mut TcpStream) -> std::io::Result<Head> {
    let mut buffer = Vec::new();
    let mut chunk = [0u8; 4096];
    loop {
        if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
            let rest = buffer.split_off(end + 4);
            return Ok(Head::Complete(buffer, rest));
        }
        if buffer.len() > MAX_HEAD_BYTES {
            return Ok(Head::TooLarge);
        }
        let n = client.read(&mut chunk).await?;
        if n == 0 {
            return Ok(Head::Closed);
        }
        buffer.extend_from_slice(&chunk[..n]);
    }
}

async fn refuse(client: &mut TcpStream, status: u16, reason: &str) -> std::io::Result<()> {
    let text = match status {
        400 => "Bad Request",
        403 => "Forbidden",
        408 => "Request Timeout",
        431 => "Request Header Fields Too Large",
        501 => "Not Implemented",
        _ => "Bad Gateway",
    };
    let body = format!("blocked by the harness's proxy: {reason}\n");
    let reply = format!(
        "HTTP/1.1 {status} {text}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    client.write_all(reply.as_bytes()).await?;
    client.shutdown().await
}

/// What a request head asks for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Request {
    /// `CONNECT`: an opaque tunnel; otherwise a plain HTTP request to forward.
    tunnel: bool,
    host: String,
    port: u16,
    method: String,
    /// The path for a forwarded request (`/index.html`).
    path: String,
    version: String,
    /// The body's length a forwarded request announces (`Content-Length`, else none).
    content_length: u64,
    /// The body comes in chunks (`Transfer-Encoding`), which the proxy does not forward.
    chunked: bool,
}

/// The port of plain HTTP and of a `CONNECT` that names none.
const HTTP_PORT: u16 = 80;
const HTTPS_PORT: u16 = 443;

impl Request {
    fn parse(head: &[u8]) -> Option<Self> {
        let text = std::str::from_utf8(head).ok()?;
        let mut lines = text.split("\r\n");
        let line = lines.next()?;
        let mut parts = line.split(' ');
        let (method, target, version) = (parts.next()?, parts.next()?, parts.next()?);
        if parts.next().is_some() || !version.starts_with("HTTP/") {
            return None;
        }
        let mut content_length = 0;
        let mut chunked = false;
        let mut seen_length = false;
        for header in lines.filter(|line| !line.is_empty()) {
            let (name, value) = header.split_once(':')?;
            match name.trim().to_ascii_lowercase().as_str() {
                // Two lengths could be read differently by the host than here: refused.
                "content-length" if seen_length => return None,
                "content-length" => {
                    content_length = value.trim().parse().ok()?;
                    seen_length = true;
                }
                "transfer-encoding" => chunked = true,
                _ => {}
            }
        }
        if method.eq_ignore_ascii_case("CONNECT") {
            let (host, port) = split_host_port(target, HTTPS_PORT)?;
            return Some(Request {
                tunnel: true,
                host,
                port,
                method: method.to_owned(),
                path: String::new(),
                version: version.to_owned(),
                content_length: 0,
                chunked: false,
            });
        }
        let scheme = target.get(..7)?;
        if !scheme.eq_ignore_ascii_case("http://") {
            return None;
        }
        let rest = &target[7..];
        let (authority, path) = match rest.find('/') {
            Some(at) => (&rest[..at], &rest[at..]),
            None => (rest, "/"),
        };
        let (host, port) = split_host_port(authority, HTTP_PORT)?;
        Some(Request {
            tunnel: false,
            host,
            port,
            method: method.to_owned(),
            path: path.to_owned(),
            version: version.to_owned(),
            content_length,
            chunked,
        })
    }

    /// The head as the origin server should see it: a relative request line, the judged host as `Host`, the
    /// connection closed after the answer, and none of the proxy's own headers.
    fn forwarded_head(&self, head: &[u8]) -> Vec<u8> {
        let text = String::from_utf8_lossy(head);
        let host = if self.host.contains(':') {
            format!("[{}]", self.host)
        } else {
            self.host.clone()
        };
        let authority = if self.port == HTTP_PORT {
            host
        } else {
            format!("{host}:{}", self.port)
        };
        let mut out = format!(
            "{} {} {}\r\nHost: {authority}\r\nConnection: close\r\n",
            self.method, self.path, self.version
        );
        for line in text.split("\r\n").skip(1) {
            if line.is_empty() {
                continue;
            }
            let name = line
                .split(':')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if name.starts_with("proxy-")
                || matches!(name.as_str(), "host" | "connection" | "keep-alive")
            {
                continue;
            }
            out.push_str(line);
            out.push_str("\r\n");
        }
        out.push_str("\r\n");
        out.into_bytes()
    }
}

/// `host:port` (or a bare host and the default port), the host lowercased and checked to be a plain name or address:
/// an IPv6 address only in brackets, a trailing dot dropped (`example.com.` is `example.com`).
fn split_host_port(authority: &str, default_port: u16) -> Option<(String, u16)> {
    let (host, port, bracketed) = if let Some(inner) = authority.strip_prefix('[') {
        let (host, after) = inner.split_once(']')?;
        let port = match after {
            "" => default_port,
            _ => after.strip_prefix(':')?.parse().ok()?,
        };
        (host, port, true)
    } else {
        match authority.split_once(':') {
            Some((host, port)) => (host, port.parse().ok()?, false),
            None => (authority, default_port, false),
        }
    };
    let host = host.strip_suffix('.').unwrap_or(host).to_ascii_lowercase();
    let allowed =
        |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-') || (bracketed && c == ':');
    let plain = !host.is_empty() && host.chars().all(allowed) && (!bracketed || host.contains(':'));
    plain.then_some((host, port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::{Mode, Nobody, Rule, RuleSet};

    fn gate(allow: &[&str]) -> Arc<Gate> {
        let rules = RuleSet {
            allow: allow
                .iter()
                .map(|text| Rule::parse(text).unwrap())
                .collect(),
            ..RuleSet::default()
        };
        Arc::new(Gate::new(rules, Mode::Default, Arc::new(Nobody)))
    }

    /// An origin server that answers each connection with `reply` and keeps what it was sent, until the client stops
    /// sending (or `reply` has been written and the client half-closed).
    async fn origin(reply: &'static str) -> (u16, Arc<tokio::sync::Mutex<Vec<u8>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let seen = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let log = Arc::clone(&seen);
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let mut buffer = [0u8; 4096];
                // Long enough for a test's whole request to arrive in one or two reads.
                let read = tokio::time::timeout(Duration::from_millis(300), async {
                    loop {
                        match socket.read(&mut buffer).await {
                            Ok(0) | Err(_) => break,
                            Ok(n) => log.lock().await.extend_from_slice(&buffer[..n]),
                        }
                    }
                });
                let _ = read.await;
                let _ = socket.write_all(reply.as_bytes()).await;
                let _ = socket.shutdown().await;
            }
        });
        (port, seen)
    }

    async fn proxy(gate: Option<Arc<Gate>>) -> Proxy {
        Proxy::start_reaching(gate, Reach::Any).await.unwrap()
    }

    async fn through(proxy: &Proxy, request: &str) -> String {
        let mut client = TcpStream::connect(("127.0.0.1", proxy.port()))
            .await
            .unwrap();
        client.write_all(request.as_bytes()).await.unwrap();
        let mut answer = Vec::new();
        let _ = client.read_to_end(&mut answer).await;
        String::from_utf8_lossy(&answer).into_owned()
    }

    #[tokio::test]
    async fn an_allowed_host_is_tunnelled() {
        let (port, seen) = origin("hi").await;
        let proxy = proxy(Some(gate(&["Network(127.0.0.1)"]))).await;
        let answer = through(
            &proxy,
            &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\nHost: x\r\n\r\nPING"),
        )
        .await;
        assert_eq!(answer, "HTTP/1.1 200 Connection Established\r\n\r\nhi");
        assert_eq!(
            &seen.lock().await[..],
            b"PING",
            "the tunnel carries the client's bytes as they are"
        );
    }

    #[tokio::test]
    async fn a_forwarded_request_names_the_judged_host_and_carries_one_body_only() {
        let (port, seen) = origin("HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nhi").await;
        let proxy = proxy(Some(gate(&["Network(127.0.0.1)"]))).await;
        let request = format!(
            "POST HTTP://127.0.0.1:{port}/a?b HTTP/1.1\r\nHost: evil.example\r\nProxy-Authorization: k\r\n\
             Connection: keep-alive\r\nContent-Length: 4\r\n\r\nbodyGET http://other.example/ HTTP/1.1\r\n\r\n"
        );
        let answer = through(&proxy, &request).await;
        assert!(answer.ends_with("hi"), "{answer}");
        let sent = String::from_utf8_lossy(&seen.lock().await).into_owned();
        assert!(sent.starts_with("POST /a?b HTTP/1.1\r\n"), "{sent}");
        assert!(
            sent.contains(&format!("Host: 127.0.0.1:{port}\r\n")),
            "{sent}"
        );
        assert!(sent.contains("Connection: close\r\n"), "{sent}");
        let lower = sent.to_lowercase();
        assert!(
            !lower.contains("evil") && !lower.contains("proxy-") && !lower.contains("keep-alive"),
            "{sent}"
        );
        assert!(
            sent.ends_with("\r\n\r\nbody"),
            "the pipelined second request stays behind: {sent}"
        );
    }

    #[tokio::test]
    async fn a_host_no_rule_allows_is_refused_headless_and_logged() {
        let (port, seen) = origin("never").await;
        let gate = gate(&["Network(crates.io)", "Network(*.127.0.0.1)"]);
        let proxy = proxy(Some(Arc::clone(&gate))).await;
        let answer = through(
            &proxy,
            &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\n"),
        )
        .await;
        assert!(answer.starts_with("HTTP/1.1 403 Forbidden"), "{answer}");
        assert!(answer.contains("no one to ask"), "{answer}");
        assert!(seen.lock().await.is_empty(), "nothing reached the host");
        let log = gate.take_log();
        assert_eq!(
            (log[0].tool.as_str(), log[0].target.as_str(), log[0].allowed),
            ("network", "127.0.0.1", false)
        );
    }

    #[tokio::test]
    async fn the_run_proxy_never_connects_to_this_machine_or_the_local_network() {
        let (port, seen) = origin("never").await;
        let proxy = Proxy::start(None).await.unwrap();
        for host in ["127.0.0.1", "localhost", "[::1]"] {
            let answer = through(&proxy, &format!("CONNECT {host}:{port} HTTP/1.1\r\n\r\n")).await;
            assert!(answer.starts_with("HTTP/1.1 502"), "{host}: {answer}");
            assert!(answer.contains("local network"), "{host}: {answer}");
        }
        assert!(seen.lock().await.is_empty());
        for ip in [
            "10.0.0.1",
            "192.168.1.1",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "fd00::1",
            "64:ff9b::a00:1",
            "2002:a00:1::",
            "::10.0.0.1",
            "fe80::1",
            "::ffff:127.0.0.1",
        ] {
            assert!(!public(ip.parse().unwrap()), "{ip}");
        }
        for ip in ["1.1.1.1", "2606:4700::1111"] {
            assert!(public(ip.parse().unwrap()), "{ip}");
        }
    }

    #[tokio::test]
    async fn an_unreachable_host_is_a_bad_gateway() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let proxy = proxy(None).await;
        let answer = through(
            &proxy,
            &format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\n"),
        )
        .await;
        assert!(answer.starts_with("HTTP/1.1 502"), "{answer}");
    }

    #[tokio::test]
    async fn bad_heads_are_answered_and_early_closes_end_quietly() {
        let proxy = proxy(None).await;
        for request in [
            "hello\r\n\r\n",
            "GET https://x/ HTTP/1.1\r\n\r\n",
            "GET http://a b/ HTTP/1.1\r\n\r\n",
            "GET http://a:b:c/ HTTP/1.1\r\n\r\n",
        ] {
            let answer = through(&proxy, request).await;
            assert!(answer.starts_with("HTTP/1.1 400"), "{request:?}: {answer}");
        }
        let twice = "POST http://x/ HTTP/1.1\r\nContent-Length: 1\r\nContent-Length: 9\r\n\r\n";
        assert!(through(&proxy, twice).await.starts_with("HTTP/1.1 400"));
        let chunked = "POST http://127.0.0.1:9/ HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert!(through(&proxy, chunked).await.starts_with("HTTP/1.1 501"));
        let huge = format!(
            "GET http://x/ HTTP/1.1\r\nX: {}",
            "a".repeat(MAX_HEAD_BYTES + 10)
        );
        assert!(through(&proxy, &huge).await.starts_with("HTTP/1.1 431"));
        let mut client = TcpStream::connect(("127.0.0.1", proxy.port()))
            .await
            .unwrap();
        client.write_all(b"CONNECT x:1 HTTP/1.1\r\n").await.unwrap();
        client.shutdown().await.unwrap();
        let mut answer = Vec::new();
        client.read_to_end(&mut answer).await.unwrap();
        assert!(
            answer.is_empty(),
            "closed before a whole head: nothing to answer"
        );
    }

    #[tokio::test]
    async fn dropping_the_proxy_ends_its_open_tunnels() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        // An origin that keeps the connection open and says nothing.
        tokio::spawn(async move {
            let (_socket, _) = listener.accept().await.unwrap();
            std::future::pending::<()>().await;
        });
        let proxy = proxy(None).await;
        let mut client = TcpStream::connect(("127.0.0.1", proxy.port()))
            .await
            .unwrap();
        client
            .write_all(format!("CONNECT 127.0.0.1:{port} HTTP/1.1\r\n\r\n").as_bytes())
            .await
            .unwrap();
        let mut answer = [0u8; 39];
        client.read_exact(&mut answer).await.unwrap();
        drop(proxy);
        let mut rest = Vec::new();
        let read =
            tokio::time::timeout(Duration::from_secs(5), client.read_to_end(&mut rest)).await;
        assert!(read.is_ok(), "the tunnel was closed with the proxy");
    }

    #[test]
    fn hosts_and_ports_are_read_strictly() {
        assert_eq!(
            split_host_port("Crates.IO:443", 80),
            Some(("crates.io".into(), 443))
        );
        assert_eq!(
            split_host_port("example.com.", 80),
            Some(("example.com".into(), 80))
        );
        assert_eq!(
            split_host_port("[::1]:8080", 80),
            Some(("::1".into(), 8080))
        );
        assert_eq!(split_host_port("[::1]", 80), Some(("::1".into(), 80)));
        assert_eq!(split_host_port("a:b:c", 80), None);
        assert_eq!(split_host_port("[example.com]:1", 80), None);
        assert_eq!(split_host_port("a/b:1", 80), None);
        assert_eq!(split_host_port("x:notaport", 80), None);
        assert_eq!(split_host_port("user@x:1", 80), None);
    }
}
