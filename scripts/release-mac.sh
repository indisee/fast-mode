#!/usr/bin/env bash
# Build a universal macOS release, sign it with a Developer ID, notarize it,
# and package it as a DMG and a zip in dist/.
#
#   scripts/release-mac.sh             # build, sign, notarize, package
#   scripts/release-mac.sh --publish   # same, then create the GitHub release vX.Y.Z
#
# No passwords or API keys: signing and notarization go through the Apple
# account signed in to Xcode (Settings → Accounts). Xcode creates the
# Developer ID certificate in the cloud if the team has none yet.
#
# Needs: Xcode, Rust with aarch64- and x86_64-apple-darwin targets, Node,
# an "Apple Development" certificate in the keychain (used only to add the
# Hardened Runtime flag before Xcode re-signs), and gh for --publish.
set -euo pipefail

TEAM_ID="${TEAM_ID:-9A4AVCBGXW}"
PUBLISH=0
[[ "${1:-}" == "--publish" ]] && PUBLISH=1

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"
export PATH="$HOME/.cargo/bin:$PATH"

VERSION="$(node -p 'require("./src-tauri/tauri.conf.json").version')"
BUNDLE_ID="$(node -p 'require("./src-tauri/tauri.conf.json").identifier')"
NAME="Fast-Mode-$VERSION-macOS-universal"
OUT="$ROOT/dist/release-$VERSION"
WORK="$OUT/work"
ARCHIVE="$WORK/FastMode.xcarchive"
APP_IN_ARCHIVE="$ARCHIVE/Products/Applications/Fast Mode.app"

say() { printf '\n==> %s\n' "$*"; }

if [[ $PUBLISH == 1 ]] && gh release view "v$VERSION" >/dev/null 2>&1; then
  echo "Release v$VERSION already exists. Bump the version first." >&2
  exit 1
fi

DEV_ID="$(security find-identity -v -p codesigning | awk -F'"' '/Apple Development/ {print $2; exit}')"
if [[ -z "$DEV_ID" ]]; then
  echo "No 'Apple Development' certificate in the keychain." >&2
  exit 1
fi

say "Building $VERSION (universal)"
npx tauri build --target universal-apple-darwin --bundles app
BUILT="src-tauri/target/universal-apple-darwin/release/bundle/macos/Fast Mode.app"

say "Wrapping it in an Xcode archive"
rm -rf "$OUT"
mkdir -p "$ARCHIVE/Products/Applications"
ditto "$BUILT" "$APP_IN_ARCHIVE"
# Xcode refuses Developer ID export without the Hardened Runtime flag.
codesign --force --options runtime --timestamp=none -s "$DEV_ID" "$APP_IN_ARCHIVE"

cat > "$ARCHIVE/Info.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>ApplicationProperties</key><dict>
<key>ApplicationPath</key><string>Applications/Fast Mode.app</string>
<key>Architectures</key><array><string>arm64</string><string>x86_64</string></array>
<key>CFBundleIdentifier</key><string>$BUNDLE_ID</string>
<key>CFBundleShortVersionString</key><string>$VERSION</string>
<key>CFBundleVersion</key><string>$VERSION</string>
<key>Team</key><string>$TEAM_ID</string>
</dict>
<key>ArchiveVersion</key><integer>2</integer>
<key>CreationDate</key><date>$(date -u +%Y-%m-%dT%H:%M:%SZ)</date>
<key>Name</key><string>Fast Mode</string>
<key>SchemeName</key><string>Fast Mode</string>
</dict></plist>
EOF

cat > "$WORK/export.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0"><dict>
<key>method</key><string>developer-id</string>
<key>destination</key><string>upload</string>
<key>teamID</key><string>$TEAM_ID</string>
<key>signingStyle</key><string>automatic</string>
</dict></plist>
EOF

say "Signing with Developer ID and uploading for notarization"
xcodebuild -exportArchive -archivePath "$ARCHIVE" -exportOptionsPlist "$WORK/export.plist" \
  -exportPath "$WORK/upload" -allowProvisioningUpdates 2>&1 | grep -v 'Progress [0-9]*%'

say "Waiting for Apple (up to 20 min)"
NOTARIZED="$WORK/notarized/Fast Mode.app"
for _ in $(seq 1 60); do
  rm -rf "$WORK/notarized"
  if xcodebuild -exportNotarizedApp -archivePath "$ARCHIVE" -exportPath "$WORK/notarized" >"$WORK/notarize.log" 2>&1; then
    break
  fi
  if grep -qi 'invalid\|rejected' "$WORK/notarize.log"; then
    cat "$WORK/notarize.log" >&2
    exit 1
  fi
  sleep 20
done
if [[ ! -d "$NOTARIZED" ]]; then
  echo "Notarization did not finish in time, see $WORK/notarize.log" >&2
  exit 1
fi

say "Checking signature and Gatekeeper"
xcrun stapler validate "$NOTARIZED"
spctl -a -vv -t exec "$NOTARIZED" 2>&1 | tee "$WORK/spctl.log"
grep -q 'source=Notarized Developer ID' "$WORK/spctl.log"

say "Packaging"
mkdir -p "$WORK/dmgroot"
ditto "$NOTARIZED" "$WORK/dmgroot/Fast Mode.app"
ln -s /Applications "$WORK/dmgroot/Applications"
hdiutil create -quiet -volname "Fast Mode" -srcfolder "$WORK/dmgroot" -format UDZO -ov "$OUT/$NAME.dmg"
(cd "$WORK/notarized" && ditto -c -k --keepParent "Fast Mode.app" "$OUT/$NAME.zip")
ls -la "$OUT"/*.dmg "$OUT"/*.zip

if [[ $PUBLISH == 1 ]]; then
  say "Publishing v$VERSION"
  NOTES="$OUT/notes.md"
  cat > "$NOTES" <<EOF
Ready-to-run macOS build (universal: Apple Silicon + Intel, macOS 11+), signed with a Developer ID and notarized by Apple.

**Download:** \`$NAME.dmg\` (drag to Applications) or the \`.zip\` with the bare \`.app\`.

Needs [Claude Code](https://docs.claude.com/en/docs/claude-code) installed and signed in.
EOF
  git push origin HEAD
  gh release create "v$VERSION" --target "$(git rev-parse HEAD)" --title "Fast Mode $VERSION" \
    --notes-file "$NOTES" "$OUT/$NAME.dmg" "$OUT/$NAME.zip"
  echo "Edit the notes to list what changed: gh release edit v$VERSION"
fi
