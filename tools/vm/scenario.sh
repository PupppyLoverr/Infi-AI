#!/bin/bash
# Runs inside the Hyprland session: exercises EVERY macOS 27 Golden Gate &
# Droppy Dynamic Island / Lock Screen surface plus 4K Unsplash Liquid Glass
# tests across 6 wallpapers, captures screenshots to the host, and measures
# idle RAM / disk / boot after login.
H=http://10.0.2.2:8088
T0=$(date +%s)
up() { curl -fsS -T "$1" "$H/$(basename "$1")" >/dev/null 2>&1; }
shot() { sleep "${2:-1.2}"; grim "/tmp/$1.png" && up "/tmp/$1.png"; echo "shot $1"; }
q() { echo "ipc $*" >> /tmp/ipc.log; qs ipc call shell "$@" >> /tmp/ipc.log 2>&1 || echo "  failed: $?" >> /tmp/ipc.log; }
sleep 14
shot 01-desktop-idle-notch 0

# Live MPRIS + PipeWire audio stream for Media, Audio Mixer, and Live Activities
mpv --no-video --idle=no --loop=inf --force-media-title="Harbour Lights" "av://lavfi:sine=frequency=220" >/dev/null 2>&1 &
sleep 3; shot 02-notch-live-activity 0
q island media; shot 03-island-media
q island queue; shot 03b-island-media-queue 1.5

# Rich Clipboard Manager (Text, Link, Color #FF9F43, Video) + persistence check
for t in "Launch review moved to 10:00. Same room." "https://getdroppy.app/blog" "#FF9F43" "showcase-clip.mp4"; do printf '%s' "$t" | wl-copy; sleep 1.4; done
q island clip; shot 04-island-clipboard 1.5
[ -f "$HOME/.local/share/solyra/clipboard.json" ] && cp "$HOME/.local/share/solyra/clipboard.json" /tmp/clipboard-persisted.json && up /tmp/clipboard-persisted.json

# Populate rich files in ~/Downloads for File Tray + compression toast
mkdir -p "$HOME/Downloads/Brand_Assets"
touch "$HOME/Downloads/Brand_Assets/logo.svg" "$HOME/Downloads/Brand_Assets/palette.json"
cp "$HOME/.config/solyra/wallpaper.jpg" "$HOME/Downloads/Coast.jpg" 2>/dev/null || cp "$HOME/.config/solyra/wallpaper.png" "$HOME/Downloads/Coast.png" 2>/dev/null || true
printf '%%PDF-1.4\n%% Droppy Keynote Deck sample\n1 0 obj << /Type /Catalog >> endobj\n' > "$HOME/Downloads/Keynote_Deck.pdf"
dd if=/dev/zero bs=1024 count=64 2>/dev/null >> "$HOME/Downloads/Keynote_Deck.pdf"
printf '# Launch Notes\n- Liquid Glass optics verified\n- 3-mode Lock Screen live\n- File Tray compression ready\n' > "$HOME/Downloads/Notes.md"
q shelfAdd "$HOME/Downloads/Brand_Assets/"
[ -f "$HOME/Downloads/Coast.jpg" ] && q shelfAdd "$HOME/Downloads/Coast.jpg" || q shelfAdd "$HOME/Downloads/Coast.png"
q shelfAdd "$HOME/Downloads/Keynote_Deck.pdf"
q shelfAdd "$HOME/Downloads/Notes.md"
sleep 2
q island shelf; sleep 1.4
q shelfSelect; shot 05-island-shelf 1.5
q shelfCompress; shot 05b-island-shelf-compressed-toast 2.0

# Two-Column Calendar Glance (dark-to-frosted-glass fade)
q island cal; shot 06a-island-calendar 1.4

# 3-Column Multi-Widget Dashboard & Widget Customizer
q island widgets; shot 06c-island-widgets-3col 1.6
q island customize; shot 06d-island-widgets-customizer 1.4

