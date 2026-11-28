-- Merged Agent Mode (matching docs/ref/terminal-b_03.png):
-- Near-black #141414 canvas, title + workspace tag in the titlebar, right-aligned
-- user bubbles, "Thinking..." status, bottom rounded composer with Auto / Model /
-- Effort pickers + round stop/send button, and top-right `>_` button to switch
-- seamlessly back to the split-pane Terminal mode.
local theme = require("../theme")
local state = require("../state")
local icons = require("../icons")
local controls = require("./controls")
local sidebar = require("./sidebar")

local M = {}

local MODELS = { "Opus 5.5", "Sonnet 5.5", "Haiku 5.5" }
local EFFORTS = { "Low", "Medium", "High" }

local function cycle(list: { string }, current: string): string
	for i, item in ipairs(list) do
		if item == current then
			return list[(i % #list) + 1]
		end
	end
	return list[1]
end

local function submit()
	local text = state.agent_draft:peek()
	if not text or text == "" then
		return
	end
	local msgs = table.clone(state.agent_messages:peek())
	table.insert(msgs, { role = "user", text = text })
	state.agent_messages:set(msgs)
	state.agent_title:set(text)
	state.agent_draft:set("")
	state.agent_thinking:set(true)
	ui.invoke("agent_run", {
		prompt = text,
		model = state.agent_model:peek(),
		effort = state.agent_effort:peek(),
		auto = state.agent_auto:peek(),
	})
end

local function stop_thinking()
	state.agent_thinking:set(false)
end

-- Seed Agent Mode when launched with SOLYRA_TERM_AGENT=1 (e.g. VM screenshot)
ui.effect(function()
	if state.start_agent_mode:get() then
		state.agent_mode:set(true)
		if #state.agent_messages:peek() == 0 then
			state.agent_title:set("write a 500 word story")
			state.agent_workspace:set("work")
			state.agent_messages:set({
				{ role = "user", text = "write a 500 word story" },
			})
			state.agent_thinking:set(true)
		end
	end
end)

-- Append asynchronous replies pushed from the Rust host.
ui.effect(function()
	local reply = state.agent_reply:get()
	if reply and reply ~= "" and state.agent_thinking:peek() then
		local msgs = table.clone(state.agent_messages:peek())
		table.insert(msgs, { role = "assistant", text = reply })
		state.agent_messages:set(msgs)
		state.agent_thinking:set(false)
	end
end)

local function picker_button(label_fn: () -> string, on_click: () -> ()): Node
	return ui.button {
		px = 8,
		py = 4,
		rounded = 8,
		bg = theme.clear,
		hover = { bg = "#ffffff12" },
		on_click = on_click,
		ui.text {
			label_fn,
			text_size = 12,
			color = "#ffffff99",
		},
	}
end

function M.View(): Node
	return ui.column {
		grow = 1,
		shrink = 1,
		min_w = 0,
		h_full = true,
		bg = "#141414",
		items = "stretch",
		-- Titlebar: traffic lights + sidebar toggle space | bold title + workspace | >_ toggle
		ui.row {
			h = "$terminal.bar",
			shrink = 0,
			items = "center",
			gap = 8,
			pr = 14,
			controls.drag_space {
				w = function()
					return if state.sidebar:get() then 12 else sidebar.toggle_left() + sidebar.TOGGLE_SIZE + 8
				end,
				shrink = 0,
			},
			ui.text {
				function()
					return state.agent_title:get()
				end,
				text_size = 13,
				color = "#fffffff2",
			},
			ui.text {
				function()
					return state.agent_workspace:get()
				end,
				text_size = 13,
				color = "#ffffff59",
			},
			controls.drag_space { grow = 1, min_w = 16 },
			controls.tool(icons.terminal, function()
				state.agent_mode:set(false)
			end),
		},
		-- Conversation stream
		ui.scroll {
			grow = 1,
			min_h = 0,
			px = 96,
			py = 24,
			items = "stretch",
			ui.column {
				gap = 16,
				items = "stretch",
				ui.each(state.agent_messages, function(msg: state.AgentMessage): Node
					if msg.role == "user" then
						return ui.row {
							w_full = true,
							justify = "end",
							ui.div {
								max_w = 480,
								px = 16,
								py = 10,
								rounded = 14,
								bg = "#1f1f21",
								border = 1,
								border_color = "#ffffff16",
								ui.text {
									msg.text,
									text_size = 13.5,
									color = "#fffffff2",
								},
							},
						}
					end
					return ui.row {
						w_full = true,
						justify = "start",
						ui.div {
							max_w = 640,
							py = 4,
							ui.text {
								msg.text,
								text_size = 13.5,
								color = "#ffffffd9",
							},
						},
					}
				end),
			},
		},
		-- Bottom "Thinking..." status + rounded composer
		ui.column {
			shrink = 0,
			px = 96,
			pb = 22,
			gap = 8,
			items = "stretch",
			ui.text {
				"Thinking...",
				hidden = function()
					return not state.agent_thinking:get()
				end,
				text_size = 12,
				color = "#ffffff73",
				pl = 4,
			},
			ui.column {
				rounded = 16,
				bg = "#19191b",
				border = 1,
				border_color = "#ffffff1c",
				px = 14,
				pt = 12,
				pb = 10,
				gap = 10,
				items = "stretch",
				ui.input {
					id = "agent-composer",
					label = "Agent prompt",
					value = state.agent_draft,
					placeholder = "Add to what Claude is doing",
					placeholder_color = "#ffffff59",
					w_full = true,
					h = 24,
					px = 0,
					text_size = 13.5,
					color = "#fffffff2",
					bg = theme.clear,
					focus = { border = 0 },
					on_key = function(e)
						if e.key == "Enter" and not e.shift then
							submit()
							return true
						end
						return false
					end,
				},
				ui.row {
					items = "center",
					gap = 4,
					picker_button(function()
						return if state.agent_auto:get() then "Auto" else "Manual"
					end, function()
						state.agent_auto:set(not state.agent_auto:peek())
					end),
					picker_button(function()
						return state.agent_model:get()
					end, function()
						state.agent_model:set(cycle(MODELS, state.agent_model:peek()))
					end),
					picker_button(function()
						return state.agent_effort:get()
					end, function()
						state.agent_effort:set(cycle(EFFORTS, state.agent_effort:peek()))
					end),
					ui.div { grow = 1 },
					ui.button {
						w = 26,
						h = 26,
						rounded = 13,
						bg = "#ffffff",
						items = "center",
						justify = "center",
						on_click = function()
							if state.agent_thinking:peek() then
								stop_thinking()
							else
								submit()
							end
						end,
						ui.div {
							w = 8,
							h = 8,
							rounded = 2,
							bg = "#141414",
						},
					},
				},
			},
		},
	}
end

return M
