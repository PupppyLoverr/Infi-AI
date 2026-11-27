#!/bin/bash
# Runs inside the Hyprland session: open every surface, screenshot it to the
# host, then measure idle RAM 5 minutes after login and report.
H=http://10.0.2.2:8088
T0=$(date +%s)
up() { curl -fsS -T "$1" "$H/$(basename "$1")" >/dev/null 2>&1; }
shot() { sleep "${2:-1.2}"; grim "/tmp/$1.png" && up "/tmp/$1.png"; echo "shot $1"; }
q() { qs ipc call shell "$@"; }
sleep 14
shot 01-desktop-idle-notch 0
mpv --no-video --idle=no --loop=inf --force-media-title="Harbour Lights" "av://lavfi:sine=frequency=220" >/dev/null 2>&1 &
sleep 3; shot 02-notch-live-activity 0
q island media; shot 03-island-media
for t in "Launch review moved to 10:00. Same room." "https://github.com/PupppyLoverr/Infi-AI" "#FF9F43" "showcase-clip.mp4"; do printf '%s' "$t" | wl-copy; sleep 1.4; done
q island clip; shot 04-island-clipboard 1.5
[ -f "$HOME/.local/share/solyra/clipboard.json" ] && cp "$HOME/.local/share/solyra/clipboard.json" /tmp/clipboard-persisted.json && up /tmp/clipboard-persisted.json
q island shelf; shot 05-island-shelf
q island cal; shot 06a-island-calendar
q island face; shot 06b-island-notchface 2
q island tools; shot 06-island-tools
q island close; sleep 0.8
q hud volume 0.62; shot 07-hud-volume 0.5
sleep 2; touch "$HOME/Downloads/report.pdf"; q island shelf; shot 07b-tracked-downloads-on-shelf
q island close; sleep 1
sleep 2
# Two live activities at once: the timer rides in the island, the music
# splits off into its own bubble.
q timer 300; shot 07c-live-activities 2.0
q timer 0; sleep 0.8
q record; shot 07d-recording-activity 3.0
q record; sleep 1.5
q query "te"; shot 08-spotlight-in-island 1.6
q spotlight; sleep 0.5
q control; shot 09-control-center
q control; sleep 0.5
notify-send -a "Solyra" "Welcome to Solyra" "Notifications arrive in the corner and respect Focus."; shot 10-notification 1.0
notify-send -a "Clock" "Timer" "Your 5-minute timer is done."; sleep 0.5
q notifications; shot 10b-notification-center 1.2
q notifications; sleep 0.5
sleep 5
q snap; shot 11-snap-layouts
q snap; sleep 0.5
solyra-term >/tmp/term1.log 2>&1 &
shot 12-terminal 6
qs ipc call shell overview; shot 12b-mission-control 1.5
qs ipc call shell overview; sleep 0.6
hyprctl dispatch 'hl.dsp.window.close()' >/dev/null; sleep 1
q glass 90; shot 14-glass-tinted 1.0
q glass 0; shot 15-glass-clear 1.0
solyra-files >/tmp/files.log 2>&1 &
shot 15b-finder 4
hyprctl dispatch "hl.dsp.window.move({ workspace = 'special:minimized', follow = false })" >/dev/null
shot 15c-dock-minimized 1.5
pkill -x solyra-files; sleep 1
q glass 35
# Populate Downloads with real files and verify the interactive macOS 27 Downloads Stack popover.
mkdir -p "$HOME/Downloads"
for f in "Solyra-0.1.iso" "Design-Spec.pdf" "wallpaper-4k.png" "release-notes.txt" "assets.zip"; do touch "$HOME/Downloads/$f"; done
q downloads; shot 15d-dock-downloads-stack 1.4
q downloads; sleep 0.5
q island close; sleep 0.5

# Verify solyra-app universal installer CLI + live LRCLIB synced lyrics API reachability.
solyra-app help > /tmp/solyra-app-check.txt 2>&1; up /tmp/solyra-app-check.txt
curl -fsSL --max-time 6 -G -H 'User-Agent: SolyraOS/0.1' 'https://lrclib.net/api/get' \
  --data-urlencode 'artist_name=Daft Punk' --data-urlencode 'track_name=Around the World' \
  > /tmp/lrclib-live-check.txt 2>&1 || true