# Specialized Droppy Widgets: Pomodoro, Storage Cleaner, Per-App Audio Mixer, AI Usage Rings
q island pomodoro; shot 06e-island-pomodoro 1.4
q island cleaner; shot 06f-island-storage-cleaner 2.0
q island mixer; shot 06g-island-audio-mixer 1.6
q island usage; shot 06h-island-ai-usage 1.4

# Notchface & Tools
q island face; shot 06b-island-notchface 1.6
q island tools; shot 06-island-tools 1.2
q island close; sleep 0.8

# Slim Volume HUD + Concave-Ear AirPods Pro Connected Battery Ring HUD (80%)
q hud volume 0.62; shot 07-hud-volume 0.5
sleep 2.4
q hudLabel bluetooth 80 "AirPods Pro"; shot 07e-hud-airpods-battery-ring 0.6
sleep 2.6

# Tri-Island Live Activities + 2-Row Hover Peek
q triActivity true; shot 07c-tri-island-three-activities 1.6
q hoverPeek true; shot 07f-tri-island-hover-peek 1.2
q hoverPeek false; q triActivity false; sleep 0.8

# Reply from the Island
q reply "Alex Rivera" "Ready for the launch review in 5 mins?"; shot 07g-reply-from-island 1.4
q island close; sleep 0.6

# Screen Recording live activity
q record; shot 07d-recording-activity 2.6
q record; sleep 1.2

# Spotlight (main's Spotlight.qml) + Menu Bar Status Menus + Control Center + Notifications
q spotlight; shot 08-spotlight 1.6
q query "te"; shot 08b-spotlight-results 1.6
q spotlight; sleep 0.6
q menu wifi; shot 18-wifi-menu 2.0
q menu wifi; q menu focus; shot 18b-focus-menu 1.2
q menu focus; sleep 0.4
q control; shot 09-control-center 1.4
q ccDetail wifi; shot 21b-cc-wifi-detail 1.2; q ccDetail ""; q ccEdit true; shot 21c-cc-edit 1.2; q ccEdit false
q control; sleep 0.5
notify-send -a "Solyra" "Welcome to Solyra" "Notifications arrive in the corner and respect Focus."; shot 10-notification 1.0
notify-send -a "Clock" "Timer" "Your 5-minute timer is done."; sleep 0.5
q notifications; shot 10b-notification-center 1.2
q notifications; sleep 0.5
sleep 4

# Snap Layouts, Revamped Terminal, Mission Control, Glass slider, Finder, Dock Stack & Magnified Hover
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
shot 15b-finder 4.5
wtype -M logo -k 2 -m logo 2>/dev/null || true
shot 22b-finder-list 1.2
up /tmp/files.log
hyprctl dispatch "hl.dsp.window.move({ workspace = 'special:minimized', follow = false })" >/dev/null
shot 15c-dock-minimized 1.5
pkill -x cupertino-files 2>/dev/null || pkill -x solyra-files 2>/dev/null || true; sleep 1
q glass 35

# Interactive Downloads 3x2 Stack + spring-damped Dock wave magnification
q downloads; shot 15d-dock-downloads-stack 1.4
q downloads; sleep 0.5
q dockHover 145; shot 15e-dock-magnified-hover 1.0
q dockHover -1; sleep 0.4
q island close; sleep 0.5

# Multi-Wallpaper 4K Unsplash Liquid Glass Optical Verification Suite
for pair in \
  "19a-glass-alpine-4k:/usr/share/backgrounds/solyra/alpine-dawn-4k.jpg" \
  "19b-glass-liquid-4k:/usr/share/backgrounds/solyra/liquid-abstract-4k.jpg" \
  "19c-glass-golden-4k:/usr/share/backgrounds/solyra/golden-valley-4k.jpg" \
  "19d-glass-light-arch-4k:/usr/share/backgrounds/solyra/light-arch-4k.jpg" \
  "19e-glass-galaxy:/usr/share/backgrounds/solyra/galaxy-1920x1080.png" \
  "19f-glass-cupertino-orb:/usr/share/backgrounds/solyra/cupertino-orb-4k.jpg" \
  "19g-glass-cupertino-lake:/usr/share/backgrounds/solyra/cupertino-lake-4k.jpg"; do
  name="${pair%%:*}"; wp="${pair#*:}"
  if [ -f "$wp" ]; then
    q wallpaper "$wp"; sleep 1.4
    q control; shot "$name" 1.2
    q control; sleep 0.4
  fi
