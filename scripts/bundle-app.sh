#!/bin/bash
# 构建 Rust core 与 release 版本的宿主，打包为 Sayso.app。
# 用法：bash scripts/bundle-app.sh            → 输出 build/Sayso.app
#       INSTALL=1 bash scripts/bundle-app.sh  → 额外安装到 /Applications
#       VERSION=1.2.0 bash scripts/bundle-app.sh → 写入版本号（发布流水线从 tag 传入）
#
# 签名：默认使用登录钥匙串中的 Apple Development 证书，可用 CODESIGN_IDENTITY 指定
# （发布流水线传入 Developer ID Application 证书，签名后再公证）。
# 证书签名的 designated requirement 是「Bundle ID + 证书」，重新打包后不变，
# 系统授予的辅助功能与麦克风权限都不会失效。
# 没有证书时退回 ad-hoc 签名：其身份是每次构建的 cdhash，重新打包后需要重新授权。
# 始终开启 Hardened Runtime，本地构建与公证版本的运行时限制一致。
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP="$PROJECT_DIR/build/Sayso.app"

IDENTITY="${CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning \
    | awk -F'"' '/"Apple Development:/ {print $2; exit}')}"

bash "$PROJECT_DIR/scripts/build-core.sh"
swift build -c release --package-path "$PROJECT_DIR"
BIN="$(swift build -c release --package-path "$PROJECT_DIR" --show-bin-path)/Sayso"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN" "$APP/Contents/MacOS/Sayso"
cp "$PROJECT_DIR/Resources/Info.plist" "$APP/Contents/Info.plist"
if [ -n "${VERSION:-}" ]; then
    /usr/libexec/PlistBuddy \
        -c "Set :CFBundleShortVersionString $VERSION" \
        -c "Set :CFBundleVersion $VERSION" \
        "$APP/Contents/Info.plist"
fi
# 图标由 scripts/render-icons.swift 从 Resources/AppIcon.svg 生成。
cp "$PROJECT_DIR/Resources/AppIcon.icns" "$APP/Contents/Resources/AppIcon.icns"

SIGN_FLAGS=(--force --options runtime --entitlements "$PROJECT_DIR/Resources/Sayso.entitlements")
if [ -n "$IDENTITY" ]; then
    # 公证要求可信时间戳。
    codesign "${SIGN_FLAGS[@]}" --timestamp --sign "$IDENTITY" "$APP"
else
    echo "WARNING: no Apple Development identity found; using ad-hoc signing (permissions reset on every rebuild)." >&2
    codesign "${SIGN_FLAGS[@]}" --sign - "$APP"
fi
codesign --verify "$APP"
echo "Built $APP"
echo "Designated requirement: $(codesign -d -r- "$APP" 2>&1 | sed -n 's/^designated => //p')"

if [ "${INSTALL:-0}" = "1" ]; then
    rm -rf /Applications/Sayso.app
    cp -R "$APP" /Applications/Sayso.app
    echo "Installed /Applications/Sayso.app"
fi
