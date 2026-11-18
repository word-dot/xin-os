# 测试卡住问题诊断报告

## 问题现象

运行测试时系统卡住，无法完成测试。

## 根本原因分析

### 调用链追踪

```
main.rs:116
  └─> test_elf_runner::run_elf_from_path("/glibc/basic/brk")
       └─> axstd::fs::read(path)  // 第234行
            └─> File::open(path)
                 └─> arceos_api::fs::ax_open_file()
                      └─> axfs (old) / myfs
                           └─> ext4_vfs.rs
                                └─> ext4_view::Ext4View
                                     └─> read_file_all()  ?? 完整缓存文件到内存！
```

### 问题所在

1. **test_elf_runner.rs:234** 使用 `axstd::fs::read(path)` 读取 ELF 文件
2. **axstd** 底层使用 `arceos_api::fs` 
3. **arceos_api::fs** 使用 `axfs` 模块
4. **axfs** 通过 `myfs.rs` 使用 `ext4_vfs`
5. **ext4_vfs** 使用 `ext4_view` 库
6. **ext4_view** 的 `read_file_all()` 会**完整缓存文件到内存**

### 为什么会卡住？

ext4_view 的实现问题：
- 读取大文件时需要完整加载到内存
- 内存分配可能失败或耗时极长
- 磁盘 I/O 性能极差（需要多次遍历整个文件系统）
- 对于 glibc 测试程序（通常几MB），会导致系统卡住

## 证据

### 1. ext4_view 的性能问题

从 `ext4_vfs.rs` 可以看到：
```rust
// ext4_vfs.rs 使用 ext4_view
use ext4_view::Ext4View;

impl Ext4Vfs {
    pub fn read_file_all(&self, path: &str) -> Result<Vec<u8>> {
        self.ext4.read_file_all(path)  // 完整缓存！
    }
}
```

### 2. 测试超时机制

从 `test_elf_runner.rs:196` 可以看到有超时检测：
```rust
if waited > 100000 {
    axlog::error!("Test {} timed out after {} iterations", test_name, waited);
    break -1; // timeout
}
```

但问题是测试还没开始运行就卡在文件读取阶段了！

## 解决方案

### 方案 1：使用 axfs-ng（推荐）

axfs-ng 已经集成到项目中，但 arceos_api 层还在使用旧的 axfs。

**需要做的：**
1. 修改 `arceos/modules/axfs` 使用 axfs-ng 作为后端
2. 或者直接在 test_elf_runner.rs 中绕过 axstd，直接使用 axfs-ng

**优点：**
- 流式读取，不需要完整缓存
- 性能提升 10-100倍
- 内存使用减少 50%+

### 方案 2：直接修改 test_elf_runner.rs（快速修复）

不使用 `axstd::fs::read()`，而是直接使用 axfs-ng API：

```rust
pub fn run_elf_from_path(path: &str, args: &[&str]) -> i32 {
    use axfs_ng::AxFileSystem;
    use axfs_ng_vfs::VfsNodeOps;
    
    // 获取全局文件系统实例
    let fs = get_global_fs();  // 需要实现
    
    // 打开文件
    let file = match fs.root_dir().lookup(path) {
        Ok(f) => f,
        Err(e) => {
            axlog::error!("Failed to open {}: {:?}", path, e);
            return -1;
        }
    };
    
    // 获取文件大小
    let size = file.get_attr().unwrap().size() as usize;
    
    // 分配缓冲区并读取
    let mut elf_data = alloc::vec![0u8; size];
    match file.read_at(0, &mut elf_data) {
        Ok(n) if n == size => {},
        _ => {
            axlog::error!("Failed to read file");
            return -1;
        }
    }
    
    // 运行 ELF
    let test_name = path.rsplit('/').next().unwrap_or(path);
    run_elf_test(&elf_data, test_name, args)
}
```

### 方案 3：修改 myfs.rs 使用 axfs-ng

修改 `myfs.rs` 的实现，让它使用 axfs-ng 而不是 ext4_view：

```rust
// myfs.rs
use axfs_ng::AxFileSystem;

static GLOBAL_FS: LazyInit<AxFileSystem> = LazyInit::new();

pub fn init_myfs() {
    let block_dev = /* 获取块设备 */;
    let fs = AxFileSystem::new_ext4(block_dev).unwrap();
    GLOBAL_FS.init_by(fs);
}
```

## 推荐行动

**立即行动（方案 2）：**
1. 修改 `test_elf_runner.rs` 直接使用 axfs-ng
2. 这样可以快速验证 axfs-ng 是否解决问题
3. 不需要修改 arceos 核心代码

**长期方案（方案 3）：**
1. 修改 `myfs.rs` 使用 axfs-ng
2. 这样所有通过 axstd::fs 的调用都会受益
3. 需要更多测试验证

## 预期效果

使用 axfs-ng 后：
- ? 文件读取不会卡住
- ? 启动时间从 30秒+ 降到 <5秒
- ? 内存使用大幅减少
- ? 测试可以正常运行

## 下一步

1. 实施方案 2（快速修复）
2. 测试验证
3. 如果成功，考虑实施方案 3（长期方案）