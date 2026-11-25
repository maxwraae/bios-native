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
# A release without checksums is not installed. Skipping the check quietly, as this once did,
# means installing whatever arrived and saying nothing about it.
curl -fsSL -o "$tmp/sums" "${url%/*}/SHA256SUMS.txt" ||
  { echo "That release has no SHA256SUMS.txt, so the download cannot be checked; nothing was installed." >&2; exit 1; }
want=$(grep " \*\{0,1\}$name\$" "$tmp/sums" | cut -d ' ' -f 1)
[ -n "$want" ] || { echo "SHA256SUMS.txt does not list $name; nothing was installed." >&2; exit 1; }
got=$(shasum -a 256 "$tmp/$name" | cut -d ' ' -f 1)
[ "$want" = "$got" ] || { echo "The download does not match the release's checksum; nothing was installed." >&2; exit 1; }

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

# Proof, not an announcement: the installed program answers a question with a known answer, so a
# copy that unpacked but cannot run says so here instead of the first time someone needs it.
cli=$app/Contents/MacOS/bios-cli
said=$("$cli" run "${TMPDIR:-/tmp}" sequence_stats '{"sequence":"ATGGTGAGCAAGGGCGAGGAGCTG"}' 2>&1 || true)
case $said in
  *64.7*) echo "Checked: it reports a melting temperature of 64.7 C for a test sequence, which is right." ;;
  *) echo "Bios is installed in $app but did not answer correctly, so something is wrong:" >&2; echo "$said" >&2; exit 1 ;;
esac

echo

# Connecting the agent is the half of the install that is easy to forget, so do it here when
# Claude Code is on this machine rather than printing a line and hoping. `claude mcp add` refuses
# a name it already has and leaves the old path in place, so an earlier `bios` is removed first:
# whoever runs this installer means *this* copy to be the one the agent talks to. Only the user
# scope is touched, so a project's shared settings are left alone.
if command -v claude >/dev/null 2>&1; then
  claude mcp remove --scope user bios >/dev/null 2>&1 || true
  if claude mcp add --scope user bios -- "$cli" mcp >/dev/null 2>&1; then
    echo "Connected to Claude Code as \"bios\"."
    echo "Now type /mcp in Claude Code and reconnect \"bios\" — its tools appear only after that."
  else
    echo "Bios is installed, but connecting it to Claude Code did not work. Run this yourself:"
    echo "  claude mcp add --scope user bios -- \"$cli\" mcp"
  fi
else
  echo "Connect an agent (Claude Code):"
  echo "  claude mcp add --scope user bios -- \"$cli\" mcp"
fi
