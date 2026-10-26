//! loom-egress-proxy — the enforcing half of Loom's `restricted` network posture.
//!
//! Protocol contract (see docs/decisions/0002-egress-allowlist-proxy.md):
//!
//! * `LOOM_ALLOWED_HOSTS` — CSV of hosts and CIDRs, e.g.
//!   `api.example.com,10.0.0.0/8`. **Must** be set, non-empty, and every entry
//!   valid — the proxy refuses to start otherwise (fail-closed).
//! * `LOOM_EGRESS_PORT` — listen port, default 3128.
//! * `NO_PROXY` is honored by callers: the runtime injects
//!   `NO_PROXY=localhost,127.0.0.1` into the workload, so loopback traffic
//!   never reaches this proxy. Every CONNECT that does arrive is governed by
//!   the allowlist alone.
//!
//! A plain CONNECT tunnel: allowed hosts get `200` and a byte-relay, everyone
//! else gets `403`. No MITM, no inspection — body bytes are never read.
//!
//! Deliberate ceiling:
//! // ponytail: host-only matching (hostname as written, case-insensitive,
//! // trailing dot ignored; CIDR entries apply to IP-literal targets only).
//! // No wildcard/subdomain/path rules — add here if a user asks; the wire
//! // protocol needs no change.

use anyhow::{bail, Context, Result};
use std::collections::HashSet;
use std::io;
use std::net::IpAddr;
use std::sync::Arc;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWriteExt, copy_bidirectional};use tokio::net::{TcpListener, TcpStream};

/// Default port; must stay in sync with the wiring in `loom-runtime`
/// (`prepare_egress` publishes/points HTTP_PROXY at 3128).
const DEFAULT_PORT: u16 = 3128;
const HEAD_LIMIT: usize = 8 * 1024;
const HEAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

/// The parsed allowlist: exact hostnames plus optional CIDR entries.
#[derive(Debug, Clone)]
struct Allowlist {
    hosts: HashSet<String>,
    cidrs: Vec<(IpAddr, u32)>,
}

impl Allowlist {
    fn parse(input: &str) -> Result<Self> {
        let entries: Vec<&str> = input
            .split(',')
            .map(str::trim)
            .filter(|e| !e.is_empty())
            .collect();
        if entries.is_empty() {
            bail!("LOOM_ALLOWED_HOSTS is empty — refusing to start (fail-closed)");
        }
        let mut hosts = HashSet::new();
        let mut cidrs = Vec::new();
        for entry in entries {
            if let Some((ip, prefix)) = entry.split_once('/') {
                cidrs.push(parse_cidr(ip, prefix)?);
            } else if entry
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
            {
                hosts.insert(entry.to_ascii_lowercase());
            } else {
                bail!("invalid LOOM_ALLOWED_HOSTS entry '{entry}' — refusing to start");
            }
        }
        Ok(Self { hosts, cidrs })
    }

    fn is_allowed(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        if self.hosts.contains(&host) {
            return true;
        }
        match host.parse::<IpAddr>() {
            Ok(ip) => self.cidrs.iter().any(|(net, prefix)| cidr_contains(net, *prefix, ip)),
            Err(_) => false, // hostnames are matched as written; no subdomains, no wildcards
        }
    }
}

fn parse_cidr(ip: &str, prefix: &str) -> Result<(IpAddr, u32)> {
    let ip: IpAddr = ip.parse().context("invalid IP in LOOM_ALLOWED_HOSTS CIDR")?;
    let prefix: u32 = prefix.parse().context("invalid prefix in LOOM_ALLOWED_HOSTS CIDR")?;
    let bits = match ip {
        IpAddr::V4(_) => 32,
        IpAddr::V6(_) => 128,
    };
    if prefix > bits {
        bail!("CIDR prefix {prefix} exceeds /{bits}");
    }
    Ok((ip, prefix))
}

