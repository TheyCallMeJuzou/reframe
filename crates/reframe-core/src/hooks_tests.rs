//! The cases of the Electron version's `src/main/hooks/server.test.ts`, and what Node's HTTP server did there by
//! itself.

use std::io::{Read, Write};
use std::net::{Ipv4Addr, TcpStream};
use std::sync::mpsc::{Receiver, channel};
use std::time::Duration;

use super::*;

fn server() -> (HookServer, Receiver<HookMessage>) {
    let (sender, received) = channel();
    let sender = std::sync::Mutex::new(sender);
    let server = HookServer::start(move |message| {
        let _ = sender.lock().unwrap().send(message);
    })
    .unwrap();
    (server, received)
}

/// Sends the bytes and returns the status of the answer (0 if there is none).
fn send(server: &HookServer, request: &[u8]) -> u16 {
    let mut stream = TcpStream::connect((Ipv4Addr::LOCALHOST, server.port())).unwrap();
    stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    stream.write_all(request).unwrap();
    let mut answer = String::new();
    let _ = stream.read_to_string(&mut answer);
    // "HTTP/1.1 100 Continue" comes before the answer proper.
    let last = answer
        .rmatch_indices("HTTP/1.1 ")
        .next()
        .map(|(at, _)| &answer[at + 9..]);
    last.and_then(|rest| rest.get(..3))
        .and_then(|status| status.parse().ok())
        .unwrap_or(0)
}

fn post(server: &HookServer, path: &str, body: &str, token: Option<&str>) -> u16 {
    let token = token
        .map(|token| format!("X-Reframe-Token: {token}\r\n"))
        .unwrap_or_default();
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: 127.0.0.1\r\n{token}Content-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    send(server, request.as_bytes())
}

const WAIT: Duration = Duration::from_secs(5);

#[test]
fn emits_authenticated_hook_events_with_the_session_from_the_path() {
    let (server, received) = server();
    let token = server.token().to_owned();
    assert_eq!(token.len(), 48);
    assert_eq!(
        post(
            &server,
            "/hook/app%201/Stop",
            r#"{"session_id":"c-1","cwd":"/w"}"#,
            Some(&token)
        ),
        200
    );
    let message = received.recv_timeout(WAIT).unwrap();
    assert_eq!(message.session_id, "app 1");
    assert_eq!(message.event, HookEvent::Stop);
    assert_eq!(
        message.payload,
        HookPayload {
            session_id: Some("c-1".into()),
            cwd: Some("/w".into()),
            ..HookPayload::default()
        }
    );
}

#[test]
fn rejects_wrong_tokens_unknown_events_and_other_routes() {
    let (server, received) = server();
    let token = server.token().to_owned();
    assert_eq!(post(&server, "/hook/a/Stop", "{}", Some("wrong")), 403);
    assert_eq!(post(&server, "/hook/a/Stop", "{}", None), 403);
    assert_eq!(post(&server, "/hook/a/PreToolUse", "{}", Some(&token)), 404);
    assert_eq!(post(&server, "/other", "{}", Some(&token)), 404);
    let get = format!("GET /hook/a/Stop HTTP/1.1\r\nHost: x\r\nX-Reframe-Token: {token}\r\n\r\n");
    assert_eq!(send(&server, get.as_bytes()), 404);
    assert!(received.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn answers_a_broken_escape_in_the_path_with_400_and_keeps_going() {
    let (server, received) = server();
    let token = server.token().to_owned();
    assert_eq!(post(&server, "/hook/%E0%A4%A/Stop", "{}", Some(&token)), 400);
    assert_eq!(post(&server, "/hook/a/Stop", "{}", Some(&token)), 200);
    assert_eq!(received.recv_timeout(WAIT).unwrap().session_id, "a");
    assert!(received.recv_timeout(Duration::from_millis(200)).is_err());
}

#[test]
fn a_broken_or_empty_body_still_tells_the_event() {
    let (server, received) = server();
    let token = server.token().to_owned();
    assert_eq!(post(&server, "/hook/a/Notification", "not json", Some(&token)), 200);
    let message = received.recv_timeout(WAIT).unwrap();
    assert_eq!(
        (message.event, message.payload),
        (HookEvent::Notification, HookPayload::default())
    );
    assert_eq!(post(&server, "/hook/a/UserPromptSubmit", "", Some(&token)), 200);
    assert_eq!(received.recv_timeout(WAIT).unwrap().event, HookEvent::UserPromptSubmit);
    // Fields that are not text are left out, the others kept.
    let body = r#"{"session_id":7,"message":"hi","notification_type":"idle_prompt","more":[1]}"#;
    assert_eq!(post(&server, "/hook/a/Notification", body, Some(&token)), 200);
    let payload = received.recv_timeout(WAIT).unwrap().payload;
    assert_eq!(payload.session_id, None);
    assert_eq!(payload.message.as_deref(), Some("hi"));
    assert_eq!(payload.notification_type.as_deref(), Some("idle_prompt"));
}

#[test]
fn reads_a_chunked_body_and_answers_expect_continue() {
    let (server, received) = server();
    let token = server.token().to_owned();
    let request = format!(
        "POST /hook/a/Stop HTTP/1.1\r\nHost: x\r\nX-Reframe-Token: {token}\r\nExpect: 100-continue\r\n\
         Transfer-Encoding: chunked\r\n\r\n8\r\n{{\"cwd\":\"\r\n5\r\n/w\"}}\n\r\n0\r\n\r\n"
    );
    assert_eq!(send(&server, request.as_bytes()), 200);
    assert_eq!(received.recv_timeout(WAIT).unwrap().payload.cwd.as_deref(), Some("/w"));
}

#[test]
fn turns_away_what_is_not_http_and_bodies_that_are_too_long() {
    let (server, received) = server();
    let token = server.token().to_owned();
    assert_eq!(send(&server, b"hello\r\n\r\n"), 400);
    let long = format!("POST /hook/a/Stop HTTP/1.1\r\nX-Long: {}\r\n\r\n", "x".repeat(MAX_HEAD));
    assert_eq!(send(&server, long.as_bytes()), 431);
    let body = "x".repeat(MAX_BODY + 1);
    assert_eq!(post(&server, "/hook/a/Stop", &body, Some(&token)), 0);
    assert!(received.recv_timeout(Duration::from_millis(200)).is_err());
    // and it still answers
    assert_eq!(post(&server, "/hook/a/Stop", "{}", Some(&token)), 200);
}

#[test]
fn stops_listening() {
    let (mut server, _received) = server();
    let port = server.port();
    server.stop();
    server.stop();
    let address = std::net::SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    assert!(TcpStream::connect_timeout(&address, Duration::from_secs(1)).is_err());
}

#[test]
fn decodes_as_decode_uri_component() {
    assert_eq!(decode_uri_component("a%20b%C3%A4").as_deref(), Some("a bä"));
    assert_eq!(decode_uri_component("plain").as_deref(), Some("plain"));
    assert_eq!(decode_uri_component("%"), None);
    assert_eq!(decode_uri_component("%zz"), None);
    assert_eq!(decode_uri_component("%E0%A4"), None);
}
