-- macOS traffic lights for Solyra (Linux): close, minimise and zoom, drawn by
-- the app in its titlebar's top-left corner, where the sidebar header and the
-- tab strip already leave them room (sidebar.toggle_left). The window's own
-- Linux buttons are off (Titlebar::Hidden). As in macOS, hovering the group
-- shows each one's glyph; while the window is inactive they would grey, but
-- Solyra's apps keep their colours as its other zgx apps do.
local theme = require("../theme")

local M = {}

M.SIZE = 12
M.GAP = 8
M.LEFT = 16
-- Their right edge, for what sits after them.
M.WIDTH = M.SIZE * 3 + M.GAP * 2

local hovering = ui.state("lights_hover", false)

local function light(id: string, color: string, glyph: string, action: string): Node
	return ui.div {
		id = "light:" .. id,
		w = M.SIZE,
		h = M.SIZE,
		rounded = "$radius.full",
		bg = color,
		border = 0.5,
		border_color = "#00000033",
		items = "center",
		justify = "center",
		on_click = function()
			ui.invoke(action)
		end,
		ui.text {
			glyph,
			text_size = 9,
			weight = 700,
			color = "#00000099",
			opacity = function()
				return if hovering:get() then 1 else 0
			end,
		},
	}
end

function M.Lights(): Node
	return ui.row {
		id = "traffic_lights",
		absolute = true,
		left = M.LEFT,
		top = (theme.titlebar - M.SIZE) / 2,
		gap = M.GAP,
		on_hover = function(inside)
			hovering:set(inside)
		end,
		light("Close", "#ff5f57", "×", "close_window"),
		light("Minimise", "#febc2e", "−", "minimize_window"),
		light("Zoom", "#28c840", "+", "zoom_window"),
	}
end

return M
