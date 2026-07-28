use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};

use crate::auth::oauth::{parse_callback, AuthCode, CallbackError};
use crate::error::Error;
use crate::prelude::Result;

/// The port Codex registers for its loopback redirect. We try it first because
/// OpenAI's client registration may pin the redirect URI to it.
pub const PREFERRED_PORT: u16 = 1455;

pub struct CallbackListener {
    listener: TcpListener,
}

/// Binds the loopback listener, preferring port 1455 and falling back to an
/// ephemeral port when it is already taken — otherwise a second `llm-stream`
/// login, or a running Codex, would hard-fail the sign-in.
pub fn bind() -> Result<CallbackListener> {
    let listener = TcpListener::bind(("127.0.0.1", PREFERRED_PORT))
        .or_else(|_| TcpListener::bind(("127.0.0.1", 0)))?;
    Ok(CallbackListener { listener })
}

impl CallbackListener {
    pub fn port(&self) -> Result<u16> {
        Ok(self.listener.local_addr()?.port())
    }

    /// The `redirect_uri` to send in the authorization request. It must match
    /// byte-for-byte in the later token exchange or the server rejects it.
    pub fn redirect_uri(&self) -> Result<String> {
        Ok(format!("http://localhost:{}/auth/callback", self.port()?))
    }

    /// Accepts exactly one connection, parses it, answers the browser, and
    /// returns. Consumes `self` — the listener is single-use by construction.
    pub fn await_code(self, expected_state: &str) -> Result<AuthCode> {
        let (mut socket, _) = self.listener.accept()?;

        let mut line = String::new();
        BufReader::new(socket.try_clone()?).read_line(&mut line)?;

        let outcome = query_of_request_line(line.trim_end())
            .ok_or(CallbackError::Malformed)
            .and_then(|query| parse_callback(query, expected_state));

        // Answer the browser either way. Leaving a tab spinning on a blank
        // page is a bad failure mode when the terminal is the thing to look at.
        match &outcome {
            Ok(_) => respond(
                &mut socket,
                200,
                "Signed in",
                "You can close this tab and return to your terminal.",
            )?,
            Err(e) => respond(&mut socket, 400, "Sign-in failed", &e.to_string())?,
        }

        outcome.map_err(|e| Error::Auth(e.to_string()))
    }
}

/// Extracts the query string from an HTTP request line such as
/// `GET /auth/callback?code=a&state=b HTTP/1.1`.
pub fn query_of_request_line(line: &str) -> Option<&str> {
    let mut parts = line.split_whitespace();
    let _method = parts.next()?;
    let target = parts.next()?;
    target.split_once('?').map(|(_, query)| query)
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn respond(socket: &mut TcpStream, status: u16, title: &str, message: &str) -> Result<()> {
    let body = format!(
        "<!doctype html><html><head><meta charset=\"utf-8\"><title>{t}</title></head>\
         <body style=\"font-family:system-ui;max-width:32rem;margin:4rem auto\">\
         <h1>{t}</h1><p>{m}</p></body></html>",
        t = html_escape(title),
        m = html_escape(message)
    );
    let reason = if status == 200 { "OK" } else { "Bad Request" };
    write!(
        socket,
        "HTTP/1.1 {status} {reason}\r\n\
         Content-Type: text/html; charset=utf-8\r\n\
         Content-Length: {len}\r\n\
         Connection: close\r\n\r\n{body}",
        len = body.len()
    )?;
    socket.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn extracts_the_query_from_a_request_line() {
        assert_eq!(
            query_of_request_line("GET /auth/callback?code=a&state=b HTTP/1.1"),
            Some("code=a&state=b")
        );
    }

    #[test]
    fn returns_none_without_a_query() {
        assert_eq!(query_of_request_line("GET /auth/callback HTTP/1.1"), None);
        assert_eq!(query_of_request_line("garbage"), None);
        assert_eq!(query_of_request_line(""), None);
    }

    #[test]
    fn escapes_html_in_the_rendered_message() {
        // The message can carry an `error=` value straight from the query
        // string. Rendering it raw into a page we serve would be an injection.
        assert_eq!(
            html_escape("<script>&\"x\""),
            "&lt;script&gt;&amp;&quot;x&quot;"
        );
    }

    #[test]
    fn await_code_accepts_a_local_callback() {
        let listener = bind().expect("bind");
        let port = listener.port().expect("port");
        std::thread::spawn(move || {
            let mut socket =
                std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
            write!(
                socket,
                "GET /auth/callback?code=xyz&state=st HTTP/1.1\r\nHost: localhost\r\n\r\n"
            )
            .expect("write");
        });
        assert_eq!(listener.await_code("st").expect("await").as_str(), "xyz");
    }

    #[test]
    fn await_code_rejects_a_forged_state() {
        let listener = bind().expect("bind");
        let port = listener.port().expect("port");
        std::thread::spawn(move || {
            let mut socket =
                std::net::TcpStream::connect(("127.0.0.1", port)).expect("connect");
            write!(
                socket,
                "GET /auth/callback?code=xyz&state=wrong HTTP/1.1\r\nHost: localhost\r\n\r\n"
            )
            .expect("write");
        });
        assert!(listener.await_code("st").is_err());
    }
}
