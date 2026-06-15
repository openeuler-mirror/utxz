use std::ffi::c_void;
use std::io;
use std::os::unix::io::RawFd;

/// 对应 `alarm(3)`，返回上一次剩余的秒数。
#[inline]
pub fn alarm(seconds: u32) -> u32 {
    // FFI 调用本身需要 unsafe；这里集中封装。
    unsafe { libc::alarm(seconds) }
}

#[inline]
pub fn geteuid() -> libc::uid_t {
    unsafe { libc::geteuid() }
}

#[inline]
pub fn pipe() -> io::Result<[RawFd; 2]> {
    let mut fds: [RawFd; 2] = [0, 0];
    let ret = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if ret == 0 {
        Ok(fds)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn close(fd: RawFd) -> io::Result<()> {
    let ret = unsafe { libc::close(fd) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn read(fd: RawFd, buf: &mut [u8]) -> io::Result<usize> {
    let ret = unsafe { libc::read(fd, buf.as_mut_ptr() as *mut c_void, buf.len()) };
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn write(fd: RawFd, buf: &[u8]) -> io::Result<usize> {
    let ret = unsafe { libc::write(fd, buf.as_ptr() as *const c_void, buf.len()) };
    if ret >= 0 {
        Ok(ret as usize)
    } else {
        Err(io::Error::last_os_error())
    }
}
