use std::ffi::CStr;
use std::io;
use std::os::unix::io::RawFd;

#[inline]
pub fn fcntl_getfl(fd: RawFd) -> io::Result<i32> {
    let ret = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

#[inline]
pub fn fcntl_setfl(fd: RawFd, flags: i32) -> io::Result<()> {
    let ret = unsafe { libc::fcntl(fd, libc::F_SETFL, flags) };
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

#[inline]
pub fn open_with_mode(path: &CStr, flags: i32, mode: libc::mode_t) -> io::Result<RawFd> {
    let fd = unsafe { libc::open(path.as_ptr(), flags, mode) };
    if fd == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(fd)
    }
}
