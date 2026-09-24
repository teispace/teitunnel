#!/usr/bin/env bash
# Checks a released Linux package on a real distribution (CI runner or container):
# installs it with the system's package manager (so dependencies must resolve), checks
# the desktop entry and icon, runs the app on a virtual display with a window manager in
# light and dark with screenshots, checks that its tray icon registers over D-Bus, and
# removes it.
#
#   linux.sh <package.deb|package.rpm> <out-dir>
set -euo pipefail

package=$(realpath "$1") out=$(realpath -m "$2")
here=$(cd "$(dirname "$0")" && pwd)
mkdir -p "$out"
failures=0
fail() { echo "::error::$*"; failures=$((failures + 1)); }
sudo() { if [ "$(id -u)" = 0 ]; then "$@"; else command sudo "$@"; fi; }

# shellcheck source=/dev/null
. /etc/os-release
echo "Linux: $PRETTY_NAME ($(uname -m))"

case "$package" in
  *.deb)
    sudo apt-get update -qq
    sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
      xvfb openbox imagemagick dbus-x11 python3-gi >/dev/null
    sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq "$package" >/dev/null
    files=$(dpkg -L teitunnel)
    remove=(apt-get remove -y -qq teitunnel)
    ;;
  *.rpm)
    sudo dnf install -y -q xorg-x11-server-Xvfb openbox ImageMagick dbus-daemon dbus-x11 \
      python3-gobject procps-ng >/dev/null
    sudo dnf install -y -q "$package" >/dev/null
    files=$(rpm -ql teitunnel)
    remove=(dnf remove -y -q teitunnel)
    ;;
  *) echo "Not a .deb or .rpm: $package" >&2; exit 2 ;;
esac

echo "$files" >"$out/files.txt"
app=$(grep -E '^/usr/bin/[Tt]eitunnel$' <<<"$files" || true)
[ -n "$app" ] || { fail "No /usr/bin/Teitunnel in the package"; exit 1; }
grep -q '^/usr/bin/teitunnel-cli$' <<<"$files" || fail "teitunnel-cli isn't in the package"
desktop=$(grep -E '\.desktop$' <<<"$files" | head -1)
[ -n "$desktop" ] || fail "No desktop entry"
[ -n "$desktop" ] && cp "$desktop" "$out/"
grep -qE '/icons/hicolor/.*/apps/' <<<"$files" || fail "No icon in the hicolor theme"
teitunnel-cli --version

# A virtual display, a session bus, a window manager, and a tray watcher.
export DISPLAY=:99
Xvfb "$DISPLAY" -screen 0 1440x900x24 >/dev/null 2>&1 &
sleep 2
eval "$(dbus-launch --sh-syntax)"
openbox >/dev/null 2>&1 &
python3 "$here/sni-watcher.py" "$out/tray.txt" &
sleep 2

run() { # name, extra environment
  local name=$1
  shift
  env "$@" "$app" >"$out/$name.log" 2>&1 &
  local pid=$!
  sleep 15
  if ! kill -0 "$pid" 2>/dev/null; then
    fail "Teitunnel exited on launch ($name)"
    tail -20 "$out/$name.log"
  fi
  import -window root "$out/$name.png"
  kill "$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
}

run light
grep -q . "$out/tray.txt" || fail "The tray icon didn't register with the StatusNotifierWatcher"
cat "$out/tray.txt"
run dark GTK_THEME=Adwaita:dark

sudo "${remove[@]}" >/dev/null
[ ! -e "$app" ] || fail "Still installed after removal: $app"

[ "$failures" = 0 ] || exit 1
echo "Linux desktop check passed"
