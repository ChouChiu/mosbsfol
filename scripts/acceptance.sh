#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# MOSBSFOL 手工验收脚本：从源码构建并逐项检查六项 macOS 特性。
set -euo pipefail

cd "$(dirname "$0")/.."
cargo build --release --quiet
BIN="$PWD/target/release/mosbsfol"
WORK="$(mktemp -d /tmp/mosbsfol-accept.XXXXXX)"
trap 'rm -rf "$WORK"' EXIT

pass()  { printf '\033[32m[PASS]\033[0m %s\n' "$*"; }
fail()  { printf '\033[31m[FAIL]\033[0m %s\n' "$*"; exit 1; }

# 1. .DS_Store
mkdir -p "$WORK/tree/sub"
echo hello > "$WORK/tree/a.txt"
echo world > "$WORK/tree/sub/b.txt"
"$BIN" poop "$WORK/tree" -r >/dev/null
[ -f "$WORK/tree/.DS_Store" ] || fail "missing .DS_Store"
[ -f "$WORK/tree/sub/.DS_Store" ] || fail "missing recursive .DS_Store"
"$BIN" dsstore inspect "$WORK/tree/.DS_Store" | grep -q 'name="a.txt"' || fail "a.txt not in .DS_Store"
"$BIN" dsstore inspect "$WORK/tree/.DS_Store" | grep -q 'id=bwsp' || fail "bwsp window record missing"
pass ".DS_Store poop / inspect"

# 2. AppleDouble / USB / volume traces
"$BIN" usb "$WORK/tree" -r --type-codes >/dev/null
[ -f "$WORK/tree/._a.txt" ] || fail "missing ._a.txt sidecar"
[ "$(xxd -p -l 4 "$WORK/tree/._a.txt")" = "00051607" ] || fail "bad AppleDouble magic"
[ -d "$WORK/tree/.Spotlight-V100" ] || fail "missing .Spotlight-V100"
[ -d "$WORK/tree/.fseventsd" ] || fail "missing .fseventsd"
[ -d "$WORK/tree/.Trashes" ] || fail "missing .Trashes"
[ -d "$WORK/tree/.TemporaryItems" ] || fail "missing .TemporaryItems"
[ -f "$WORK/tree/.localized" ] || fail "missing .localized"
[ -f "$WORK/tree/.VolumeIcon.icns" ] || fail "missing .VolumeIcon.icns"
pass "usb / AppleDouble ._* / volume traces"

# 3. plist
"$BIN" plist write "$WORK/demo.plist" name=mosbsfol answer=42 enabled=true >/dev/null
[ "$(xxd -p -l 8 "$WORK/demo.plist")" = "62706c6973743030" ] || fail "binary plist magic missing"
"$BIN" plist read "$WORK/demo.plist" | grep -q '"answer":42' || fail "binary plist readback"
"$BIN" plist write "$WORK/demo.xml.plist" name=mosbsfol --xml >/dev/null
grep -q '<plist version="1.0">' "$WORK/demo.xml.plist" || fail "XML plist missing"
pass "plist binary/XML write & read"

# 4. xattr（文件系统不支持 user xattr 时自动跳过）
touch "$WORK/x.bin"
if "$BIN" xattr quarantine "$WORK/x.bin" 2>/dev/null; then
    "$BIN" xattr get "$WORK/x.bin" com.apple.quarantine | grep -q '0083;' || fail "quarantine xattr value"
    "$BIN" xattr tag "$WORK/x.bin" red | grep -q 'tag: red' || fail "Finder tag"
    "$BIN" xattr hide "$WORK/x.bin" yes | grep -q 'hidden: true' || fail "Finder hidden flag"
    "$BIN" xattr resourcefork "$WORK/x.bin" deadbeef >/dev/null
    "$BIN" xattr resourcefork "$WORK/x.bin" | grep -q 'de ad be ef' || fail "resource fork xattr"
    pass "xattr quarantine / tag / hide / resourcefork"
else
    echo "[SKIP] filesystem does not support user xattr; use: mosbsfol usb"
fi

# 5. __MACOSX ZIP
"$BIN" maczip "$WORK/tree" "$WORK/tree.zip" >/dev/null
python3 - "$WORK/tree.zip" <<'PY' || fail "__MACOSX entry missing from ZIP"
import sys, zipfile
names = set(zipfile.ZipFile(sys.argv[1]).namelist())
assert 'a.txt' in names, sorted(names)
assert '__MACOSX/._a.txt' in names, sorted(names)
PY
pass "maczip / __MACOSX AppleDouble entries"

echo
echo "All MOSBSFOL acceptance checks passed."