done
# Restore default user 4K Twilight Skyline wallpaper
q wallpaper ""; sleep 1.0

# Verify solyra-app CLI + solyra-control MCP + live LRCLIB synced lyrics API
solyra-app help > /tmp/solyra-app-check.txt 2>&1; up /tmp/solyra-app-check.txt
printf '{"jsonrpc":"2.0","id":1,"method":"tools/list"}\n' | solyra-control --mcp > /tmp/solyra-control-check.txt 2>&1 || true
up /tmp/solyra-control-check.txt
curl -fsSL --max-time 6 -G -H 'User-Agent: SolyraOS/0.1' 'https://lrclib.net/api/get' \
  --data-urlencode 'artist_name=Daft Punk' --data-urlencode 'track_name=Around the World' \
  > /tmp/lrclib-live-check.txt 2>&1 || true
up /tmp/lrclib-live-check.txt

# Cupertino Session Agent, App Switcher, Screenshot Tool, OSD & Merged Terminal Agent Mode
q sessionDialog shutdown; shot 24a-logout-panel 1.0
q sessionDialog ""; sleep 0.3
q switcher true; shot 24b-switcher 1.0
q switcher false; sleep 0.3
q screenshot true; shot 24c-screenshot-tool 1.0
q screenshot false; sleep 0.3
q layout unity; shot 26a-layout-unity 1.0
q layout taskbar; shot 26b-layout-taskbar 1.0
q layout macos; sleep 0.4
SOLYRA_TERM_AGENT=1 solyra-term >/tmp/term-agent.log 2>&1 &
AGENT_PID=$!; sleep 3.5; shot 23b-terminal-agent 0.5
kill $AGENT_PID 2>/dev/null || true; sleep 0.5

# Seed timestamped .lrc for Firefox real web song playback
mkdir -p "$HOME/.local/share/solyra/lyrics"
cat > "$HOME/.local/share/solyra/lyrics/Erik Satie - Gymnopédie No. 1.lrc" <<'LRC'
[00:00.00] Lent et douloureux — Three Gymnopédies (1888)
[00:08.00] Gentle alternating seventh chords open in D major
[00:18.00] A quiet waltz melody rises over a steady left-hand pulse
[00:30.00] Soft modal harmonies drift across the piano register
[00:45.00] The theme returns, suspended and calm in half-light
[01:00.00] Fading slowly into stillness along the final cadence
LRC

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
q island queue; shot 17f-web-song-notch-queue 2.0
q island close; sleep 0.8
q control; shot 17d-web-song-control-center 1.2
q control; sleep 0.6

# All three Droppy Fullscreen Lock Screen Modes + Swipe-Up Sign-In Page on live Firefox playback
q lockMode depth; q lock; shot 16a-lock-screen-full-art-depth 2.5
q lockMode lyrics; shot 16b-lock-screen-live-lyrics 1.8
q lockMode queue; shot 16c-lock-screen-playing-next 1.8
q lockSignIn true; shot 16d-lock-screen-signin-page 1.8
q lockSignIn false; sleep 0.4

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
for f in /tmp/qs.log /tmp/hypr.log /tmp/scenario.log /tmp/term1.log /tmp/firefox.log /tmp/ipc.log; do [ -f "$f" ] && cp "$f" "/tmp/log-$(basename $f)" && up "/tmp/log-$(basename $f)"; done
touch /tmp/solyra-done
