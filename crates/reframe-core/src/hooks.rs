//! Local HTTP endpoint for Claude Code's HTTP hooks: `POST /hook/<reframe session id>/<event>`.
//!
//! Ported from the Electron version's `src/main/hooks/server.ts`.
//!
//! The Electron version stands on Node's HTTP server; this one reads HTTP/1.1 itself from a
//! [`std::net::TcpListener`], as far as a hook needs it: the request line, the headers, and a body given by
//! `Content-Length` or sent chunked. One thread accepts connections, each connection is answered on a short-lived
//! thread of its own (so a stuck client cannot hold up the others), and every answer closes its connection.
//! What Node's parser itself refuses before the server's code sees a request (a request that is not HTTP, headers
//! beyond 16 KB) is refused here the way Node does: with a bare status and no body.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde::{Deserialize, Deserializer};
use serde_json::Value;
use uuid::Uuid;

use crate::claude::settings::{HOOK_EVENTS, HOOK_TOKEN_HEADER};

/// What Claude Code sends with a hook, as far as reframe reads it. Anything else in the body is ignored, and so
/// is one of these that is not text.
#[derive(Clone, Debug, Default, PartialEq, Eq, Deserialize)]
#[serde(default)]
pub struct HookPayload {
    #[serde(deserialize_with = "text_or_absent")]
    pub session_id: Option<String>,
    #[serde(deserialize_with = "text_or_absent")]
    pub cwd: Option<String>,
    #[serde(deserialize_with = "text_or_absent")]
    pub hook_event_name: Option<String>,
    #[serde(deserialize_with = "text_or_absent")]
    pub notification_type: Option<String>,
    #[serde(deserialize_with = "text_or_absent")]
    pub message: Option<String>,
}

impl HookPayload {
    /// The payload in a request's body; an empty one if the body is not a JSON object.
    fn from_body(body: &[u8]) -> Self {
        // As Node reads it: bytes that are not UTF-8 become replacement characters.
        match serde_json::from_str::<Value>(&String::from_utf8_lossy(body)) {
            Ok(parsed @ Value::Object(_)) => serde_json::from_value(parsed).unwrap_or_default(),
            // an empty or broken body still tells us the event happened
            _ => Self::default(),
        }
    }
}

fn text_or_absent<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<String>, D::Error> {
    Ok(match Value::deserialize(deserializer)? {
        Value::String(text) => Some(text),
        _ => None,
    })
}

/// The hook events reframe asks Claude Code for ([`HOOK_EVENTS`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum HookEvent {
    UserPromptSubmit,
    Stop,
    Notification,
}

impl HookEvent {
    pub const ALL: [HookEvent; 3] = [HookEvent::UserPromptSubmit, HookEvent::Stop, HookEvent::Notification];

    /// The event's name, as Claude Code has it.
    pub fn as_str(self) -> &'static str {
        match self {
            HookEvent::UserPromptSubmit => "UserPromptSubmit",
            HookEvent::Stop => "Stop",
            HookEvent::Notification => "Notification",
        }
    }

    fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|event| event.as_str() == name)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookMessage {
    pub session_id: String,
    pub event: HookEvent,
    pub payload: HookPayload,
}

const MAX_BODY: usize = 1024 * 1024;
/// How long the request line and the headers may be together (Node's limit; beyond it Node answers 431).
const MAX_HEAD: usize = 16 * 1024;
/// How long a client may take over one read or one write before its connection is dropped.
const IO_TIMEOUT: Duration = Duration::from_secs(10);

/// What the threads share.
struct Shared {
    token: String,
    on_hook: Box<dyn Fn(HookMessage) + Send + Sync>,
}

pub struct HookServer {
    token: String,
    port: u16,
    stopping: Arc<AtomicBool>,
    accepting: Option<JoinHandle<()>>,
}

