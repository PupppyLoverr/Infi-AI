-- Solyra on Hyprland 0.57 (Lua config): macOS-like floating windows, one
-- corner radius, soft two-layer shadows, Liquid Glass blur behind the shell
-- and Apple-ish spring motion. The shell itself is Quickshell (QML).

local home = os.getenv("HOME") or ""

hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })

-- A real arrow cursor (Adwaita), not Hyprland's fallback.
hl.env("XCURSOR_THEME", "Adwaita")
hl.env("XCURSOR_SIZE", "24")
hl.env("HYPRCURSOR_SIZE", "24")
hl.env("QT_QPA_PLATFORM", "wayland")
hl.env("ELECTRON_OZONE_PLATFORM_HINT", "auto")
hl.env("NIXOS_OZONE_WL", "1")
hl.env("MOZ_ENABLE_WAYLAND", "1")

hl.on("hyprland.start", function()
    hl.exec_cmd("hyprctl setcursor Adwaita 24")
    hl.exec_cmd("quickshell > /tmp/qs.log 2>&1")
end)

hl.config({
    general = {
        gaps_in = 6,
        gaps_out = 8,
        border_size = 1,
        -- No visible window frame (macOS 26): a hairline only to separate
        -- dark windows from dark backdrops.
        col = {
            active_border = "rgba(ffffff1a)",
            inactive_border = "rgba(ffffff0d)",
        },
        layout = "dwindle",
    },
    decoration = {
        rounding = 24,          -- macOS 26/27's larger window corners
        rounding_power = 2,
        active_opacity = 1.0,
        inactive_opacity = 1.0,
        dim_inactive = true,
        dim_strength = 0.06,
        shadow = {
            enabled = true,
            range = 40,
            render_power = 3,
            offset = "0 14",
            color = 0x55000000,
            color_inactive = 0x2E000000,
        },
        blur = {
            enabled = true,
            size = 12,
            passes = 3,
            noise = 0.01,
            vibrancy = 0.20,          -- subtle colour through clean frosted glass
            vibrancy_darkness = 0.14,
            popups = true,
            popups_ignorealpha = 0.08, -- menus and tooltips: blur the card, not its corners
            -- Subtle Liquid Glass: gentle frosted optical edge without fish-eye distortion
            variant = 8,
            acrylic = {
                refraction = 6,      -- minimal gentle edge lensing (no heavy warp)
                bulb = 6,            -- narrow subtle rim, px
                clarity = 0.84,      -- clean frosted backdrop
                aberration = 0.0,    -- no chromatic fringe
                tint = 0x05F4F6FA,
            },
        },
    },
    animations = { enabled = true },
    -- Budget: no Xwayland by default (about 58 MB); our apps, Firefox and
    -- most of the desktop are native Wayland. Set true in hypr-local.lua
    -- for X11-only apps.
    xwayland = { enabled = false },
    dwindle = { preserve_split = true },
    misc = {
        force_default_wallpaper = 0,
        disable_hyprland_logo = true,
        disable_splash_rendering = true,
    },
    input = {
        kb_layout = "us",
        follow_mouse = 1,
        natural_scroll = true,
        touchpad = { natural_scroll = true },
    },
})

-- Apple-like motion: springs for windows and layers, a smooth workspace slide.
hl.curve("smooth", { type = "bezier", points = { {0.25, 0.1}, {0.25, 1} } })
hl.curve("spring", { type = "spring", mass = 1, stiffness = 260, damping = 26 })
hl.animation({ leaf = "windowsIn", enabled = true, speed = 4, spring = "spring", style = "popin 88%" })
hl.animation({ leaf = "windowsOut", enabled = true, speed = 3, bezier = "smooth", style = "popin 92%" })
hl.animation({ leaf = "windowsMove", enabled = true, speed = 4, spring = "spring" })
hl.animation({ leaf = "fade", enabled = true, speed = 3, bezier = "smooth" })
hl.animation({ leaf = "layersIn", enabled = true, speed = 3, spring = "spring", style = "popin 94%" })
hl.animation({ leaf = "layersOut", enabled = true, speed = 2, bezier = "smooth", style = "fade" })
hl.animation({ leaf = "workspaces", enabled = true, speed = 5, bezier = "smooth", style = "slide" })
hl.animation({ leaf = "specialWorkspace", enabled = true, speed = 4, spring = "spring", style = "slidevert" })

-- Glass behind the shell's translucent surfaces: blur only where the card
-- itself is drawn. ignore_alpha sits above the soft drop shadows' opacity and
-- below the glass bodies' (Glass.qml keeps every body above it), so a card
-- gets rounded blur and its shadow never turns into a blurred box.
hl.layer_rule({
    name = "solyra-glass",
    match = { namespace = "^solyra-(bar|dock|glass|controlcenter|notifications|notifcenter|snap|overview|island|widgets|basket|session-agent|switcher|screenshot|osd)$" },
    blur = true,
    ignore_alpha = 0.08,
})
hl.layer_rule({
    name = "solyra-pop",
    match = { namespace = "^solyra-(glass|snap)$" },
    animation = "popin 92%",
})

