use std::io;

/// 返回当前线程的 `errno`（如果无法获取则为 0）。
///
/// 注意：优先在 syscall 失败后立刻调用，避免被后续操作覆盖。
#[inline]
pub fn last_errno() -> i32 {
    io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

#[inline]
pub fn io_error_from_errno(errno: i32) -> io::Error {
    io::Error::from_raw_os_error(errno)
}