impl HookServer {
    /// Listens on a free port of 127.0.0.1. `on_hook` is called for every hook, on the thread of its connection.
    pub fn start(on_hook: impl Fn(HookMessage) + Send + Sync + 'static) -> io::Result<HookServer> {
        let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))?;
        let port = listener.local_addr()?.port();
        let token = random_token();
        let shared = Arc::new(Shared {
            token: token.clone(),
            on_hook: Box::new(on_hook),
        });
        let stopping = Arc::new(AtomicBool::new(false));
        let accepting = thread::Builder::new().name("reframe-hooks".to_owned()).spawn({
            let stopping = Arc::clone(&stopping);
            move || accept(&listener, &shared, &stopping)
        })?;
        Ok(HookServer {
            token,
            port,
            stopping,
            accepting: Some(accepting),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn token(&self) -> &str {
        &self.token
    }

    /// Stops listening. Requests that are being answered are answered still.
    pub fn stop(&mut self) {
        let Some(accepting) = self.accepting.take() else {
            return;
        };
        self.stopping.store(true, Ordering::SeqCst);
        // The accepting thread waits for a connection, and notices with the next one that it is to stop.
        let address = SocketAddr::from((Ipv4Addr::LOCALHOST, self.port));
        let woken = (0..3).any(|_| TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_ok());
        // Not woken (which a listener on this machine does not do): it is left to end with its next connection.
        if woken {
            let _ = accepting.join();
        }
    }
}

impl Drop for HookServer {
    fn drop(&mut self) {
        self.stop();
    }
}

fn accept(listener: &TcpListener, shared: &Arc<Shared>, stopping: &AtomicBool) {
    for stream in listener.incoming() {
        if stopping.load(Ordering::SeqCst) {
            break;
        }
        match stream {
            Ok(stream) => {
                let shared = Arc::clone(shared);
                // Without a thread for it, the connection is dropped.
                let _ = thread::Builder::new()
                    .name("reframe-hook".to_owned())
                    .spawn(move || handle(&stream, &shared));
            }
            // Out of sockets, for one: not in a tight loop.
            Err(_) => thread::sleep(Duration::from_millis(50)),
        }
    }
}

/// 24 random bytes as 48 hex digits.
fn random_token() -> String {
    // There is no random number crate here, so the bytes are those of two random (version 4) UUIDs: each has 14
    // bytes that are random throughout (the 7th and the 9th carry the version and the variant, and are left out).
    [Uuid::new_v4(), Uuid::new_v4()]
        .iter()
        .flat_map(|uuid| {
            let random = uuid.as_bytes().iter().enumerate();
            random.filter(|(at, _)| *at != 6 && *at != 8).map(|(_, byte)| *byte)
        })
        .take(24)
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Why a request gets no answer of the server's own.
enum Refused {
    /// The connection is dropped without an answer.
    Dropped,
    /// Not HTTP as Node's parser takes it: a bare status, no body.
    Status(u16),
}

/// How the body of a request is sent.
#[derive(Clone, Copy)]
enum Framing {
    /// So many bytes (`Content-Length`; none without it).
    Length(u64),
    /// In chunks (`Transfer-Encoding: chunked`).
    Chunked,
}

struct Request {
    method: String,
    /// The request's target as sent (Node's `req.url`).
    url: String,
    /// Names in lower case; values of a name sent twice joined with ", ", as Node gives them.
    headers: Vec<(String, String)>,
    framing: Framing,
}

impl Request {
    fn header(&self, name: &str) -> Option<&str> {
        let name = name.to_ascii_lowercase();
        self.headers
            .iter()
            .find(|(key, _)| *key == name)
            .map(|(_, value)| value.as_str())
    }
}

fn handle(stream: &TcpStream, shared: &Shared) {
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let mut reader = BufReader::new(stream);
    let request = match read_head(&mut reader) {
        Ok(request) => request,
        Err(refused) => return refuse(stream, &refused),
    };
    // Node answers this by itself before the server's code sees the request.
    if request
        .header("expect")
        .is_some_and(|expect| expect.eq_ignore_ascii_case("100-continue"))
    {
        let mut out = stream;
        let _ = out.write_all(b"HTTP/1.1 100 Continue\r\n\r\n");
    }
    let (session_id, event) = match route(&request, &shared.token) {
        Ok(route) => route,
        Err(status) => {
            reply(stream, status);
            // The body is taken off the connection all the same: closing with it unread could reset the
            // connection before the client has read the answer.
            let _ = read_body(&mut reader, request.framing);
            return;
        }
    };
    let body = match read_body(&mut reader, request.framing) {
        Ok(body) => body,
        Err(refused) => return refuse(stream, &refused),
    };
    reply(stream, 200);
    (shared.on_hook)(HookMessage {
        session_id,
        event,
        payload: HookPayload::from_body(&body),
    });
}

/// Whose hook the request is and which, or the status it is turned away with.
fn route(request: &Request, token: &str) -> Result<(String, HookEvent), u16> {
    let matched = match_route(&request.url);
    let sent = request.header(HOOK_TOKEN_HEADER);
    let Some((session, event)) = matched.filter(|_| request.method == "POST") else {
        return Err(404);
    };
    if !sent.is_some_and(|sent| safe_equal(sent, token)) {
        return Err(403);
    }
    if !HOOK_EVENTS.contains(&event) {
        return Err(404);
    }
    let event = HookEvent::from_name(event).ok_or(404_u16)?;
    // A broken escape in the path is answered, not passed on.
    let session_id = decode_uri_component(session).ok_or(400_u16)?;
    Ok((session_id, event))
}

/// The session and the event of `/hook/<session>/<event>` (`^\/hook\/([^/]+)\/([A-Za-z]+)$`).
fn match_route(url: &str) -> Option<(&str, &str)> {
    let (session, event) = url.strip_prefix("/hook/")?.split_once('/')?;
    let matches = !session.is_empty() && !event.is_empty() && event.bytes().all(|byte| byte.is_ascii_alphabetic());
    matches.then_some((session, event))
}

/// As JavaScript's `decodeURIComponent`: percent escapes as the UTF-8 bytes of characters. `None` (there: an
/// exception) for an escape that is not two hex digits or for bytes that are not UTF-8.
pub(crate) fn decode_uri_component(text: &str) -> Option<String> {
    fn flush(escaped: &mut Vec<u8>, decoded: &mut String) -> Option<()> {
        decoded.push_str(std::str::from_utf8(escaped).ok()?);
        escaped.clear();
        Some(())
    }
    let mut decoded = String::with_capacity(text.len());
    // Escapes one after the other: a character is made of nothing else.
    let mut escaped: Vec<u8> = Vec::new();
    let mut chars = text.chars();
    while let Some(char) = chars.next() {
        if char == '%' {
            let high = chars.next()?.to_digit(16)?;
            let low = chars.next()?.to_digit(16)?;
            escaped.push(u8::try_from(high * 16 + low).ok()?);
        } else {
            flush(&mut escaped, &mut decoded)?;
            decoded.push(char);
        }
    }
    flush(&mut escaped, &mut decoded)?;
    Some(decoded)
}

/// Compares in constant time (for texts of the same length).
fn safe_equal(a: &str, b: &str) -> bool {
    a.len() == b.len()
        && a.bytes()
            .zip(b.bytes())
            .fold(0, |differing, (x, y)| differing | (x ^ y))
            == 0
}

fn reason(status: u16) -> &'static str {
    match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        431 => "Request Header Fields Too Large",
        _ => "",
    }
}

