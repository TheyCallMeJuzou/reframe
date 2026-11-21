//! Plain GET requests for the updater, redirects followed (GitHub hands release files over to
//! another host). On Windows through WinHTTP, with the system's proxy settings and certificate
//! store; on Linux through ureq, with the proxy from the environment. Blocking: call it from a
//! thread of its own.

#[cfg(target_os = "linux")]
pub use self::linux::get;
#[cfg(windows)]
pub use self::win32::get;

/// What a request brought back.
pub struct Response {
    pub status: u32,
    /// Where it ended up after redirects.
    pub final_url: String,
    /// How many bytes went to the sink.
    pub length: u64,
}

#[cfg(windows)]
mod win32 {
    use std::io::Write;

    use reframe_core::updates::parse_url;
    use windows::Win32::Networking::WinHttp::*;
    use windows::core::{HSTRING, PCWSTR};

    use super::Response;

    /// A WinHTTP handle that is closed when it goes.
    struct Handle(*mut core::ffi::c_void);

    impl Drop for Handle {
        fn drop(&mut self) {
            if !self.0.is_null() {
                // SAFETY: the handle came from WinHTTP and is closed once.
                unsafe {
                    let _ = WinHttpCloseHandle(self.0);
                }
            }
        }
    }

    fn opened(handle: *mut core::ffi::c_void, what: &str) -> Result<Handle, String> {
        if handle.is_null() {
            Err(format!("{what}: {}", windows::core::Error::from_thread()))
        } else {
            Ok(Handle(handle))
        }
    }

    /// Fetches `url` and writes the body to `sink`, at most `limit` bytes (more is an error).
    /// `accept` goes along as the Accept header.
    pub fn get(url: &str, accept: Option<&str>, sink: &mut dyn Write, limit: u64) -> Result<Response, String> {
        let parsed = parse_url(url).ok_or_else(|| format!("not an address: {url}"))?;
        let agent = HSTRING::from(format!("reframe/{}", env!("CARGO_PKG_VERSION")));
        // SAFETY: WinHTTP is called with valid strings and buffers; every handle is closed by its
        // guard, the request first.
        unsafe {
            let session = opened(
                WinHttpOpen(
                    &agent,
                    WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
                    PCWSTR::null(),
                    PCWSTR::null(),
                    0,
                ),
                "opening WinHTTP",
            )?;
            WinHttpSetTimeouts(session.0, 30_000, 30_000, 30_000, 60_000).map_err(|error| error.to_string())?;
            let connection = opened(
                WinHttpConnect(session.0, &HSTRING::from(parsed.host.as_str()), parsed.port, 0),
                "connecting",
            )?;
            let flags = if parsed.secure {
                WINHTTP_FLAG_SECURE
            } else {
                WINHTTP_OPEN_REQUEST_FLAGS(0)
            };
            let request = opened(
                WinHttpOpenRequest(
                    connection.0,
                    &HSTRING::from("GET"),
                    &HSTRING::from(parsed.path.as_str()),
                    PCWSTR::null(),
                    PCWSTR::null(),
                    std::ptr::null(),
                    flags,
                ),
                "opening the request",
            )?;
            let headers: Option<Vec<u16>> =
                accept.map(|accept| format!("Accept: {accept}\r\n").encode_utf16().collect());
            WinHttpSendRequest(request.0, headers.as_deref(), None, 0, 0, 0)
                .map_err(|error| format!("sending the request: {error}"))?;
            WinHttpReceiveResponse(request.0, std::ptr::null_mut())
                .map_err(|error| format!("receiving the answer: {error}"))?;

            let mut status = 0u32;
            let mut size = size_of::<u32>() as u32;
            WinHttpQueryHeaders(
                request.0,
                WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
                PCWSTR::null(),
                Some((&raw mut status).cast()),
                &mut size,
                std::ptr::null_mut(),
            )
            .map_err(|error| format!("reading the status: {error}"))?;
            let final_url = final_url(&request).unwrap_or_else(|| url.to_owned());

            let mut length = 0u64;
            let mut buffer = vec![0u8; 64 * 1024];
            loop {
                let mut read = 0u32;
                WinHttpReadData(request.0, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut read)
                    .map_err(|error| format!("reading: {error}"))?;
                if read == 0 {
                    break;
                }
                length += u64::from(read);
                if length > limit {
                    return Err(format!("more than {limit} bytes"));
                }
                sink.write_all(&buffer[..read as usize])
                    .map_err(|error| format!("writing: {error}"))?;
            }
            Ok(Response {
                status,
                final_url,
                length,
            })
        }
    }

    /// The address a request ended up at.
    fn final_url(request: &Handle) -> Option<String> {
        let mut size = 0u32;
        // SAFETY: the first call asks for the size, the second fills a buffer that big.
        unsafe {
            let _ = WinHttpQueryOption(request.0, WINHTTP_OPTION_URL, None, &mut size);
            if size == 0 {
                return None;
            }
            let mut buffer = vec![0u16; size as usize / 2 + 1];
            WinHttpQueryOption(
                request.0,
                WINHTTP_OPTION_URL,
                Some(buffer.as_mut_ptr().cast()),
                &mut size,
            )
            .ok()?;
            let end = buffer.iter().position(|unit| *unit == 0).unwrap_or(buffer.len());
            Some(String::from_utf16_lossy(&buffer[..end]))
        }
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use std::io::{Read, Write};
    use std::time::Duration;

    use ureq::ResponseExt;

    use super::Response;

    /// Fetches `url` and writes the body to `sink`, at most `limit` bytes (more is an error).
    /// `accept` goes along as the Accept header.
    pub fn get(url: &str, accept: Option<&str>, sink: &mut dyn Write, limit: u64) -> Result<Response, String> {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(format!("reframe/{}", env!("CARGO_PKG_VERSION")))
            .timeout_connect(Some(Duration::from_secs(30)))
            .timeout_recv_response(Some(Duration::from_secs(30)))
            .timeout_recv_body(Some(Duration::from_secs(60)))
            // A status is an answer like any other; the caller looks at it.
            .http_status_as_error(false)
            .build()
            .into();
        let mut request = agent.get(url);
        if let Some(accept) = accept {
            request = request.header("Accept", accept);
        }
        let mut response = request
            .call()
            .map_err(|error| format!("sending the request: {error}"))?;
        let status = u32::from(response.status().as_u16());
        let final_url = response.get_uri().to_string();
        let mut reader = response.body_mut().with_config().limit(u64::MAX).reader();
        let mut length = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];
        loop {
            let read = reader.read(&mut buffer).map_err(|error| format!("reading: {error}"))?;
            if read == 0 {
                break;
            }
            length += read as u64;
            if length > limit {
                return Err(format!("more than {limit} bytes"));
            }
            sink.write_all(&buffer[..read])
                .map_err(|error| format!("writing: {error}"))?;
        }
        Ok(Response {
            status,
            final_url,
            length,
        })
    }
}

/// Fetches a small text (at most 1 MB).
pub fn get_text(url: &str, accept: Option<&str>) -> Result<(String, Response), String> {
    let mut body = Vec::new();
    let response = get(url, accept, &mut body, 1024 * 1024)?;
    Ok((String::from_utf8_lossy(&body).into_owned(), response))
}
