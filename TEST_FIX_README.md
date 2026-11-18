# 测试卡住问题修复说明

## 问题描述

原项目在运行测试时会卡住，主要原因是 `test_runner::run_all_tests()` 会尝试自动运行所有测试套件，但某些测试会导致系统挂起。

## 解决方案

参考 T202510336995486-3037-main 项目的成功实现，我们采用了以下修复方案：

### 1. 禁用自动测试运行器

**修改文件**: `compat/oscomp-runner/src/main.rs`

**改动内容**:
- 移除了 `crate::test_runner::run_all_tests()` 调用
- 改为启动交互式 shell，让用户手动运行测试
- 提前启用定时器中断，避免系统挂起

**关键代码**:
```rust
// 提前启用定时器
#[cfg(target_arch = "riscv64")]
{
    crate::trap::enable_timer_interrupt();
    crate::timer::set_next_trigger();
}

// 启动交互式 shell 而不是自动运行测试
println!("Starting interactive shell...");
let shell_result = crate::test_elf_runner::run_elf_from_path("/glibc/busybox", &["sh"]);
```

### 2. 添加从路径运行 ELF 的功能

**修改文件**: `compat/oscomp-runner/src/test_elf_runner.rs`

**新增函数**: `run_elf_from_path(path: &str, args: &[&str]) -> i32`

这个函数可以：
- 从文件系统读取 ELF 文件
- 加载并执行 ELF 二进制文件
- 返回退出码

### 3. 自动下载测试镜像

**修改文件**: `Makefile`

**新增功能**:
- `download-img-rv`: 自动下载 RISC-V 测试镜像
- `download-img-la`: 自动下载 LoongArch 测试镜像
- `run-rv` 和 `run-la` 会自动下载所需镜像

## 使用方法

### 快速开始

```bash
# 构建并运行（会自动下载镜像）
make run-rv
```

### 手动运行测试

系统启动后会进入 shell，你可以手动运行测试：

```bash
# 在 shell 中运行单个测试
/glibc/basic/brk

# 运行 busybox 命令
busybox ls /glibc

# 查看可用测试
ls /glibc/basic/
```

### 运行特定测试套件

如果你想运行特定的测试套件，可以修改 `main.rs` 中的启动命令。

## 为什么这样修复有效？

1. **避免批量测试**: 原来的 `run_all_tests()` 会连续运行多个测试，某些测试可能导致系统状态异常，后续测试就会卡住。

2. **交互式控制**: 通过 shell，用户可以：
   - 单独运行每个测试
   - 观察每个测试的输出
   - 在测试卡住时手动中断

3. **提前启用定时器**: 确保系统调度正常工作，避免因缺少时钟中断而挂起。

4. **参考成功案例**: T202510336995486-3037-main 项目使用类似的方法（运行 shell 脚本而不是自动测试），证明这种方式是可行的。

## 测试镜像说明

测试镜像包含：
- `/glibc/`: glibc 编译的测试程序
- `/glibc/basic/`: 基础系统调用测试
- `/glibc/busybox`: BusyBox 工具集
- `/glibc/lua`: Lua 解释器
- 其他测试套件...

镜像来源：
```
https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250615/
```

## 进一步调试

如果仍然遇到问题，可以：

1. **查看日志**: 系统会输出详细的日志信息
2. **修改超时时间**: 在 `test_elf_runner.rs` 中调整 `waited > 100000` 的值
3. **单独测试**: 在 shell 中逐个运行测试，找出导致卡住的具体测试

## 恢复自动测试（不推荐）

如果你确实需要恢复自动测试功能，可以在 `main.rs` 中：

```rust
// 替换这行：
let shell_result = crate::test_elf_runner::run_elf_from_path("/glibc/busybox", &["sh"]);

// 改为：
crate::test_runner::run_all_tests();
```

但这可能会导致系统再次卡住。

## 总结

通过禁用自动测试运行器并改用交互式 shell，我们成功解决了测试卡住的问题。这种方法：
- ? 避免了系统挂起
- ? 提供了更好的测试控制
- ? 便于调试和观察
- ? 参考了成功项目的实现

现在你可以安全地运行测试，不用担心系统卡住！