up /tmp/lrclib-live-check.txt

# Seed a local .lrc for the public-domain Erik Satie track played in Firefox so
# both the Notch Live Synced Lyrics drawer and the Droppy Ambient Lock Screen
# exercise real-time timestamped line tracking alongside live Firefox MPRIS playback.
mkdir -p "$HOME/.local/share/solyra/lyrics"
cat > "$HOME/.local/share/solyra/lyrics/Erik Satie - Gymnopédie No. 1.lrc" <<'LRC'
[00:00.00] Lent et douloureux — Three Gymnopédies (1888)
[00:08.00] Gentle alternating seventh chords open in D major
[00:18.00] A quiet waltz melody rises over a steady left-hand pulse
[00:30.00] Soft modal harmonies drift across the piano register
[00:45.00] The theme returns, suspended and calm in half-light
[01:00.00] Fading slowly into stillness along the final cadence
LRC

# A real song on the web: Firefox plays it from a page with Media Session
# metadata; the notch, its live activities, synced lyrics drawer, Control Center
# and the Droppy Ambient Lock Screen pick it up over MPRIS like YouTube or Spotify web.
pkill -x mpv; sleep 1
mkdir -p /tmp/ffp
cat > /tmp/ffp/user.js <<'JS'
user_pref("media.autoplay.default", 0);
user_pref("media.autoplay.blocking_policy", 0);
user_pref("media.hardwaremediakeys.enabled", true);
user_pref("browser.aboutwelcome.enabled", false);
user_pref("browser.shell.checkDefaultBrowser", false);
user_pref("datareporting.policy.dataSubmissionEnabled", false);
user_pref("datareporting.policy.firstRunURL", "");
user_pref("browser.startup.homepage_override.mstone", "ignore");
user_pref("trailhead.firstrun.didSeeAboutWelcome", true);
JS
firefox --no-remote --profile /tmp/ffp --new-window "$H/web/index.html" >/tmp/firefox.log 2>&1 &
sleep 20
playerctl -l > /tmp/players.txt 2>&1; playerctl -p firefox metadata >> /tmp/players.txt 2>&1; up /tmp/players.txt
shot 17-web-song-playing 0
q island media; shot 17b-web-song-notch 2.0
q island lyrics; shot 17e-web-song-notch-lyrics 2.0
q island close; sleep 0.8
q timer 300; shot 17c-web-song-two-activities 2.0
q control; shot 17d-web-song-control-center 1.2
q control; q timer 0; sleep 0.6
q lock; shot 16-lock-screen 3.0
# The test media player is part of the scenario, not the desktop: stop it,
# then idle until 3 minutes after login and measure (nothing excluded).
pkill -x mpv; pkill -x firefox; qs ipc call shell island close
while [ $(( $(date +%s) - T0 + 14 )) -lt 180 ]; do sleep 5; done
{
  echo "=== solyra-vm report ==="
  echo "VM: 2 vCPU, 4 GiB RAM, virtio-vga (llvmpipe)"
  free -m
  systemd-analyze 2>&1 | head -2
  echo "root_used_mb=$(df -m --output=used / | tail -1)"
  echo "packages=$(pacman -Qq | wc -l)"
  echo "--- top RSS (KiB)"; ps -eo rss,comm --sort=-rss | head -20
  echo "total_rss_kib=$(ps -eo rss= | awk '{s+=$1} END{print s}')"
  hyprctl version | head -2
  qs --version 2>/dev/null | head -1
} > /tmp/report.txt 2>&1
up /tmp/report.txt
for f in /tmp/qs.log /tmp/hypr.log /tmp/scenario.log /tmp/term1.log /tmp/firefox.log; do [ -f "$f" ] && cp "$f" "/tmp/log-$(basename $f)" && up "/tmp/log-$(basename $f)"; done
touch /tmp/solyra-done
