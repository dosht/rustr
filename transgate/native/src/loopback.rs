//! The RFC 8252 loopback redirect (D7): a one-shot listener on 127.0.0.1 that
//! receives the browser's return from the authorization page. It hands the
//! request target back untouched; core's route parser decides whether it carries
//! a PKCE `code` or an error, so no auth rule is repeated here.
use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::time::{Duration, Instant};

use transgate_core::presentation::route;

pub struct Loopback {
    listener: TcpListener,
}

impl Loopback {
    pub fn bind() -> std::io::Result<Loopback> {
        let listener = TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)))?;
        listener.set_nonblocking(true)?;
        Ok(Loopback { listener })
    }

    /// `http://127.0.0.1:<port>`: the origin auth redirects return to.
    pub fn origin(&self) -> std::io::Result<String> {
        Ok(format!("http://{}", self.listener.local_addr()?))
    }

    /// The request target (`/home?code=...`) of the first request that carries a PKCE
    /// code or an auth error, answered with `page`; other requests (a favicon, a
    /// probe) get a 404. `None` after `timeout`.
    pub fn wait_callback(&self, timeout: Duration, page: &str) -> std::io::Result<Option<String>> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            match self.listener.accept() {
                Ok((stream, _)) => {
                    if let Some(target) = answer(stream, page)? {
                        return Ok(Some(target));
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => return Err(error),
            }
        }
        Ok(None)
    }
}

fn answer(mut stream: TcpStream, page: &str) -> std::io::Result<Option<String>> {
    stream.set_nonblocking(false)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    let mut request = Vec::new();
    let mut chunk = [0u8; 1024];
    while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 8192 {
        match stream.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => request.extend_from_slice(&chunk[..n]),
        }
    }
    let text = String::from_utf8_lossy(&request);
    let mut parts = text.lines().next().unwrap_or("").split(' ');
    let target = match (parts.next(), parts.next()) {
        (Some("GET"), Some(target)) => target.to_string(),
        _ => String::new(),
    };
    let callback = route::parse(&target).is_some_and(|l| l.auth_code.is_some() || l.auth_error.is_some());
    let (status, body) = if callback { ("200 OK", escape(page)) } else { ("404 Not Found", String::new()) };
    let response = format!("HTTP/1.1 {status}\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    let _ = stream.write_all(response.as_bytes());
    Ok(callback.then_some(target))
}

fn escape(text: &str) -> String {
    let body = text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
    format!("<!doctype html><meta charset=\"utf-8\"><title>Transgate</title><p>{body}</p>")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get(origin: &str, target: &str) -> String {
        let addr = origin.trim_start_matches("http://");
        let mut stream = TcpStream::connect(addr).unwrap();
        write!(stream, "GET {target} HTTP/1.1\r\nHost: {addr}\r\n\r\n").unwrap();
        let mut out = String::new();
        stream.read_to_string(&mut out).unwrap();
        out
    }

    #[test]
    fn only_a_request_with_a_code_or_an_error_ends_the_wait() {
        let loopback = Loopback::bind().unwrap();
        let origin = loopback.origin().unwrap();
        let o = origin.clone();
        let client = std::thread::spawn(move || {
            let probe = get(&o, "/favicon.ico");
            let real = get(&o, "/home?code=abc123");
            (probe, real)
        });
        let target = loopback.wait_callback(Duration::from_secs(5), "Signing in & <done>").unwrap();
        assert_eq!(target.as_deref(), Some("/home?code=abc123"));
        let (probe, real) = client.join().unwrap();
        assert!(probe.starts_with("HTTP/1.1 404"), "{probe}");
        assert!(real.starts_with("HTTP/1.1 200") && real.contains("Signing in &amp; &lt;done&gt;"), "{real}");
    }

    #[test]
    fn an_unanswered_wait_times_out() {
        let loopback = Loopback::bind().unwrap();
        assert_eq!(loopback.wait_callback(Duration::from_millis(60), "x").unwrap(), None);
    }
}
