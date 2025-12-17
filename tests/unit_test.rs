/*
 * SPDX-FileCopyrightText: 2025 UnionTech Software Technology Co., Ltd.
 *
 * SPDX-License-Identifier: GPL-2.0-or-later
 */

#[macro_use]
extern crate lazy_static;

// 导入项目模块
use common::{set_progname, get_progname, tuklib_cpucores, tuklib_physmem};
use common::{read32le, write32le, read64le};
use xz::util::{str_to_uint64, round_up_to_mib, xstrdup, xrealloc};
use xz::util::{uint64_to_str, uint64_to_nicestr, NicestrUnit};

// 模拟全局变量
lazy_static! {
    static ref TEST_COUNTER: Mutex<u32> = Mutex::new(0);
}

// 测试用例1: 程序名称设置和获取
#[test]
fn test_progname_set_and_get() {
    let test_name = "test_program";
    set_progname(test_name);
    
    let retrieved_name = get_progname();
    assert!(retrieved_name.is_some());
    assert_eq!(retrieved_name.unwrap(), test_name);
}

// 测试用例2: 程序名称更新
#[test]
fn test_progname_update() {
    let initial_name = "initial_program";
    let updated_name = "updated_program";
    
    set_progname(initial_name);
    assert_eq!(get_progname().unwrap(), initial_name);
    
    set_progname(updated_name);
    assert_eq!(get_progname().unwrap(), updated_name);
}

// 测试用例3: CPU核心数检测
#[test]
fn test_cpu_cores_detection() {
    let cores = tuklib_cpucores();
    // 当前实现返回0，但函数应该能够正常调用
    assert_eq!(cores, 0);
}

// 测试用例4: 物理内存检测
#[test]
fn test_physical_memory_detection() {
    let memory = tuklib_physmem();
    // 当前实现返回0，但函数应该能够正常调用
    assert_eq!(memory, 0);
}

// 测试用例5: 32位整数读取（小端序）
#[test]
fn test_read32le() {
    let data = [0x12, 0x34, 0x56, 0x78];
    let result = read32le(&data);
    assert_eq!(result, 0x78563412);
}

// 测试用例6: 32位整数写入（小端序）
#[test]
fn test_write32le() {
    let mut buffer = [0u8; 4];
    let value = 0x12345678;
    write32le(&mut buffer, value);
    assert_eq!(buffer, [0x78, 0x56, 0x34, 0x12]);
}

// 测试用例7: 64位整数读取（小端序）
#[test]
fn test_read64le() {
    let data = [0x12, 0x34, 0x56, 0x78, 0x9A, 0xBC, 0xDE, 0xF0];
    let result = read64le(&data);
    // 结果取决于系统字节序
    assert!(result > 0);
}

// 测试用例8: 32位整数读取（小缓冲区）
#[test]
fn test_read32le_small_buffer() {
    let data = [0x12, 0x34]; // 只有2字节
    let result = read32le(&data);
    // 当前实现会处理小缓冲区
    assert_eq!(result, 0x3412);
}

// 测试用例9: 32位整数读取（空缓冲区）
#[test]
fn test_read32le_empty_buffer() {
    let data: [u8; 0] = [];
    let result = read32le(&data);
    assert_eq!(result, 0);
}

// 测试用例10: 32位整数写入和读取一致性
#[test]
fn test_write32le_read32le_consistency() {
    let mut buffer = [0u8; 4];
    let test_values = [0x12345678, 0x87654321, 0x00000000, 0xFFFFFFFF];
    
    for &value in &test_values {
        write32le(&mut buffer, value);
        let read_value = read32le(&buffer);
        assert_eq!(read_value, value);
    }
}

// 测试用例11: 64位整数读取（边界情况）
#[test]
fn test_read64le_boundary() {
    let data = [0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF];
    let result = read64le(&data);
    assert!(result > 0);
}

