#!/bin/bash
# 构建 Rust core 并生成 Swift 绑定。
# 输出：build/SaysoCoreFFI.xcframework（静态库 + C 头文件）与 Sources/SaysoCore/SaysoCore.swift（生成的绑定）。
# 用法：bash scripts/build-core.sh            → release 构建
#       PROFILE=debug bash scripts/build-core.sh
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CORE_DIR="$PROJECT_DIR/core"
PROFILE="${PROFILE:-release}"
BUILD_DIR="$PROJECT_DIR/build"
GENERATED="$BUILD_DIR/core-bindings"
XCFRAMEWORK="$BUILD_DIR/SaysoCoreFFI.xcframework"

CARGO_FLAGS=(--manifest-path "$CORE_DIR/Cargo.toml" -p sayso-core)
[ "$PROFILE" = "release" ] && CARGO_FLAGS+=(--release)
# 与 Package.swift 的 platforms 保持一致，否则 C 依赖按本机系统版本编译，链接时报版本不匹配。
export MACOSX_DEPLOYMENT_TARGET=14.0
cargo build "${CARGO_FLAGS[@]}"
LIBRARY="$CORE_DIR/target/$PROFILE/libsayso_core.a"

rm -rf "$GENERATED"
# 库模式读取 cargo metadata 找到 uniffi.toml，需要在 workspace 内运行。
(cd "$CORE_DIR" && cargo run --quiet -p uniffi-bindgen -- \
    generate --library "$LIBRARY" --language swift --out-dir "$GENERATED")

# xcframework 的头文件目录中，模块描述文件必须命名为 module.modulemap。
HEADERS="$GENERATED/headers"
mkdir -p "$HEADERS"
cp "$GENERATED/SaysoCoreFFI.h" "$HEADERS/"
cp "$GENERATED/SaysoCoreFFI.modulemap" "$HEADERS/module.modulemap"

rm -rf "$XCFRAMEWORK"
xcodebuild -create-xcframework -library "$LIBRARY" -headers "$HEADERS" -output "$XCFRAMEWORK" >/dev/null
cp "$GENERATED/SaysoCore.swift" "$PROJECT_DIR/Sources/SaysoCore/SaysoCore.swift"
echo "Built $XCFRAMEWORK"