-- Our translucent apps get the same frosted glass.
-- Window arrangement. Default: macOS. Windows float, centred, where they
-- open. With Auto-arrange on (Control Center, the Window menu or Spotlight;
-- saved in ~/.config/solyra/look.json) they tile instead (dwindle, with
-- gaps), as Windows 11 snap groups do. The shell reloads Hyprland when it
-- changes and re-arranges the windows already open.
local function auto_arrange()
    local f = io.open(home .. "/.config/solyra/look.json", "r")
    if not f then return false end
    local json = f:read("a") or ""
    f:close()
    return json:find('"autoArrange"%s*:%s*true') ~= nil
end
if not auto_arrange() then
    hl.window_rule({
        name = "solyra-float-all",
        match = { class = ".*" },
        float = true,
        center = true,
    })
end

-- Our document apps open at a comfortable size when floating.
hl.window_rule({
    name = "solyra-apps-float",
    match = { class = "^(solyra-files|solyra-settings|solyra-term)$" },
    size = "monitor_w*0.70 monitor_h*0.74",
    center = true,
})
hl.window_rule({
    name = "solyra-apps-glass",
    match = { class = "^(solyra-term|solyra-files|solyra-settings)$" },
    opacity = "0.999 override",
})

local mod = "SUPER"
hl.bind(mod .. " + SPACE", hl.dsp.global("solyra:spotlight"))
hl.bind(mod .. " + C", hl.dsp.global("solyra:control"))
hl.bind(mod .. " + L", hl.dsp.global("solyra:lock"))
hl.bind(mod .. " + Z", hl.dsp.global("solyra:snap"))
hl.bind(mod .. " + N", hl.dsp.global("solyra:island"))
hl.bind(mod .. " + up", hl.dsp.global("solyra:overview"))
hl.bind("F3", hl.dsp.global("solyra:overview"))
hl.bind(mod .. " + RETURN", hl.dsp.exec_cmd("solyra-term"))
hl.bind(mod .. " + E", hl.dsp.exec_cmd("solyra-files"))
hl.bind(mod .. " + Q", hl.dsp.window.close())
hl.bind(mod .. " + F", hl.dsp.window.fullscreen())
hl.bind(mod .. " + V", hl.dsp.window.float({ action = "toggle" }))
-- macOS-style: minimise to the dock (it shows minimised windows), screenshots
-- (⇧3 whole screen, ⇧4 an area, both to ~/Pictures and the clipboard),
-- ⇧5 starts or stops a screen recording (a live activity in the island),
-- Super+, opens the Notification Center.
hl.bind(mod .. " + M", hl.dsp.window.move({ workspace = "special:minimized", follow = false }))
hl.bind(mod .. " + SHIFT + 3", hl.dsp.exec_cmd("sh -c 'mkdir -p \"$HOME/Pictures\"; f=\"$HOME/Pictures/Screenshot $(date +%F\\ at\\ %H.%M.%S).png\"; grim \"$f\" && wl-copy < \"$f\" && notify-send -a Screenshot \"Screenshot saved\" \"$f\"'"))
hl.bind(mod .. " + SHIFT + 4", hl.dsp.exec_cmd("sh -c 'mkdir -p \"$HOME/Pictures\"; f=\"$HOME/Pictures/Screenshot $(date +%F\\ at\\ %H.%M.%S).png\"; grim -g \"$(slurp)\" \"$f\" && wl-copy < \"$f\" && notify-send -a Screenshot \"Screenshot saved\" \"$f\"'"))
hl.bind(mod .. " + SHIFT + 5", hl.dsp.exec_cmd("qs ipc call shell record"))
hl.bind(mod .. " + COMMA", hl.dsp.exec_cmd("qs ipc call shell notifications"))
for i = 1, 9 do
    hl.bind(mod .. " + " .. i, hl.dsp.focus({ workspace = i }))
    hl.bind(mod .. " + SHIFT + " .. i, hl.dsp.window.move({ workspace = i }))
