#!/bin/sh
# Installs Bios from its newest GitHub release:
#   curl -fsSL https://raw.githubusercontent.com/maxwraae/bios-native/main/install.sh | sh
# Into /Applications, or ~/Applications when /Applications/Bios.app is another app or the folder
# is not writable. A file curl downloads is not quarantined, so macOS opens the ad-hoc-signed app.
set -eu

repo=maxwraae/bios-native
id=com.wraae.bios.native

[ "$(uname -s)" = Darwin ] || { echo "Bios runs on macOS only." >&2; exit 1; }
case "$(uname -m)" in
  arm64) arch=aarch64 ;;
  x86_64) arch=x86_64 ;;
  *) echo "Bios has no build for $(uname -m)." >&2; exit 1 ;;
esac

url=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" |
  grep -o '"browser_download_url": *"[^"]*"' | sed 's/.*"\(https:[^"]*\)"$/\1/' |
  grep -E "/bios-[^/]*-macos-($arch|universal)\.dmg$" | head -n 1) || true
[ -n "$url" ] || { echo "No Bios release for this Mac ($arch) at https://github.com/$repo/releases" >&2; exit 1; }
name=${url##*/}

tmp=$(mktemp -d)
trap 'hdiutil detach -quiet "$tmp/mnt" 2>/dev/null || true; rm -rf "$tmp"' EXIT
echo "Downloading $name"
curl -fL --progress-bar -o "$tmp/$name" "$url"
if curl -fsSL -o "$tmp/sums" "${url%/*}/SHA256SUMS.txt"; then
  want=$(grep " \*\{0,1\}$name\$" "$tmp/sums" | cut -d ' ' -f 1)
  got=$(shasum -a 256 "$tmp/$name" | cut -d ' ' -f 1)
  [ "$want" = "$got" ] || { echo "The download does not match the release's checksum; nothing was installed." >&2; exit 1; }
fi

# Bios already installed is replaced where it is.
ours() { [ "$(defaults read "$1/Contents/Info" CFBundleIdentifier 2>/dev/null)" = "$id" ]; }
if ours /Applications/Bios.app; then dest=/Applications
elif ours "$HOME/Applications/Bios.app"; then dest=$HOME/Applications
elif [ ! -e /Applications/Bios.app ] && [ -w /Applications ]; then dest=/Applications
else dest=$HOME/Applications
fi
mkdir -p "$dest"
app=$dest/Bios.app

hdiutil attach -quiet -nobrowse -readonly -mountpoint "$tmp/mnt" "$tmp/$name"
rm -rf "$app.new"
ditto "$tmp/mnt/Bios.app" "$app.new"
if [ "$(osascript -e "application id \"$id\" is running" 2>/dev/null)" = true ]; then
  osascript -e "tell application id \"$id\" to quit" >/dev/null 2>&1 || true
fi
rm -rf "$app"
mv "$app.new" "$app"

echo "Installed Bios in $app"
echo "Connect an agent (Claude Code):"
echo "  claude mcp add --scope user bios -- \"$app/Contents/MacOS/bios-cli\" mcp"
