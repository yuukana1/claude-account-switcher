//! Synchronous WinHTTP, called only on the updater's worker thread.
use std::{
    ffi::c_void,
    io, ptr,
    time::{Duration, Instant},
};
use windows_sys::Win32::Networking::WinHttp::*;

struct Handle(*mut c_void);
impl Handle {
    fn new(raw: *mut c_void) -> io::Result<Self> {
        if raw.is_null() {
            Err(io::Error::last_os_error())
        } else {
            Ok(Self(raw))
        }
    }
}
impl Drop for Handle {
    fn drop(&mut self) {
        unsafe {
            WinHttpCloseHandle(self.0);
        }
    }
}
fn ok(result: i32) -> io::Result<()> {
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

pub(super) fn get(
    host: &str,
    path: &str,
    limit: usize,
    mut progress: impl FnMut(usize),
) -> io::Result<Vec<u8>> {
    let agent = wide(concat!(
        "Claude-Account-Switcher/",
        env!("CARGO_PKG_VERSION")
    ));
    // Handles are owned, nested and synchronous. WinHTTP uses the system's TLS
    // verifier and proxy settings; the default redirect policy forbids HTTPS → HTTP.
    unsafe {
        let session = Handle::new(WinHttpOpen(
            agent.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            ptr::null(),
            ptr::null(),
            0,
        ))?;
        ok(WinHttpSetTimeouts(session.0, 8000, 12000, 15000, 20000))?;
        let conn = Handle::new(WinHttpConnect(session.0, wide(host).as_ptr(), 443, 0))?;
        let request = Handle::new(WinHttpOpenRequest(
            conn.0,
            wide("GET").as_ptr(),
            wide(path).as_ptr(),
            ptr::null(),
            ptr::null(),
            ptr::null(),
            WINHTTP_FLAG_SECURE,
        ))?;
        let policy = WINHTTP_AUTOLOGON_SECURITY_LEVEL_HIGH;
        ok(WinHttpSetOption(
            request.0,
            WINHTTP_OPTION_AUTOLOGON_POLICY,
            (&policy as *const u32).cast(),
            4,
        ))?;
        ok(WinHttpSendRequest(
            request.0,
            ptr::null(),
            0,
            ptr::null(),
            0,
            0,
            0,
        ))?;
        ok(WinHttpReceiveResponse(request.0, ptr::null_mut()))?;
        let mut status = 0u32;
        let mut length = 4;
        ok(WinHttpQueryHeaders(
            request.0,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            ptr::null(),
            (&mut status as *mut u32).cast(),
            &mut length,
            ptr::null_mut(),
        ))?;
        if status != 200 {
            return Err(io::Error::other(format!("GitHub 返回 HTTP {status}")));
        }
        let start = Instant::now();
        let mut bytes = Vec::new();
        let mut buf = [0u8; 32 * 1024];
        loop {
            if start.elapsed() > Duration::from_secs(180) {
                return Err(io::Error::other("下载超时"));
            }
            let mut n = 0;
            ok(WinHttpReadData(
                request.0,
                buf.as_mut_ptr().cast(),
                buf.len() as u32,
                &mut n,
            ))?;
            if n == 0 {
                return Ok(bytes);
            }
            if bytes.len() + n as usize > limit {
                return Err(io::Error::other("响应超过预期大小"));
            }
            bytes.extend_from_slice(&buf[..n as usize]);
            progress(bytes.len());
        }
    }
}
