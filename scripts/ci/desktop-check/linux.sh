#!/usr/bin/env bash
# Checks a released Linux package on a real distribution (CI runner or container):
# installs it with the system's package manager (so dependencies must resolve), checks
# the desktop entry and icon, runs the app on a virtual display with a window manager in
# light and dark with screenshots, checks that its tray icon registers over D-Bus, and
# removes it.
#
#   linux.sh <package.deb|package.rpm|package.AppImage> <out-dir>
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
  *.AppImage)
    # Runs as downloaded: no install, and only the libraries it bundles plus the system's.
    # AppImages never bundle the graphics stack (EGL, GL): it has to match the machine's
    # drivers, and every desktop has it. The runner is a server image without one, so it
    # gets Mesa's, as a desktop would have.
    sudo apt-get update -qq
    sudo env DEBIAN_FRONTEND=noninteractive apt-get install -y -qq \
      xvfb openbox imagemagick dbus-x11 python3-gi libegl1 libgl1 libgles2 >/dev/null
    chmod +x "$package"
    export APPIMAGE_EXTRACT_AND_RUN=1
    root=$(mktemp -d)
    (cd "$root" && "$package" --appimage-extract >/dev/null)
    files=$(cd "$root/squashfs-root" && find . -type f -o -type l | sed 's|^\.||')
    app=$package
    remove=()
    ;;
  *) echo "Not a .deb, .rpm or .AppImage: $package" >&2; exit 2 ;;
esac

echo "$files" >"$out/files.txt"
if [ ${#remove[@]} -gt 0 ]; then
  # The app is /usr/bin/Teitunnel; the command /usr/bin/teitunnel.
  app=$(grep -x '/usr/bin/Teitunnel' <<<"$files" || true)
  [ -n "$app" ] || { fail "No /usr/bin/Teitunnel in the package"; exit 1; }
  grep -qx '/usr/bin/teitunnel' <<<"$files" || fail "The teitunnel command isn't in the package"
  desktop=$(grep -E '\.desktop$' <<<"$files" | head -1)
  [ -n "$desktop" ] || fail "No desktop entry"
  [ -n "$desktop" ] && cp "$desktop" "$out/"
  grep -qE '/icons/hicolor/.*/apps/' <<<"$files" || fail "No icon in the hicolor theme"
  teitunnel --version | grep '^teitunnel ' || fail "teitunnel --version failed"
else
  grep -qx '/usr/bin/teitunnel' <<<"$files" || fail "The teitunnel command isn't in the AppImage"
  grep -qE '^/[^/]+\.desktop$' <<<"$files" || fail "No desktop entry in the AppImage"
  grep -qE '/libayatana-appindicator3\.so' <<<"$files" || fail "The AppImage doesn't bundle the tray library"
fi

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
  # In its own process group: an AppImage's launcher starts the app as a child, and a copy
  # left running would take the next launch over as the single instance.
  setsid env "$@" "$app" >"$out/$name.log" 2>&1 &
  local pid=$!
  sleep 15
  if ! kill -0 "$pid" 2>/dev/null; then
    fail "Teitunnel exited on launch ($name)"
    tail -20 "$out/$name.log"
  fi
  import -window root "$out/$name.png"
  kill -- "-$pid" 2>/dev/null || true
  wait "$pid" 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -g "$pid" >/dev/null || return 0; sleep 1; done
  kill -KILL -- "-$pid" 2>/dev/null || true
}

run light
grep -q . "$out/tray.txt" || fail "The tray icon didn't register with the StatusNotifierWatcher"
cat "$out/tray.txt"
run dark GTK_THEME=Adwaita:dark

if [ ${#remove[@]} -gt 0 ]; then
  sudo "${remove[@]}" >/dev/null
  [ ! -e "$app" ] || fail "Still installed after removal: $app"
fi

[ "$failures" = 0 ] || exit 1
echo "Linux desktop check passed"