/// The server's answer: `{}` with a status.
fn reply(mut stream: &TcpStream, status: u16) {
    let answer = format!(
        "HTTP/1.1 {status} {}\r\ncontent-type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{{}}",
        reason(status)
    );
    let _ = stream.write_all(answer.as_bytes());
    let _ = stream.flush();
    let _ = stream.shutdown(Shutdown::Write);
}

/// What Node's HTTP parser does with a request it does not take: a bare status, or nothing at all.
fn refuse(mut stream: &TcpStream, refused: &Refused) {
    if let Refused::Status(status) = refused {
        let answer = format!("HTTP/1.1 {status} {}\r\nConnection: close\r\n\r\n", reason(*status));
        let _ = stream.write_all(answer.as_bytes());
        let _ = stream.flush();
        let _ = stream.shutdown(Shutdown::Write);
    }
}

/// One line without its end, of at most `budget` bytes (which it is taken off); bytes as characters (Latin-1), as
/// Node reads a request's head. `Dropped` if the connection ends first.
fn read_line(reader: &mut BufReader<&TcpStream>, budget: &mut usize, too_long: u16) -> Result<String, Refused> {
    let mut line = Vec::new();
    let limit = u64::try_from(*budget).unwrap_or(u64::MAX);
    let read = reader.by_ref().take(limit).read_until(b'\n', &mut line);
    let read = read.map_err(|_| Refused::Dropped)?;
    *budget -= read.min(*budget);
    if line.pop() != Some(b'\n') {
        return Err(if read == 0 || *budget > 0 {
            Refused::Dropped
        } else {
            Refused::Status(too_long)
        });
    }
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(line.into_iter().map(char::from).collect())
}

