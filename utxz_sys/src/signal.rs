use std::io;
use std::mem::MaybeUninit;
use std::ptr;

#[inline]
pub fn sigset_empty() -> libc::sigset_t {
    let mut set = MaybeUninit::<libc::sigset_t>::zeroed();
    // sigemptyset 会把 set 初始化为“空集合”，比纯 zeroed 更语义化。
    let ret = unsafe { libc::sigemptyset(set.as_mut_ptr()) };
    if ret != 0 {
        // 这类失败极少见；直接 panic 便于定位环境问题。
        panic!("sigemptyset failed: {}", io::Error::last_os_error());
    }
    unsafe { set.assume_init() }
}

#[inline]
pub fn sigaddset(set: &mut libc::sigset_t, sig: i32) -> io::Result<()> {
    let ret = unsafe { libc::sigaddset(set as *mut libc::sigset_t, sig) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn sigfillset(set: &mut libc::sigset_t) -> io::Result<()> {
    let ret = unsafe { libc::sigfillset(set as *mut libc::sigset_t) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn sigaction_get(sig: i32) -> io::Result<libc::sigaction> {
    let mut old = MaybeUninit::<libc::sigaction>::zeroed();
    let ret = unsafe { libc::sigaction(sig, ptr::null(), old.as_mut_ptr()) };
    if ret == 0 {
        Ok(unsafe { old.assume_init() })
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn sigaction_set(sig: i32, new: &libc::sigaction) -> io::Result<()> {
    let ret = unsafe { libc::sigaction(sig, new as *const libc::sigaction, ptr::null_mut()) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::last_os_error())
    }
}

#[inline]
pub fn raise(sig: i32) -> io::Result<()> {
    let ret = unsafe { libc::raise(sig) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(ret))
    }
}

/// 线程级信号掩码操作。
///
/// 在多线程程序里优先使用 `pthread_sigmask`（而非 `sigprocmask`），
/// 这里沿用历史命名，提供与 C 侧 `sigprocmask` 类似的接口形态。
#[inline]
pub fn sigprocmask(
    how: i32,
    set: Option<&libc::sigset_t>,
    oldset: Option<&mut libc::sigset_t>,
) -> io::Result<()> {
    let set_ptr = set.map_or(ptr::null(), |s| s as *const libc::sigset_t);
    let old_ptr = oldset.map_or(ptr::null_mut(), |s| s as *mut libc::sigset_t);

    // pthread_sigmask 返回错误码（而不是 -1 并设置 errno）。
    let ret = unsafe { libc::pthread_sigmask(how, set_ptr, old_ptr) };
    if ret == 0 {
        Ok(())
    } else {
        Err(io::Error::from_raw_os_error(ret))
    }
}

#[inline]
pub fn zeroed_sigaction() -> libc::sigaction {
    unsafe { std::mem::zeroed() }
}
