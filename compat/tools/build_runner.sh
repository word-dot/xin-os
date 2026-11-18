#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
ARCEOS_DIR="$ROOT/arceos"
APP_DIR="$ROOT/compat/oscomp-runner"
ARCH="${1:-riscv64}"
TARGET="${2:-build}"
FEATURES="alloc,fs,myfs,paging,irq,multitask"
OUT_CONFIG="$ROOT/.axconfig-${ARCH}.toml"

find_cargo() {
    local candidate
    for candidate in "${CARGO:-}" cargo /usr/bin/cargo /usr/local/bin/cargo; do
        if [[ -n "$candidate" ]] && command -v "$candidate" >/dev/null 2>&1; then
            command -v "$candidate"
            return 0
        fi
        if [[ -n "$candidate" && -x "$candidate" ]]; then
            printf '%s\n' "$candidate"
            return 0
        fi
    done
    return 1
}

CARGO_BIN="$(find_cargo)" || {
    echo "cargo is required to build oscomp-runner" >&2
    exit 127
}

case "$ARCH" in
    riscv64)
        PLAT_CONFIG_FILE="$ROOT/platconfig/riscv64-qemu-virt.toml"
        ;;
    loongarch64)
        PLAT_CONFIG_FILE="$ROOT/platconfig/loongarch64-qemu-virt.toml"
        ;;
    *)
        echo "unsupported arch: $ARCH" >&2
        exit 1
        ;;
esac

if [[ ! -f "$PLAT_CONFIG_FILE" ]]; then
    echo "platform config file not found: $PLAT_CONFIG_FILE" >&2
    exit 1
fi

PLAT_CONFIG="$PLAT_CONFIG_FILE"

make -C "$ARCEOS_DIR" \
    A="$APP_DIR" \
    ARCH="$ARCH" \
    BUS=mmio \
    LOG=warn \
    OUT_CONFIG="$OUT_CONFIG" \
    PLAT_CONFIG="$PLAT_CONFIG" \
    FEATURES="$FEATURES" \
    "$TARGET"
