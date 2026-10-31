#!/bin/bash
# 构建 Rust core 与 release 版本的宿主，打包为 Sayso.app。
# 用法：bash scripts/bundle-app.sh            → 输出 build/Sayso.app
#       INSTALL=1 bash scripts/bundle-app.sh  → 额外安装到 /Applications
#
# 签名：默认使用登录钥匙串中的 Apple Development 证书，可用 CODESIGN_IDENTITY 指定。
# 证书签名的 designated requirement 是「Bundle ID + 证书」，重新打包后不变，
# 系统授予的辅助功能、麦克风权限与钥匙串访问都不会失效。
# 没有证书时退回 ad-hoc 签名：其身份是每次构建的 cdhash，重新打包后需要重新授权。
set -euo pipefail

PROJECT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
APP="$PROJECT_DIR/build/Sayso.app"

IDENTITY="${CODESIGN_IDENTITY:-$(security find-identity -v -p codesigning \
    | awk -F'"' '/"Apple Development:/ {print $2; exit}')}"

bash "$PROJECT_DIR/scripts/build-core.sh"
swift build -c release --package-path "$PROJECT_DIR"
BIN="$(swift build -c release --package-path "$PROJECT_DIR" --show-bin-path)/Sayso"

rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS"
cp "$BIN" "$APP/Contents/MacOS/Sayso"
cp "$PROJECT_DIR/Resources/Info.plist" "$APP/Contents/Info.plist"

if [ -n "$IDENTITY" ]; then
    codesign --force --sign "$IDENTITY" "$APP"
else
    echo "WARNING: no Apple Development identity found; using ad-hoc signing (permissions reset on every rebuild)." >&2
    codesign --force --sign - "$APP"
fi
codesign --verify "$APP"
echo "Built $APP"
echo "Designated requirement: $(codesign -d -r- "$APP" 2>&1 | sed -n 's/^designated => //p')"

if [ "${INSTALL:-0}" = "1" ]; then
    rm -rf /Applications/Sayso.app
    cp -R "$APP" /Applications/Sayso.app
    echo "Installed /Applications/Sayso.app"
fi