/// The request line and the headers.
fn read_head(reader: &mut BufReader<&TcpStream>) -> Result<Request, Refused> {
    let mut budget = MAX_HEAD;
    let mut line = read_line(reader, &mut budget, 431)?;
    // Empty lines before the request are passed over.
    while line.is_empty() {
        line = read_line(reader, &mut budget, 431)?;
    }
    let mut parts = line.split(' ');
    let (Some(method), Some(url), Some(version), None) = (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return Err(Refused::Status(400));
    };
    if method.is_empty() || url.is_empty() || !matches!(version, "HTTP/1.1" | "HTTP/1.0") {
        return Err(Refused::Status(400));
    }
    let (method, url) = (method.to_owned(), url.to_owned());

    let mut headers: Vec<(String, String)> = Vec::new();
    let mut length: Option<u64> = None;
    let mut chunked: Option<bool> = None;
    loop {
        let line = read_line(reader, &mut budget, 431)?;
        if line.is_empty() {
            break;
        }
        let Some((name, value)) = line.split_once(':') else {
            return Err(Refused::Status(400));
        };
        if name.is_empty() || name.contains([' ', '\t']) {
            return Err(Refused::Status(400));
        }
        let name = name.to_ascii_lowercase();
        let value = value.trim_matches([' ', '\t']);
        match name.as_str() {
            "content-length" => {
                // Digits and nothing else, and once only.
                let digits = !value.is_empty() && value.bytes().all(|byte| byte.is_ascii_digit());
                let Some(bytes) = value.parse::<u64>().ok().filter(|_| digits && length.is_none()) else {
                    return Err(Refused::Status(400));
                };
                length = Some(bytes);
            }
            "transfer-encoding" => {
                let last = value.rsplit(',').next().unwrap_or_default();
                chunked = Some(last.trim_matches([' ', '\t']).eq_ignore_ascii_case("chunked"));
            }
            _ => {}
        }
        match headers.iter_mut().find(|(key, _)| *key == name) {
            Some((_, values)) => {
                values.push_str(", ");
                values.push_str(value);
            }
            None => headers.push((name, value.to_owned())),
        }
    }
    let framing = match (chunked, length) {
        // A request's body in another encoding has no end, and a length next to an encoding is not allowed.
        (Some(false), _) | (Some(true), Some(_)) => return Err(Refused::Status(400)),
        (Some(true), None) => Framing::Chunked,
        (None, length) => Framing::Length(length.unwrap_or(0)),
    };
    Ok(Request {
        method,
        url,
        headers,
        framing,
    })
}

/// The request's body. `Dropped` (the Electron version destroys the request) once it is more than `MAX_BODY`, and
/// if the connection ends before the body does.
fn read_body(reader: &mut BufReader<&TcpStream>, framing: Framing) -> Result<Vec<u8>, Refused> {
    let mut body = Vec::new();
    match framing {
        Framing::Length(length) => read_bytes(reader, length, &mut body)?,
        Framing::Chunked => loop {
            let line = read_line(reader, &mut MAX_HEAD.clone(), 400)?;
            // The size in hex, and after a ";" what this server has no use for.
            let size = line.split(';').next().unwrap_or_default().trim_matches([' ', '\t']);
            let hex = !size.is_empty() && size.bytes().all(|byte| byte.is_ascii_hexdigit());
            let Some(size) = u64::from_str_radix(size, 16).ok().filter(|_| hex) else {
                return Err(Refused::Status(400));
            };
            if size == 0 {
                // Headers after the body, up to an empty line.
                let mut budget = MAX_HEAD;
                while !read_line(reader, &mut budget, 431)?.is_empty() {}
                break;
            }
            read_bytes(reader, size, &mut body)?;
            if !read_line(reader, &mut MAX_HEAD.clone(), 400)?.is_empty() {
                return Err(Refused::Status(400));
            }
        },
    }
    Ok(body)
}

/// Reads so many bytes onto the body.
fn read_bytes(reader: &mut BufReader<&TcpStream>, mut length: u64, body: &mut Vec<u8>) -> Result<(), Refused> {
    let mut piece = [0_u8; 16 * 1024];
    while length > 0 {
        let want = usize::try_from(length).map_or(piece.len(), |length| length.min(piece.len()));
        let read = match reader.read(&mut piece[..want]) {
            Ok(read) if read > 0 => read,
            _ => return Err(Refused::Dropped),
        };
        if body.len() + read > MAX_BODY {
            return Err(Refused::Dropped);
        }
        body.extend_from_slice(&piece[..read]);
        length -= u64::try_from(read).unwrap_or(length).min(length);
    }
    Ok(())
}

#[cfg(test)]
#[path = "hooks_tests.rs"]
mod tests;
