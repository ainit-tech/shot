//! Fetching over HTTPS with WinHTTP and the proxy settings of Windows: the description of updates
//! and their installers, and the models with what runs them.

use std::ptr::{null, null_mut};
use std::time::Instant;

use windows_sys::Win32::Networking::WinHttp::{
    URL_COMPONENTS, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_FLAG_SECURE, WINHTTP_INTERNET_SCHEME_HTTPS,
    WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE, WinHttpCloseHandle, WinHttpConnect, WinHttpCrackUrl,
    WinHttpOpen, WinHttpOpenRequest, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
    WinHttpSetTimeouts,
};
use windows_sys::w;

use super::wide;
use crate::update::Version;

/// What stopped a fetch: no connection, or none that answered in time; a server that answered
/// otherwise than asked; or the receiver of the body, with its reason.
#[derive(Debug, PartialEq, Eq)]
pub enum Failure<E> {
    Offline,
    Server,
    Stopped(E),
}

/// What to fetch: the bytes of `url` from `from` to `to`, the last one included, or to its end;
/// no more than `limit` of them, and no later than `deadline`, if there is one.
pub struct Get<'a> {
    pub url: &'a str,
    pub from: u64,
    pub to: Option<u64>,
    pub limit: u64,
    pub deadline: Option<Instant>,
}

impl<'a> Get<'a> {
    /// The whole of `url`.
    pub fn whole(url: &'a str, limit: u64, deadline: Option<Instant>) -> Get<'a> {
        Get { url, from: 0, to: None, limit, deadline }
    }
}

/// A handle of WinHTTP, closed when dropped.
struct Handle(*mut core::ffi::c_void);

impl Drop for Handle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe { WinHttpCloseHandle(self.0) };
        }
    }
}

/// Fetches what `get` says, giving the body to `sink` as it arrives. A part of a file is asked
/// for with a range; a server that sends the whole file instead has the bytes before the part
/// skipped and those after it left.
pub fn get_into<E>(get: &Get, sink: &mut dyn FnMut(&[u8]) -> Result<(), E>) -> Result<(), Failure<E>> {
    let address = wide(get.url);
    let mut parts = URL_COMPONENTS {
        dwStructSize: size_of::<URL_COMPONENTS>() as u32,
        dwHostNameLength: u32::MAX,
        dwUrlPathLength: u32::MAX,
        dwExtraInfoLength: u32::MAX,
        ..Default::default()
    };
    if unsafe { WinHttpCrackUrl(address.as_ptr(), 0, 0, &mut parts) } == 0 || parts.nScheme != WINHTTP_INTERNET_SCHEME_HTTPS {
        return Err(Failure::Server);
    }
    let piece = |start: *const u16, len: u32| -> Vec<u16> {
        let mut text = unsafe { std::slice::from_raw_parts(start, len as usize) }.to_vec();
        text.push(0);
        text
    };
    let host = piece(parts.lpszHostName, parts.dwHostNameLength);
    // The query, if any, follows the path in the same text.
    let path = piece(parts.lpszUrlPath, parts.dwUrlPathLength + parts.dwExtraInfoLength);
    let agent = wide(&format!("ainitShot/{}", Version::current()));
    let session = Handle(unsafe { WinHttpOpen(agent.as_ptr(), WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, null(), null(), 0) });
    if session.0.is_null() {
        return Err(Failure::Offline);
    }
    unsafe { WinHttpSetTimeouts(session.0, 10_000, 10_000, 15_000, 20_000) };
    let connection = Handle(unsafe { WinHttpConnect(session.0, host.as_ptr(), parts.nPort, 0) });
    if connection.0.is_null() {
        return Err(Failure::Offline);
    }
    let request = Handle(unsafe {
        WinHttpOpenRequest(connection.0, w!("GET"), path.as_ptr(), null(), null(), null(), WINHTTP_FLAG_SECURE)
    });
    let ranged = get.from > 0 || get.to.is_some();
    let range = match get.to {
        Some(to) => format!("Range: bytes={}-{to}", get.from),
        None => format!("Range: bytes={}-", get.from),
    };
    let headers = wide(&range);
    let (headers, length) = if ranged { (headers.as_ptr(), u32::MAX) } else { (null(), 0) };
    if request.0.is_null()
        || unsafe { WinHttpSendRequest(request.0, headers, length, null(), 0, 0, 0) } == 0
        || unsafe { WinHttpReceiveResponse(request.0, null_mut()) } == 0
    {
        return Err(Failure::Offline);
    }
    let (mut status, mut size) = (0u32, size_of::<u32>() as u32);
    let queried = unsafe {
        WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            (&mut status as *mut u32).cast(),
            &mut size,
            null_mut(),
        )
    };
    if queried == 0 {
        return Err(Failure::Server);
    }
    // The bytes of the body to skip before those asked for, none when the server sends just them,
    // and how many of them are left to come.
    let (mut skip, mut left) = match status {
        206 if ranged => (0, get.to.map(|to| to + 1 - get.from)),
        200 => (get.from, get.to.map(|to| to + 1 - get.from)),
        _ => return Err(Failure::Server),
    };
    let mut buffer = vec![0u8; 64 * 1024];
    let mut total = 0u64;
    loop {
        if get.deadline.is_some_and(|deadline| Instant::now() > deadline) {
            return Err(Failure::Offline);
        }
        let mut read = 0u32;
        if unsafe { WinHttpReadData(request.0, buffer.as_mut_ptr().cast(), buffer.len() as u32, &mut read) } == 0 {
            return Err(Failure::Offline);
        }
        if read == 0 {
            return if left.is_none_or(|left| left == 0) { Ok(()) } else { Err(Failure::Offline) };
        }
        let mut bytes = &buffer[..read as usize];
        let skipped = skip.min(bytes.len() as u64);
        bytes = &bytes[skipped as usize..];
        skip -= skipped;
        if let Some(left) = &mut left {
            let kept = (*left).min(bytes.len() as u64);
            bytes = &bytes[..kept as usize];
            *left -= kept;
        }
        total += bytes.len() as u64;
        if total > get.limit {
            return Err(Failure::Server);
        }
        if !bytes.is_empty() {
            sink(bytes).map_err(Failure::Stopped)?;
        }
        if left == Some(0) {
            return Ok(());
        }
    }
}

/// What `get` asks for, in memory.
#[cfg(test)]
fn get(get: &Get) -> Result<Vec<u8>, Failure<()>> {
    let mut body = Vec::new();
    get_into(get, &mut |bytes| {
        body.extend_from_slice(bytes);
        Ok(())
    })?;
    Ok(body)
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;

    /// Fetches a page over HTTPS, whole and a part of it. It goes online, so it runs only when
    /// asked: `cargo test -p ainitshot fetches_over_https -- --ignored`.
    #[test]
    #[ignore]
    fn fetches_over_https_within_its_limits() {
        let deadline = Some(Instant::now() + Duration::from_secs(30));
        let url = "https://www.google.com/robots.txt";
        let page = get(&Get::whole(url, 1024 * 1024, deadline)).unwrap();
        assert!(String::from_utf8_lossy(&page).contains("User-agent"));
        let part = get(&Get { url, from: 10, to: Some(19), limit: 1024, deadline }).unwrap();
        assert_eq!(part, page[10..20]);
        assert_eq!(get(&Get::whole(url, 10, deadline)), Err(Failure::Server));
        assert_eq!(get(&Get::whole("http://www.google.com/robots.txt", 1024, deadline)), Err(Failure::Server));
        assert_eq!(get(&Get::whole("https://shot.invalid/latest.json", 1024, deadline)), Err(Failure::Offline));
    }
}
