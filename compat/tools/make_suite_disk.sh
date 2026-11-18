#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
TESTSUITS_DIR="$ROOT/testsuits-for-oskernel"
ARCH="${1:-riscv64}"

case "$ARCH" in
    riscv64)
        build_target="build-rv"
        suite_root_glibc="$TESTSUITS_DIR/sdcard/riscv/glibc"
        suite_root_musl="$TESTSUITS_DIR/sdcard/riscv/musl"
        image_path="$ROOT/sdcard-rv.img"
        ;;
    loongarch64)
        build_target="build-la"
        suite_root_glibc="$TESTSUITS_DIR/sdcard/loongarch/glibc"
        suite_root_musl="$TESTSUITS_DIR/sdcard/loongarch/musl"
        image_path="$ROOT/sdcard-la.img"
        ;;
    *)
        echo "unsupported arch: $ARCH" >&2
        exit 1
        ;;
esac

# Ensure at least the glibc suite root exists with basic test content
if [[ ! -d "$suite_root_glibc" || ! -f "$suite_root_glibc/basic_testcode.sh" ]]; then
    echo "building local suite artifacts: $build_target"
    make -C "$TESTSUITS_DIR" "$build_target"
fi

# Verify that the glibc suite root now has content
if [[ ! -d "$suite_root_glibc" ]]; then
    echo "error: glibc suite root directory missing after build: $suite_root_glibc" >&2
    exit 1
fi

# Restore ELF binaries from .elfbin backups created by run-suite.sh wrap_elf_binaries.
# The wrapper scripts use qemu user-mode emulation which only works on the host,
# not inside QEMU system mode. For system mode we need the raw ELF binaries.
restore_elf_binaries() {
    local suite_root="$1"
    local count=0
    while IFS= read -r -d "" elfbin; do
        local orig="${elfbin%.elfbin}"
        if [[ -f "$orig" ]]; then
            # Replace the wrapper script with the real ELF binary
            cp "$elfbin" "$orig"
            chmod +x "$orig"
            count=$((count + 1))
        fi
    done < <(find "$suite_root" -name "*.elfbin" -print0)
    if [[ $count -gt 0 ]]; then
        echo "restored $count ELF binaries from .elfbin backups in $suite_root"
    fi
}

restore_elf_binaries "$suite_root_glibc"
if [[ -d "$suite_root_musl" ]]; then
    restore_elf_binaries "$suite_root_musl"
fi

workdir="$(mktemp -d)"
cleanup() {
    if mountpoint -q "$workdir/mnt" 2>/dev/null; then
        sudo umount "$workdir/mnt"
    fi
    rm -rf "$workdir"
}
trap cleanup EXIT

mkdir -p "$workdir/mnt"
rm -f "$image_path"
truncate -s 1024M "$image_path"
mkfs.ext4 -F "$image_path" >/dev/null
sudo mount "$image_path" "$workdir/mnt"

# Copy glibc suite (always present)
sudo mkdir -p "$workdir/mnt/glibc"
sudo cp -rL "$suite_root_glibc"/. "$workdir/mnt/glibc"/

# Copy musl suite if available
if [[ -d "$suite_root_musl" ]]; then
    sudo mkdir -p "$workdir/mnt/musl"
    sudo cp -rL "$suite_root_musl"/. "$workdir/mnt/musl"/
fi

# Copy shared DSO files to top-level directories (matching upstream sdcard layout)
if [[ -d "$workdir/mnt/musl/lib" ]]; then
    for dso in dlopen_dso.so tls_get_new-dtv_dso.so; do
        if [[ -f "$workdir/mnt/musl/lib/$dso" ]]; then
            sudo cp "$workdir/mnt/musl/lib/$dso" "$workdir/mnt/musl/"
        fi
    done
fi
if [[ -d "$workdir/mnt/glibc/lib" ]]; then
    for dso in dlopen_dso.so tls_get_new-dtv_dso.so; do
        if [[ -f "$workdir/mnt/glibc/lib/$dso" ]]; then
            sudo cp "$workdir/mnt/glibc/lib/$dso" "$workdir/mnt/glibc/"
        fi
    done
fi

sudo umount "$workdir/mnt"

echo "generated $image_path"
