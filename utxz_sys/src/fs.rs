use std::ffi::CStr;
use std::io;
use std::mem::MaybeUninit;
use std::os::unix::io::RawFd;

#[inline]
pub fn stat(path: &CStr) -> io::Result<libc::stat> {
    let mut st = MaybeUninit::<libc::stat>::zeroed();
    let ret = unsafe { libc::stat(path.as_ptr(), st.as_mut_ptr()) };
    if ret == 0 {
        Ok(unsafe { st.assume_init() })
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn lstat(path: &CStr, st: &mut libc::stat) -> io::Result<()> {
    let ret = unsafe { libc::lstat(path.as_ptr(), st as *mut libc::stat) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn fstat(fd: RawFd, st: &mut libc::stat) -> io::Result<()> {
    let ret = unsafe { libc::fstat(fd, st as *mut libc::stat) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn unlink(path: &CStr) -> io::Result<()> {
    let ret = unsafe { libc::unlink(path.as_ptr()) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn lseek(fd: RawFd, offset: libc::off_t, whence: i32) -> io::Result<libc::off_t> {
    let ret = unsafe { libc::lseek(fd, offset, whence) };
    if ret == -1 {
        Err(io::Error::last_os_error())
    } else {
        Ok(ret)
    }
}

#[inline]
pub fn fchmod(fd: RawFd, mode: libc::mode_t) -> io::Result<()> {
    let ret = unsafe { libc::fchmod(fd, mode) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn fchown(fd: RawFd, uid: libc::uid_t, gid: libc::gid_t) -> io::Result<()> {
    let ret = unsafe { libc::fchown(fd, uid, gid) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn futimens(fd: RawFd, times: &[libc::timespec; 2]) -> io::Result<()> {
    let ret = unsafe { libc::futimens(fd, times.as_ptr()) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn posix_fadvise(
    fd: RawFd,
    offset: libc::off_t,
    len: libc::off_t,
    advice: i32,
) -> io::Result<()> {
    let ret = unsafe { libc::posix_fadvise(fd, offset, len, advice) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(ret))
    }
}

#[inline]
pub fn zeroed_stat() -> libc::stat {
    // 这里集中承载 “C struct 置零初始化” 的 unsafe。
    unsafe { std::mem::zeroed() }
}
