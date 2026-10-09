# Welcome to CosmosOS

CosmosOS is a Debian-based desktop with a Mac-style look, Windows-style window snapping, and AI agents that only do what you allow. This guide covers the basics. It is also in this folder as a PDF, CosmosOS User Guide.pdf.

## Getting around

- **Dock:** sits at the bottom of the screen. Click an app to open it. A dot under an icon means the app is running. You can move the dock to the left or right in Settings > Desktop & Dock.
- **Start:** click the Cosmos mark at the left end of the dock. Start shows your pinned apps, recent files and widgets: calendar, CPU and memory, a to-do list, volume and a slideshow of your Pictures folder.
- **Search or Ask:** press Super+Space. Type to find apps, files, settings and commands, or do quick sums like 18*4. Press Tab to switch to Ask and put a question to an agent.
- **Control Centre:** click the clock at the right of the menubar. It has network, sound, Focus, Dark Mode, Lite Mode and the lock and log out buttons.
- **Dynamic Island:** the small black capsule at the top centre. It shows what agents are doing and asks you when an agent needs permission. Click it to see your clipboard history and the Shelf, where you can park files.

## Windows

- Hover over the green button in a window's title bar to choose a snap layout, such as halves or quarters.
- Drag a window to the left or right edge of the screen to snap it to that half. Snap Assist then offers your other windows for the empty half.
- Double-click a title bar to maximise the window, and again to restore it.
- Windows open where you last left them.

## Keyboard shortcuts

| Shortcut | What it does |
| --- | --- |
| Super+Space | Search or Ask |
| Super+Return | Open Terminal |
| Super+E | Open Files |
| Super+D | Show the desktop |
| Super+L | Lock the screen |
| Super+Left / Super+Right | Snap the window to the left or right half |
| Super+Down | Restore a snapped window |
| Super+F | Full screen |
| Super+M | Minimise |
| Super+Q | Close the window |
| Super+1 to Super+9 | Switch workspace |
| Super+Shift+1 to 9 | Move the window to a workspace |
| Super+T | Turn tiling on or off for this workspace |
| Super+? | Show all shortcuts |

## Agents

CosmosOS comes with one agent, **Cosmos Helper**. It runs on opencode and can only read, search, write and move files inside your Documents folder. Before it writes or moves anything, the Dynamic Island asks you to allow or deny it.

Open the Agents app to see what an agent is doing right now, everything it has done, and the permissions it has. To use Cosmos Helper you need a model provider: open Agents and choose Sign In.

## Undoing changes

CosmosOS keeps snapshots of your home folder. One is taken on first boot and another before an agent first changes your files. Open Agents > Rollback, choose a snapshot and click Restore to put your files back the way they were. Restoring does not touch your settings.

## Lite Mode

On slower machines, turn on Lite Mode in Control Centre or in Settings > Appearance. It replaces glass with flat surfaces and turns off animation and shadows.