fn cidr_contains(net: &IpAddr, prefix: u32, ip: IpAddr) -> bool {
    match (net, ip) {
        (IpAddr::V4(net), IpAddr::V4(ip)) => {
            match_top(u128::from(u32::from(*net)), u128::from(u32::from(ip)), prefix, 32)
        }
        (IpAddr::V6(net), IpAddr::V6(ip)) => match_top(u128::from(*net), u128::from(ip), prefix, 128),
        _ => false, // v4 CIDR never matches a v6 authority or vice versa
    }
}

/// `true` when the top `prefix` bits of `net` and `ip` agree.
fn match_top(net: u128, ip: u128, prefix: u32, bits: u32) -> bool {
    if prefix == 0 {
        true
    } else if prefix >= bits {
        net == ip
    } else {
        let shift = bits - prefix;
        (net >> shift) == (ip >> shift)
    }
}

/// Extract the lowercase hostname from a CONNECT authority (`host:port`,
/// IPv6 `[h]:p`), with the trailing dot trimmed.
fn authority_host(authority: &str) -> String {
    let host = if let Some(rest) = authority.strip_prefix('[') {
        rest.split_once(']').map_or(rest, |(h, _)| h)
    } else {
        match authority.rsplit_once(':') {
            Some((h, p)) if p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty() => h,
            _ => authority,
        }
    };
    host.trim_matches('.').trim_end_matches('.').to_ascii_lowercase()
}

fn authority_port(authority: &str) -> u16 {
    const HTTPS: u16 = 443;
    if let Some(rest) = authority.strip_prefix('[') {
        if let Some((_, p)) = rest.split_once(']') {
            return p.strip_prefix(':').and_then(|p| p.parse().ok()).unwrap_or(HTTPS);
        }
        return HTTPS;
    }
    match authority.rsplit_once(':') {
        Some((_, p)) => p.parse().unwrap_or(HTTPS),
        None => HTTPS,
    }
}

/// Read the request head (up to `\r\n\r\n`), capped at `HEAD_LIMIT`.
async fn read_head<S: AsyncRead + Unpin>(client: &mut S) -> io::Result<Option<String>> {
    let mut buf = Vec::with_capacity(128);
    let mut byte = [0u8; 1];
    loop {
        if buf.len() > HEAD_LIMIT {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "request head too large"));
        }
        match client.read(&mut byte).await {
            Ok(0) => return Ok(None),
            Ok(_) => {
                buf.push(byte[0]);
                if buf.ends_with(b"\r\n\r\n") {
                    let _ = buf.len();
                    break;
                }
            }
            Err(e) => return Err(e),
        }
    }
    Ok(Some(String::from_utf8_lossy(&buf).into_owned()))
}

#[tokio::main]
async fn main() -> Result<()> {
    let raw = match std::env::var("LOOM_ALLOWED_HOSTS") {
        Ok(raw) => raw,
        Err(_) => bail!("LOOM_ALLOWED_HOSTS not set — refusing to start (fail-closed)"),
    };
    let allowlist = Arc::new(Allowlist::parse(&raw)?);

    let port: u16 = match std::env::var("LOOM_EGRESS_PORT") {
        Ok(v) => v.parse().context("invalid LOOM_EGRESS_PORT")?,
        Err(_) => DEFAULT_PORT,
    };

    let listener = TcpListener::bind(("0.0.0.0", port))
        .await
        .with_context(|| format!("failed to bind 0.0.0.0:{port}"))?;
    eprintln!("EGRESS allowlist: {raw}");
    eprintln!("EGRESS listening on 0.0.0.0:{port}");

    loop {
        let (stream, _peer) = listener.accept().await?;
        let allowlist = allowlist.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(stream, allowlist).await {
                eprintln!("EGRESS connection error: {e}");
            }
        });
    }
}

