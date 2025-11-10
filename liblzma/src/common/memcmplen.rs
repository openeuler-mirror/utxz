use common::{my_min, read64ne};

pub const LZMA_MEMCMPLEN_EXTRA: usize = 8;

pub fn lzma_memcmplen(buf1: &[u8], buf2: &[u8], mut len: u32, limit: u32) -> u32 {
    assert!(len <= limit);
    assert!(limit <= u32::MAX / 2);

    while len < limit {
        let buf1_tem = &buf1[len as usize..(len + 8) as usize]; // 获取 buf1 的切片
        let buf2_tem = &buf2[len as usize..(len + 8) as usize]; // 获取 buf2 的切片
        let a = read64ne(buf1_tem);
        let b = read64ne(buf2_tem);
        // let x = read64ne(buf1_tem) - read64ne(buf2_tem); // 传递切片
        let x = a.wrapping_sub(b);
        // println!("x = {}", x);
        if x != 0 {
            len += x.trailing_zeros() >> 3;
            // println!("888888 len = {}", len);
            return my_min(len, limit);
        }
        len += 8;
    }

    limit
}
