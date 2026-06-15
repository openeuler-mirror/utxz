use std::io;

#[inline]
pub fn poll(pfds: &mut [libc::pollfd], timeout_ms: i32) -> io::Result<i32> {
    let ret = unsafe { libc::poll(pfds.as_mut_ptr(), pfds.len() as libc::nfds_t, timeout_ms) };
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}