async fn handle(mut client: TcpStream, allowlist: Arc<Allowlist>) -> Result<()> {
    let head = tokio::time::timeout(HEAD_TIMEOUT, read_head(&mut client)).await??;
    let Some(head) = head else {
        return Ok(()); // client hung up mid-head
    };
    let request = head.lines().next().unwrap_or_default().trim().to_string();
    let Some(target) = request
        .split_whitespace()
        .nth(1)
        .filter(|_| request.starts_with("CONNECT "))
    else {
        client
            .write_all(b"HTTP/1.1 405 Method Not Allowed\r\nConnection: close\r\n\r\n")
            .await?;
        // The allowlist covers CONNECT only; plaintext HTTP proxying is
        // deliberately out of scope (see the module ceiling note).
        return Ok(());
    };

    let host = authority_host(target);
    if !allowlist.is_allowed(&host) {
        eprintln!("EGRESS DENY {host}");
        client
            .write_all(b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\n\r\n")
            .await?;
        return Ok(());
    }

    let port = authority_port(target);
    let mut upstream = match TcpStream::connect((host.as_str(), port)).await {
        Ok(stream) => stream,
        Err(e) => {
            eprintln!("EGRESS FAIL {host}:{port} {e}");
            client
                .write_all(b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\n\r\n")
                .await?;
            return Ok(());
        }
    };
    eprintln!("EGRESS ALLOW {host}:{port}");

    client
        .write_all(b"HTTP/1.1 200 Connection established\r\n\r\n")
        .await?;
    client.flush().await?;

    match copy_bidirectional(&mut client, &mut upstream).await {
        Ok((_, _)) => Ok(()),
        Err(e) => Err(e).context("tunnel relay failed"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn allow(input: &str) -> Allowlist {
        Allowlist::parse(input).expect("valid allowlist")
    }

    // --- env contract (fail-closed) ---------------------------------------

    #[test]
    fn empty_or_missing_env_refuses_to_start() {
        assert!(Allowlist::parse("").is_err());
        assert!(Allowlist::parse("   ").is_err());
        assert!(Allowlist::parse("  ,  , ").is_err());
    }

    #[test]
    fn malformed_entries_refuse_to_start() {
        assert!(Allowlist::parse("api.example.com,10.0.0.0/33").is_err());
        assert!(Allowlist::parse("a b.com").is_err()); // whitespace
        assert!(Allowlist::parse("evil/8/extra").is_err());
        assert!(Allowlist::parse("api.com;rm -rf").is_err());
    }

    // --- matching ----------------------------------------------------------

    #[test]
    fn listed_host_matches_as_written() {
        let a = allow("API.example.com");
        assert!(a.is_allowed("api.example.com"));
        assert!(a.is_allowed("api.example.com.")); // trailing dot
        // Exact-as-written: no subdomain, no suffix games.
        assert!(!a.is_allowed("evil-api.example.com"));
        assert!(!a.is_allowed("notexample.com"));
    }

    #[test]
    fn cidr_matches_ip_literals_only() {
        let a = allow("10.0.0.0/8,192.168.1.0/24,::1/128");
        assert!(a.is_allowed("10.42.1.250"));
        assert!(a.is_allowed("192.168.1.7"));
        assert!(!a.is_allowed("192.168.2.7"));
        assert!(!a.is_allowed("11.0.0.1"));
        assert!(a.is_allowed("::1"));
        // A CIDR entry does NOT shortcut a hostname.
        let bare = allow("api.example.com,10.0.0.0/8");
        assert!(!bare.is_allowed("10.0.0.1reich"));
        assert!(bare.is_allowed("10.0.0.1"));
    }

    // --- authority parsing --------------------------------------------------

    #[test]
    fn authority_host_strips_port_brackets_and_dot() {
        assert_eq!(authority_host("api.example.com:443"), "api.example.com");
        assert_eq!(authority_host("api.example.com"), "api.example.com");
        assert_eq!(authority_host("[2001:db8::1]:443"), "2001:db8::1");
        assert_eq!(authority_host("API.Example.COM."), "api.example.com");
    }

    #[test]
    fn authority_port_defaults_to_https() {
        assert_eq!(authority_port("h:8443"), 8443);
        assert_eq!(authority_port("h"), 443);
        assert_eq!(authority_port("[::1]:8443"), 8443);
        assert_eq!(authority_port("[::1]"), 443);
    }

    // --- end-to-end handler behaviour over real loopback sockets ------------

    async fn serve(allow: Allowlist) -> std::net::SocketAddr {
        let listener = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let addr = listener.local_addr().unwrap();
        let allow = Arc::new(allow);
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                let allow = allow.clone();
                tokio::spawn(async move {
                    let _ = handle(stream, allow).await;
                });
            }
        });
        addr
    }

    async fn response_prefix(stream: &mut TcpStream) -> String {
        let mut buf = Vec::new();
        let mut byte = [0u8; 1];
        while !buf.ends_with(b"\r\n\r\n") {
            stream.read(&mut byte).await.unwrap();
            buf.push(byte[0]);
        }
        String::from_utf8_lossy(&buf).into_owned()
    }

    #[tokio::test]
    async fn listed_target_is_tunneled_both_ways() {
        // Real upstream: echoes what it receives.
        let upstream = TcpListener::bind(("127.0.0.1", 0)).await.unwrap();
        let upstream_port = upstream.local_addr().unwrap().port();
        tokio::spawn(async move {
            let (mut sock, _) = upstream.accept().await.unwrap();
            let mut buf = vec![0u8; 16];
            let n = sock.read(&mut buf).await.unwrap();
            sock.write_all(&buf[..n]).await.unwrap();
        });

        let proxy_addr = serve(allow("127.0.0.1")).await;
        let mut client = TcpStream::connect(proxy_addr).await.unwrap();
        client
            .write_all(
                format!("CONNECT 127.0.0.1:{upstream_port} HTTP/1.1\r\nHost: 127.0.0.1\r\n\r\n")
                    .as_bytes(),
            )
            .await
            .unwrap();
        assert!(response_prefix(&mut client).await.starts_with("HTTP/1.1 200"));

        client.write_all(b"ping").await.unwrap();
        let mut buf = vec![0u8; 4];
        client.read(&mut buf).await.unwrap();
        assert_eq!(&buf, b"ping");
    }

    #[tokio::test]
    async fn unlisted_target_refused() {
        let proxy_addr = serve(allow("api.example.com")).await;
        let mut client = TcpStream::connect(proxy_addr).await.unwrap();
        client
            .write_all(b"CONNECT evil.example.com:443 HTTP/1.1\r\nHost: evil\r\n\r\n")
            .await
            .unwrap();
        let resp = response_prefix(&mut client).await;
        assert!(resp.starts_with("HTTP/1.1 403"), "{resp}");
    }

    #[tokio::test]
    async fn cidr_only_ip_literal_target_allowed() {
        // Nothing answers at 127.0.0.1:1 here; a 502 (not 403) proves the
        // allowlist matched and the proxy tried to connect.
        let proxy_addr = serve(allow("127.0.0.0/8")).await;
        let mut client = TcpStream::connect(proxy_addr).await.unwrap();
        client
            .write_all(b"CONNECT 127.0.0.1:1 HTTP/1.1\r\n\r\n")
            .await
            .unwrap();
        assert!(response_prefix(&mut client).await.starts_with("HTTP/1.1 502"));
    }

    #[tokio::test]
    async fn non_connect_not_tunnelled() {
        let proxy_addr = serve(allow("127.0.0.1")).await;
        let mut client = TcpStream::connect(proxy_addr).await.unwrap();
        client.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        assert!(response_prefix(&mut client).await.starts_with("HTTP/1.1 405"));
    }

    #[tokio::test]
    async fn oversized_head_rejected() {
        let proxy_addr = serve(allow("127.0.0.1")).await;
        let mut client = TcpStream::connect(proxy_addr).await.unwrap();
        let junk = vec![b'x'; HEAD_LIMIT + 1];
        client.write_all(&junk).await.unwrap();
        client.flush().await.unwrap();
        let mut sink = Vec::new();
        let _ = tokio::time::timeout(HEAD_TIMEOUT, stream_to_end(&mut client, &mut sink)).await;
        // No 200: either an error close or nothing — never a tunnel.
        assert!(!sink.ends_with(b"200"));
    }

    async fn stream_to_end<S: AsyncRead + Unpin>(
        stream: &mut S,
        out: &mut Vec<u8>,
    ) -> io::Result<usize> {
        AsyncReadExt::read_to_end(stream, out).await
    }
}
