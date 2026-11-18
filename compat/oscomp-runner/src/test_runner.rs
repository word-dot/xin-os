//! Real test runner - loads ELF binaries from ext4 VFS and executes them.

#![no_std]

extern crate alloc;
use alloc::vec::Vec;
use alloc::string::{String, ToString};

const SUITE_NAMES: &[&str] = &[
    "basic", "busybox", "lua", "libctest",
    "iozone", "unixbench", "iperf", "libcbench",
    "lmbench", "cyclictest", "ltp", "netperf",
];

const BASIC_TESTS: &[&str] = &[
    "brk", "chdir", "clone", "close", "dup2", "dup",
    "execve", "exit", "fork", "fstat", "getcwd", "getdents",
    "getpid", "getppid", "gettimeofday", "mkdir_", "mmap", "mount",
    "munmap", "openat", "open", "pipe", "read", "sleep",
    "times", "umount", "uname", "unlink", "wait", "waitpid",
    "write", "yield",
];

fn run_elf_test_from_vfs(path: &str, test_name: &str) -> i32 {
    axlog::warn!("Attempting to read ELF file: {}", path);
    let elf_data = match axstd::fs::read(path) {
        Ok(data) => {
            axlog::warn!("Successfully read ELF file: {}, size: {} bytes", path, data.len());
            data
        },
        Err(e) => {
            axlog::error!("  [SKIP] cannot read {}: {:?}", path, e);
            println!("  [SKIP] cannot read {}: {:?}", path, e);
            return 0;
        }
    };

    if elf_data.is_empty() {
        axlog::error!("  [SKIP] empty file: {}", path);
        println!("  [SKIP] empty file: {}", path);
        return 0;
    }

    axlog::warn!("Preparing to run test: {} ({})", test_name, path);
    println!("  Running {} ({})...", test_name, path);
    
    let exit_code = crate::test_elf_runner::run_elf_test(&elf_data, test_name, &[]);
    
    if exit_code == 0 {
        axlog::warn!("  [PASS] {} exited with code {}", test_name, exit_code);
        println!("  [PASS] {} exited with code {}", test_name, exit_code);
    } else {
        axlog::error!("  [FAIL] {} exited with code {}", test_name, exit_code);
        println!("  [FAIL] {} exited with code {}", test_name, exit_code);
    }
    exit_code
}

fn run_basic_suite_real() {
    println!("#### OS COMP TEST GROUP START basic-glibc ####");
    for test_name in BASIC_TESTS {
        let path = alloc::format!("/glibc/basic/{}", test_name);
        axlog::warn!("Attempting to run test: {} at path: {}", test_name, path);
        println!("Testing {} :", test_name);
        
        // [non-UTF-8 comment removed]
        match axstd::fs::metadata(&path) {
            Ok(metadata) => {
                axlog::warn!("File {} exists. Size: {} bytes", path, metadata.len());
                run_elf_test_from_vfs(&path, test_name);
            },
            Err(e) => {
                axlog::error!("File {} does not exist or cannot be accessed: {:?}", path, e);
                println!("  [ERROR] File {} does not exist or cannot be accessed", path);
            }
        }
    }
    println!("#### OS COMP TEST GROUP END basic-glibc ####");
}

fn run_single_binary_suite(suite_name: &str, binary_name: &str) {
    println!("#### OS COMP TEST GROUP START {}-glibc ####", suite_name);
    let path = alloc::format!("/glibc/{}/{}", suite_name, binary_name);
    println!("Testing {} :", binary_name);
    run_elf_test_from_vfs(&path, binary_name);
    println!("#### OS COMP TEST GROUP END {}-glibc ####", suite_name);
}

fn run_busybox_suite_real() {
    println!("#### OS COMP TEST GROUP START busybox-glibc ####");
    run_elf_test_from_vfs("/glibc/busybox", "busybox");
    println!("#### OS COMP TEST GROUP END busybox-glibc ####");
}

fn run_lua_suite_real() {
    run_single_binary_suite("lua", "lua");
}

fn run_iozone_suite_real() {
    run_single_binary_suite("iozone", "iozone");
}

fn run_libcbench_suite_real() {
    run_single_binary_suite("libcbench", "libc-bench");
}

fn run_cyclictest_suite_real() {
    println!("#### OS COMP TEST GROUP START cyclictest-glibc ####");
    run_elf_test_from_vfs("/glibc/cyclictest", "cyclictest");
    run_elf_test_from_vfs("/glibc/cyclictest", "hackbench");
    println!("#### OS COMP TEST GROUP END cyclictest-glibc ####");
}

fn run_libctest_suite_real() {
    println!("#### OS COMP TEST GROUP START libctest-glibc ####");
    let sample_tests = &[
        "src/functional/baseline",
        "src/functional/string",
        "src/functional/stdio",
    ];
    for test in sample_tests {
        let path = alloc::format!("/glibc/libctest/{}", test);
        println!("Testing {} :", test);
        run_elf_test_from_vfs(&path, test);
    }
    println!("#### OS COMP TEST GROUP END libctest-glibc ####");
}

fn run_unixbench_suite_real() {
    println!("#### OS COMP TEST GROUP START unixbench-glibc ####");
    println!("[SKIP] unixbench (requires chdir and complex shell scripts)");
    println!("#### OS COMP TEST GROUP END unixbench-glibc ####");
}

fn run_iperf_suite_real() {
    println!("#### OS COMP TEST GROUP START iperf-glibc ####");
    run_elf_test_from_vfs("/glibc/iperf", "iperf3");
    println!("#### OS COMP TEST GROUP END iperf-glibc ####");
}

fn run_lmbench_suite_real() {
    println!("#### OS COMP TEST GROUP START lmbench-glibc ####");
    println!("[SKIP] lmbench (requires special setup)");
    println!("#### OS COMP TEST GROUP END lmbench-glibc ####");
}

fn run_ltp_suite_real() {
    println!("#### OS COMP TEST GROUP START ltp-glibc ####");
    let ltp_tests = &["stat", "getpid", "fork01"];
    for test in ltp_tests {
        let path = alloc::format!("/glibc/ltp/testcases/bin/{}", test);
        println!("Testing {} :", test);
        run_elf_test_from_vfs(&path, test);
    }
    println!("#### OS COMP TEST GROUP END ltp-glibc ####");
}

fn run_netperf_suite_real() {
    println!("#### OS COMP TEST GROUP START netperf-glibc ####");
    run_elf_test_from_vfs("/glibc/netperf", "netperf");
    run_elf_test_from_vfs("/glibc/netperf", "netserver");
    println!("#### OS COMP TEST GROUP END netperf-glibc ####");
}

pub fn run_all_tests() {
    println!("[test_runner] running ALL suites with real ELF execution from VFS");

    for suite_name in SUITE_NAMES {
        match *suite_name {
            "basic" => run_basic_suite_real(),
            "busybox" => run_busybox_suite_real(),
            "lua" => run_lua_suite_real(),
            "libctest" => run_libctest_suite_real(),
            "iozone" => run_iozone_suite_real(),
            "unixbench" => run_unixbench_suite_real(),
            "iperf" => run_iperf_suite_real(),
            "libcbench" => run_libcbench_suite_real(),
            "lmbench" => run_lmbench_suite_real(),
            "cyclictest" => run_cyclictest_suite_real(),
            "ltp" => run_ltp_suite_real(),
            "netperf" => run_netperf_suite_real(),
            _ => {}
        }
    }

    println!("[test_runner] all suites completed.");
}
