-- Solyra on Hyprland 0.57 (Lua config): macOS-like floating windows, one
-- corner radius, soft two-layer shadows, Liquid Glass blur behind the shell
-- and Apple-ish spring motion. The shell itself is Quickshell (QML).

local home = os.getenv("HOME") or ""

hl.monitor({ output = "", mode = "preferred", position = "auto", scale = 1 })

hl.env("XCURSOR_SIZE", "24")
hl.env("HYPRCURSOR_SIZE", "24")
hl.env("QT_QPA_PLATFORM", "wayland")

hl.on("hyprland.start", function()
    hl.exec_cmd("swaybg -i " .. home .. "/.config/solyra/wallpaper.png -m fill")
    hl.exec_cmd("quickshell > /tmp/qs.log 2>&1")
end)

hl.config({
    general = {
        gaps_in = 6,
        gaps_out = 8,
        border_size = 1,
        col = {
            active_border = "rgba(ffffff38)",
            inactive_border = "rgba(ffffff14)",
        },
        layout = "dwindle",
    },
    decoration = {
        rounding = 16,
        rounding_power = 2,
        active_opacity = 1.0,
        inactive_opacity = 1.0,
        dim_inactive = true,
        dim_strength = 0.06,
        shadow = {
            enabled = true,
            range = 28,
            render_power = 3,
            offset = "0 10",
            color = 0x59000000,
            color_inactive = 0x33000000,
        },
        blur = {
            enabled = true,
            size = 9,
            passes = 3,
            noise = 0.012,
            vibrancy = 0.22,
            popups = true,
        },
    },
    animations = { enabled = true },
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

-- Glass behind the shell's translucent surfaces: blur only where pixels are
-- drawn (ignore_alpha), so rounded cards get rounded blur.
hl.layer_rule({
    name = "solyra-glass",
    match = { namespace = "^solyra-(bar|dock|spotlight|control|notifications|snap|overview|island)$" },
    blur = true,
    ignore_alpha = 0.02,
})
hl.layer_rule({
    name = "solyra-pop",
    match = { namespace = "^solyra-(spotlight|control|snap)$" },
    animation = "popin 92%",
})

-- Our translucent apps get the same frosted glass.
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

-- Machine-local additions (the CI test scenario, user tweaks).
local localcfg = home .. "/.config/solyra/hypr-local.lua"
local f = io.open(localcfg, "r")
if f then
    f:close()
    pcall(dofile, localcfg)
end