end
hl.bind(mod .. " + TAB", hl.dsp.focus({ workspace = "e+1" }))
hl.bind(mod .. " + SHIFT + TAB", hl.dsp.focus({ workspace = "e-1" }))
hl.bind(mod .. " + mouse:272", hl.dsp.window.drag(), { mouse = true })
hl.bind(mod .. " + mouse:273", hl.dsp.window.resize(), { mouse = true })
hl.bind("XF86AudioRaiseVolume", hl.dsp.exec_cmd("wpctl set-volume -l 1 @DEFAULT_AUDIO_SINK@ 5%+"), { locked = true, repeating = true })
hl.bind("XF86AudioLowerVolume", hl.dsp.exec_cmd("wpctl set-volume @DEFAULT_AUDIO_SINK@ 5%-"), { locked = true, repeating = true })
hl.bind("XF86AudioMute", hl.dsp.exec_cmd("wpctl set-mute @DEFAULT_AUDIO_SINK@ toggle"), { locked = true })
hl.bind("XF86MonBrightnessUp", hl.dsp.exec_cmd("brightnessctl -q set 5%+ && qs ipc call shell hud brightness $(brightnessctl -m | cut -d, -f4 | tr -d %)"), { locked = true, repeating = true })
hl.bind("XF86MonBrightnessDown", hl.dsp.exec_cmd("brightnessctl -q set 5%- && qs ipc call shell hud brightness $(brightnessctl -m | cut -d, -f4 | tr -d %)"), { locked = true, repeating = true })
hl.bind("XF86AudioPlay", hl.dsp.exec_cmd("playerctl play-pause"), { locked = true })
hl.bind("XF86AudioNext", hl.dsp.exec_cmd("playerctl next"), { locked = true })
hl.bind("XF86AudioPrev", hl.dsp.exec_cmd("playerctl previous"), { locked = true })

-- Snap by dragging (Windows 11 Aero Snap): drag a floating window by its
-- title bar or with Super and let go at a screen edge. Left or right edge:
-- that half; a corner: that quarter; the top edge: fill the screen. The
-- zones leave the menu bar, the dock and the usual gaps clear, as the snap
-- layouts (Super+Z) do. A window that did not move (a click) never snaps.
local SNAP_EDGE, BAR, DOCK, GAP = 6, 28, 66, 8
local function xy(v)
    if type(v) == "table" then return v.x or v[1] or 0, v.y or v[2] or 0 end
    return 0, 0
end
local snap_from = nil
local function snap_mark()
    local w = hl.get_active_window()
    if w then
        local x, y = xy(w.at)
        snap_from = w.address .. ":" .. x .. ":" .. y
    else
        snap_from = nil
    end
end
local function snap_release()
    local w = hl.get_active_window()
    if not w or not w.floating or (w.fullscreen or 0) ~= 0 then return end
    local x, y = xy(w.at)
    if snap_from == w.address .. ":" .. x .. ":" .. y then return end
    local c, m = hl.get_cursor_pos(), hl.get_monitor_at_cursor()
    if not c or not m then return end
    local mx, my = xy(m.position)
    local cx, cy = c.x - mx, c.y - my
    local W, H = m.width / (m.scale or 1), m.height / (m.scale or 1)
    local zone
    local top_band, bottom_band = cy < H * 0.25, cy > H * 0.75
    if cx <= SNAP_EDGE then
        zone = top_band and { 0, 0, .5, .5 } or bottom_band and { 0, .5, .5, .5 } or { 0, 0, .5, 1 }
    elseif cx >= W - 1 - SNAP_EDGE then
        zone = top_band and { .5, 0, .5, .5 } or bottom_band and { .5, .5, .5, .5 } or { .5, 0, .5, 1 }
    elseif cy <= SNAP_EDGE then
        zone = { 0, 0, 1, 1 }
    end
    if not zone then return end
    local top, bottom = BAR + GAP, DOCK + GAP
    local AW, AH = W - 2 * GAP, H - top - bottom
    local zx = math.floor(GAP + zone[1] * AW + (zone[1] > 0 and GAP / 2 or 0) + 0.5)
    local zy = math.floor(top + zone[2] * AH + (zone[2] > 0 and GAP / 2 or 0) + 0.5)
    local zw = math.floor(zone[3] * AW - (zone[1] > 0 and GAP / 2 or 0) - (zone[1] + zone[3] < 0.999 and GAP / 2 or 0) + 0.5)
    local zh = math.floor(zone[4] * AH - (zone[2] > 0 and GAP / 2 or 0) - (zone[2] + zone[4] < 0.999 and GAP / 2 or 0) + 0.5)
    hl.dispatch(hl.dsp.window.resize({ x = zw, y = zh }))
    hl.dispatch(hl.dsp.window.move({ x = mx + zx, y = my + zy }))
end
-- Non-consuming and modifier-blind: clicks still reach apps, and a Super-drag
-- (the bind above) is seen too.
pcall(hl.bind, "mouse:272", snap_mark, { non_consuming = true, ignore_mods = true })
pcall(hl.bind, "mouse:272", snap_release, { non_consuming = true, ignore_mods = true, release = true })

-- Machine-local additions (the CI test scenario, user tweaks).
local localcfg = home .. "/.config/solyra/hypr-local.lua"
local f = io.open(localcfg, "r")
if f then
    f:close()
    pcall(dofile, localcfg)
end
