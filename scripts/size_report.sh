#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

format_bytes() {
    local bytes="$1"
    awk -v b="$bytes" 'BEGIN {
        if (b >= 1048576) printf "%.2f MB (%d B)", b / 1048576.0, b;
        else if (b >= 1024) printf "%.1f KB (%d B)", b / 1024.0, b;
        else printf "%d B", b;
    }'
}

echo "=== CraftUI Release Footprint Report ==="

echo "[1/3] Desktop release binary (examples/counter, opt-level=z, lto=thin, strip=true)..."
cargo build --release -p craftui-example-counter >/dev/null
COUNTER_BIN="target/release/craftui-counter"
if [[ -f "$COUNTER_BIN" ]]; then
    SIZE="$(stat -c%s "$COUNTER_BIN" 2>/dev/null || wc -c <"$COUNTER_BIN")"
    echo "  craftui-counter (stripped release): $(format_bytes "$SIZE")"
fi

if rustup target list --installed | grep -qx "wasm32-unknown-unknown"; then
    echo "[2/3] Web WASM library (craftui-platform-web, release gpu-sdf)..."
    cargo build --release --target wasm32-unknown-unknown -p craftui-platform-web --no-default-features --features gpu-sdf >/dev/null
    WASM_MIN="target/wasm32-unknown-unknown/release/libcraftui_platform_web.rlib"
    if [[ -f "$WASM_MIN" ]]; then
        SIZE="$(stat -c%s "$WASM_MIN" 2>/dev/null || wc -c <"$WASM_MIN")"
        echo "  libcraftui_platform_web.rlib (wasm32 gpu-sdf): $(format_bytes "$SIZE")"
    fi
else
    echo "[2/3] Web WASM target not installed (run: rustup target add wasm32-unknown-unknown)"
fi

NDK_HOME="${ANDROID_NDK_HOME:-${ANDROID_NDK_ROOT:-${ANDROID_NDK_LATEST_HOME:-}}}"
if [[ -z "$NDK_HOME" && -d "$HOME/Android/Sdk/ndk" ]]; then
    NDK_HOME="$(ls -1d "$HOME"/Android/Sdk/ndk/* 2>/dev/null | sort -V | tail -n 1 || true)"
fi
if rustup target list --installed | grep -qx "aarch64-linux-android" && [[ -n "$NDK_HOME" ]]; then
    echo "[3/3] Android NDK library check ($NDK_HOME)..."
    NDK_BIN="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin"
    if [[ -x "${NDK_BIN}/aarch64-linux-android26-clang" ]]; then
        CC_aarch64_linux_android="${NDK_BIN}/aarch64-linux-android26-clang" \
        CXX_aarch64_linux_android="${NDK_BIN}/aarch64-linux-android26-clang++" \
        AR_aarch64_linux_android="${NDK_BIN}/llvm-ar" \
        cargo build --release --target aarch64-linux-android -p craftui-platform-android --no-default-features --features gpu-sdf >/dev/null
        ANDROID_RLIB="target/aarch64-linux-android/release/libcraftui_platform_android.rlib"
        if [[ -f "$ANDROID_RLIB" ]]; then
            SIZE="$(stat -c%s "$ANDROID_RLIB" 2>/dev/null || wc -c <"$ANDROID_RLIB")"
            echo "  libcraftui_platform_android.rlib (arm64 gpu-sdf): $(format_bytes "$SIZE")"
        fi
    else
        echo "  NDK clang not found under $NDK_BIN"
    fi
else
    echo "[3/3] Android target/NDK skipped cleanly (target or NDK missing)"
fi

echo "Done."
