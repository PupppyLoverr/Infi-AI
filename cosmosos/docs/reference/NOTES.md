# v5 reference notes

Owner's reference screenshots for CosmosOS v5. The build must follow these
notes. Pixel numbers are scaled to a 1920px-wide screen. Colours are the mean
RGB over a box, measured with [`measure.py`](measure.py); the output is in
[`measure.txt`](measure.txt). Box edges were picked by hand from
device-framed or low-resolution captures, so treat sizes as ±10%.

These images are references only. Nothing from them (icons, fonts,
wallpapers) ships in the image.

## ref-1: owner's sketch (`ref-1.png`)

- Three elements only: a full-width top bar ("Finder like apple macos the top bar"), a vertical dock on the left, and an island at top centre ("something like getdroppy.app").
- The top bar runs edge to edge as a thin strip. At 1920×1080 that is our 30px logical menubar (37.5px physical at 125%).
- The island sits inside the top bar at the centre and hugs it, like a notch. It is not a separate floating window below the bar.
- The dock is a tall rounded pill. It is inset slightly from the left edge, starts about 23% down the screen and spans about half the screen height, so it is vertically centred-ish rather than full height.
- Nothing else sits on the desktop: no bottom taskbar, and no bottom dock.
- Rounded screen and dock corners. The dock's radius is about half its width (a capsule).

## ref-2: Windows 11 snap layouts flyout (`ref-2.png`)

- The flyout opens directly under the maximise button, right-aligned to the window edge, overlapping the window's content.
- Size about 293×306 px. 16px outer padding, a 2-column × 3-row grid of layout tiles, 16px gutters.
- Each layout tile is about 123×78 px (screen aspect). Zones inside a tile are separate rounded rects with a 4px gap. Zone radius is about 4px; flyout radius is about 8px.
- Flyout fill is dark acrylic `#1c2735` (hue 214), the same hue as the blue content behind it (`#839dba`, hue 212). The material picks up what is underneath.
- Zones: fill `#42484f`, 1px border `#60656b`. A zone that already holds a window shows that app's icon centred in it.
- Presets: 50/50, 66/33 (two layouts), 33/33/33, 50 + 25/25, and 2×2 quarters.
- No labels or titles: just the mini screens. Hovering highlights a zone (accent in our version).

## ref-3: macOS "Search or Ask" (`ref-3.png`)

- One floating capsule, centred horizontally, its top at about 13% of the screen height (the upper third), well clear of the menubar.
- About 670px wide at 1920 (≈35% of screen width). Height ≈ 56–60px logical without the shadow. Radius = half the height.
- Fill is dark glass `#2a241f` (hue 27), taken from the warm wallpaper under it (`#c8b7a2`, hue 33). It is a tinted dark glass, not neutral grey.
- Content is a text cursor plus a "Search or Ask" hint in secondary grey on the left, and a mic glyph on the right. No border chrome, only a faint edge highlight and a soft shadow.
- When idle there are no results, no dim and no backdrop panel. The desktop stays fully visible around the pill.
- The menubar is ≈43px at 1920 (24pt logical on a 2× panel). It is translucent dark glass `#2c2521` that also takes the wallpaper hue.
- The dock is light glass `#a8a7a2`, bottom-centred (ours stays on the left per ref-1).

## ref-4: macOS Preview + context menu (`ref-4.png`)