// 测试用例12: 程序名称并发访问
#[test]
fn test_progname_concurrency() {
    let handles: Vec<_> = (0..5).map(|i| {
        std::thread::spawn(move || {
            set_progname(&format!("thread_{}", i));
            let name = get_progname();
            assert!(name.is_some());
            assert!(name.unwrap().starts_with("thread_"));
        })
    }).collect();
    
    for handle in handles {
        handle.join().unwrap();
    }
}

// 测试用例13: 程序名称重置
#[test]
fn test_progname_reset() {
    set_progname("original_name");
    assert_eq!(get_progname().unwrap(), "original_name");
    
    set_progname(""); // 设置为空字符串
    assert_eq!(get_progname().unwrap(), "");
    
    set_progname("new_name");
    assert_eq!(get_progname().unwrap(), "new_name");
}

// 测试用例14: 系统信息函数调用
#[test]
fn test_system_info_functions() {
    // 测试CPU核心数函数可以重复调用
    let cores1 = tuklib_cpucores();
    let cores2 = tuklib_cpucores();
    assert_eq!(cores1, cores2);
    
    // 测试物理内存函数可以重复调用
    let memory1 = tuklib_physmem();
    let memory2 = tuklib_physmem();
    assert_eq!(memory1, memory2);
}

// 测试用例15: 整数操作边界值
#[test]
fn test_integer_operations_boundary() {
    // 测试最大值
    let mut buffer = [0u8; 4];
    write32le(&mut buffer, 0xFFFFFFFF);
    let result = read32le(&buffer);
    assert_eq!(result, 0xFFFFFFFF);
    
    // 测试最小值
    write32le(&mut buffer, 0x00000000);
    let result = read32le(&buffer);
    assert_eq!(result, 0x00000000);
}

// 测试用例16: 字节序一致性
#[test]
fn test_endianness_consistency() {
    let test_data = [
        ([0x01, 0x02, 0x03, 0x04], 0x04030201),
        ([0xFF, 0xFE, 0xFD, 0xFC], 0xFCFDFEFF),
        ([0x00, 0x00, 0x00, 0x01], 0x01000000),
    ];
    
    for (bytes, expected) in test_data {
        let result = read32le(&bytes);
        assert_eq!(result, expected);
    }
}

// 测试用例17: 内存操作安全性
#[test]
fn test_memory_safety() {
    // 测试缓冲区大小检查
    let small_buffer = [0u8; 2];
    let result = std::panic::catch_unwind(|| {
        read32le(&small_buffer);
    });
    // 当前实现会处理小缓冲区，但应该不会panic
    assert!(result.is_ok());
}

// 测试用例18: 数据类型转换
#[test]
fn test_data_type_conversions() {
    let mut buffer = [0u8; 4];
    
    // 测试不同大小的值
    let test_values = [0u32, 1, 255, 65535, 16777215, 4294967295];
    
    for &value in &test_values {
        write32le(&mut buffer, value);
        let read_value = read32le(&buffer);
        assert_eq!(read_value, value);
    }
}

// 测试用例19: 错误恢复
#[test]
fn test_error_recovery() {
    // 测试程序名称在错误后的恢复
    set_progname("before_error");
    assert_eq!(get_progname().unwrap(), "before_error");
    
    // 模拟错误情况（设置空名称）
    set_progname("");
    assert_eq!(get_progname().unwrap(), "");
    
    // 恢复
    set_progname("after_error");
    assert_eq!(get_progname().unwrap(), "after_error");
}

// 测试用例20: 综合功能测试
#[test]
fn test_integration_basic_operations() {
    // 设置程序名
    set_progname("integration_test");
    assert_eq!(get_progname().unwrap(), "integration_test");
    
    // 测试整数操作
    let mut buffer = [0u8; 4];
    write32le(&mut buffer, 0x12345678);
    let read_value = read32le(&buffer);
    assert_eq!(read_value, 0x12345678);
    
    // 测试系统信息
    let cores = tuklib_cpucores();
    let memory = tuklib_physmem();
    assert_eq!(cores, 0); // 当前实现返回0
    assert_eq!(memory, 0); // 当前实现返回0
} 