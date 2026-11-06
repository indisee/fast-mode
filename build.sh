#!/bin/bash
# Builds "Fast Mode.app" into ./build
set -euo pipefail
cd "$(dirname "$0")"
swift build -c release
APP="build/Fast Mode.app"
rm -rf "$APP"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp Icon/AppIcon.icns "$APP/Contents/Resources/AppIcon.icns"
cp .build/release/FastMode "$APP/Contents/MacOS/FastMode"
cat > "$APP/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Fast Mode</string>
  <key>CFBundleDisplayName</key><string>Fast Mode</string>
  <key>CFBundleIdentifier</key><string>local.fastmode</string>
  <key>CFBundleExecutable</key><string>FastMode</string>
  <key>CFBundleIconFile</key><string>AppIcon</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>1.0</string>
  <key>CFBundleVersion</key><string>1</string>
  <key>LSMinimumSystemVersion</key><string>15.0</string>
  <key>LSUIElement</key><true/>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST
codesign --force -s - "$APP" >/dev/null
echo "Built: $(pwd)/$APP"