- Menubar ≈45px at 1920, warm translucent `#362d25` (hue 28 = the wallpaper's). On the left: the Apple logo, then the app name in **bold** ("Preview"), then the menus in regular weight. On the right: battery, Wi-Fi, Control Centre glyph, and "Wed Apr 1 9:41 AM".
- The window has one unified toolbar ≈63px tall at 1920 (≈52pt logical), with traffic lights at left. The title is two lines: the file name in semibold and "Page 2 of 4" in secondary text. Toolbar icons are grouped in pill clusters, and a search field sits on the right. There is no separate title bar.
- The context menu is ≈238px wide, with a row pitch of ≈31px at 1920 (≈26px logical rows at 1920×1080). Radius ≈10.
- Context menu translucency is strong. Its fill `#9ba891` (hue 94) is plainly green from the document under it (`#415330`, hue 91). Vibrancy shows through the menu.
- Menu structure: an AI item at the top ("Ask Siri" with an icon), groups divided by thin separators, a checkmark column on the left for state, AI actions inline ("Summarize", "Show Writing Tools") with icons, and "Services" with a right chevron for the submenu. Text is regular 13pt, dark on light.
- Desktop widgets at top-left (world clocks, weather) use glass `#7d5b40` that takes the wallpaper hue, radius ≈20.
- Window corner radius ≈10–12, with a large soft shadow. The light content panes (`#f1f1f0` and `#f7f8f8`) are near-opaque, so readability wins over glass.
- Info pane: an image preview, the file name in bold, a meta line in secondary ("JPEG image · 4.5 MB"), a blue "Show More" accent link, and a row of icon + label actions at the bottom.
- Dock icons ≈49px at 1920, bottom glass dock `#628cc0` taken from the wallpaper behind. Small dots under running apps.

## ref-5: macOS Mail (awaiting image)

The Mail screenshot was not among the uploads. Until it arrives, density follows the spec: 220px sidebar, 28px rows, 13pt body, 11pt captions, and sidebar + list + detail panes where the window is wide enough. The sidebar material is lighter and translucent; the content panes are mostly opaque.

## ref-6: purple concept desktop (`ref-6.png`)

- Vivid 4-corner gradient wallpaper: violet `#9982e2` top-left (hue 254), magenta `#db74fd` top-right (hue 285), blue-violet `#767adb` bottom-left (hue 238), purple `#af75f0` bottom-right (hue 268). Saturation 0.42–0.54, value 0.86–0.99, so it is bright, not dark.
- The top row has no bar background. On the left: weather (a sun/cloud glyph, "26°C Partly Cloudy") in white regular text, ≈40px row height at 1920.
- At top centre is a light glass search capsule, ≈348×55px at 1920. Fill `#d1a8ec` is the magenta wallpaper showing through a white tint. The hint text is "Type here to search" with a magnifier glyph.
- On the right: tray glyphs, then the time in **bold** ("09:25"), the date in regular ("November 1"), and a moon (theme toggle).
- The dock is small: ≈301×57px at 1920, bottom centre, light glass `#b1b5df`, 6 icons. Ours is on the left per ref-1, but it should be just as compact.
- One hero object and lots of empty space. White text on the vivid gradient, nothing cluttered. This is the "colour energy" for the default violet wallpaper.

## ref-7: Windows 11 widgets panel (`ref-7.png`)

- A panel ≈663×939px at 1920, anchored ≈225px from the left, ≈70px from the top, nearly full height. Radius ≈8–12.
- Frosted light fill `#d1e6f0` (hue 199), the same hue as the blue wallpaper (`#83d0f7`, hue 200). The tint is visible but readable.
- Header: weather (location + "19°C Sunny") at left, three icon-only tabs centred, avatar at right. "Widgets Library" title in semibold with an expand glyph.
- 2-column masonry of cards ≈291px wide, ≈16–20px gaps. Each card has a small header (glyph + title + ⋯), card radius ≈8, and a slightly more opaque fill (`#c1cdf8`).
- Cards: a Photos 2×2 collage; Reminders with coloured left bars and circle checkboxes; a Volume Mixer of icon + slider rows; Quick Controls as round toggle buttons with labels; a Calendar month grid with an accent "Create an event" button and today circled in the accent `#3772c6`; a To Do list with star toggles and a "New" button; and a Performance Monitor of three ≈52px rings (RAM, Disk, CPU) with % centred.
- The taskbar is light glass `#c9d0ea`. Ours has no taskbar: the panel opens from the left dock instead.

## Extra: Windows 11 Start (`ref-7b-win11-start.png`)

- A centred light frosted panel: a search field across the top, a category filter row, then grouped app tiles (4 icons per group) with group captions. Avatar + name at the bottom left, power at the bottom right. We match the footer layout for Start (§2.3).

## What this means for the build

- **Glass must carry the wallpaper hue.** Every reference material (menubar, menus, pill, panels, dock) measures within ±5° of the hue behind it. Our glass check in v5-REPORT compares the panel pixels' mean hue against the wallpaper hue behind the panel.
- **Dark glass in dark mode is tinted dark, not grey** (ref-3 pill, ref-4 menubar). Light glass in light mode is white over a visible colour (ref-6 pill, ref-7 panel).
- **Content stays readable:** document and list panes are near-opaque (ref-4). Glass goes on chrome: bars, menus, panels and sidebars.
- **Sizes are larger than v4's:** a 45px menubar, 63px toolbar, 49px dock icons and 26px menu rows at 1920 all match our 125% default scale (30/52/40/21 logical × 1.25 ≈ 38/65/50/26).
- **Type:** bold app name in the menubar, a bold time with a regular date, semibold window titles with a secondary subtitle.
- **Spacing:** generous; 16px panel padding and 16–20px gaps between cards.
