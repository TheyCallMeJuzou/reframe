//! A release feed as GitHub serves it, on a port of its own, standing in for reframe's releases:
//! `/latest` (the newest tag), `/download/<tag>/<latest.yml>` and the file it names. Remembers what
//! was asked for.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};

use base64::Engine;
use serde_json::json;
use sha2::{Digest, Sha512};

pub struct Feed {
    pub url: String,
    asked: Arc<Mutex<Vec<String>>>,
}

pub fn sha512_of(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(Sha512::digest(bytes))
}

impl Feed {
    /// A release `version` whose `yml` (latest.yml, latest-linux.yml) names `file`, with these
    /// `bytes` and that `sha512` (which a test may get wrong on purpose).
    pub fn start(version: &str, yml: &str, file: &str, bytes: &[u8], sha512: &str) -> Feed {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://127.0.0.1:{}", listener.local_addr().unwrap().port());
        let asked = Arc::new(Mutex::new(Vec::new()));
        let tag = format!("v{version}");
        let text = format!(
            "version: {version}\nfiles:\n  - url: {file}\n    sha512: {sha512}\n    size: {}\npath: {file}\nsha512: {sha512}\nreleaseDate: '2026-11-20T10:00:00.000Z'\n",
            bytes.len()
        );
        let routes: Vec<(String, Vec<u8>)> = vec![
            (
                "/latest".to_owned(),
                json!({ "tag_name": tag }).to_string().into_bytes(),
            ),
            (format!("/download/{tag}/{yml}"), text.into_bytes()),
            (format!("/download/{tag}/{file}"), bytes.to_vec()),
        ];
        let log = asked.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                if reader.read_line(&mut line).is_err() {
                    continue;
                }
                // The rest of the request.
                loop {
                    let mut header = String::new();
                    if reader.read_line(&mut header).unwrap_or(0) == 0 || header == "\r\n" {
                        break;
                    }
                }
                let path = line.split_whitespace().nth(1).unwrap_or_default().to_owned();
                log.lock().unwrap().push(path.clone());
                let mut stream = stream;
                match routes.iter().find(|(route, _)| *route == path) {
                    Some((_, body)) => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        let _ = stream.write_all(body);
                    }
                    None => {
                        let _ = write!(
                            stream,
                            "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                    }
                }
            }
        });
        Feed { url, asked }
    }

    pub fn asked(&self) -> Vec<String> {
        self.asked.lock().unwrap().clone()
    }
}
