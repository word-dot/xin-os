# rCore-Tutorial-v3 快速开始指南

## 自动下载测试镜像并运行

本项目已经集成了自动下载测试镜像的功能，无需手动下载和配置。

### 快速运行 RISC-V 测试

```bash
# 构建内核并自动下载镜像，然后运行
make run-rv
```

这个命令会：
1. 构建 RISC-V 内核 (`kernel-rv`)
2. 自动下载 `sdcard-rv.img` 测试镜像（如果不存在）
3. 启动 QEMU 运行测试

### 快速运行 LoongArch 测试

```bash
# 构建内核并自动下载镜像，然后运行
make run-la
```

### 仅下载镜像

如果你只想下载镜像而不运行：

```bash
# 下载 RISC-V 镜像
make download-img-rv

# 下载 LoongArch 镜像
make download-img-la

# 下载所有镜像
make disk-images
```

### 在 Docker 中运行

```bash
# 在 Docker 容器中运行 RISC-V 测试
make docker-run-rv
```

### 构建内核

```bash
# 构建所有架构的内核
make all

# 仅构建 RISC-V 内核
make kernel-rv

# 仅构建 LoongArch 内核
make kernel-la
```

### 清理

```bash
# 清理所有生成的文件（包括下载的镜像）
make clean
```

## 镜像来源

测试镜像自动从以下地址下载：
```
https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250615/
```

- RISC-V: `sdcard-rv.img.xz` (解压后约 1GB)
- LoongArch: `sdcard-la.img.xz` (解压后约 1GB)

## 注意事项

1. **首次运行会自动下载镜像**：第一次运行 `make run-rv` 或 `make run-la` 时，会自动下载对应的测试镜像，这可能需要几分钟时间（取决于网络速度）。

2. **镜像会被缓存**：下载的镜像会保存在项目根目录，后续运行不会重复下载。

3. **磁盘空间**：确保有足够的磁盘空间（每个镜像约 1GB）。

4. **测试不会卡住**：新的 Makefile 使用了经过验证的镜像挂载方法，避免了之前测试卡住的问题。

## 故障排除

### 下载失败

如果自动下载失败，可以手动下载：

```bash
# RISC-V
wget https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250615/sdcard-rv.img.xz
xz -d sdcard-rv.img.xz

# LoongArch
wget https://github.com/oscomp/testsuits-for-oskernel/releases/download/pre-20250615/sdcard-la.img.xz
xz -d sdcard-la.img.xz
```

### 镜像损坏

如果怀疑镜像损坏，删除后重新下载：

```bash
rm sdcard-rv.img
make download-img-rv
```

## 更多信息

查看所有可用的 make 目标：

```bash
make info