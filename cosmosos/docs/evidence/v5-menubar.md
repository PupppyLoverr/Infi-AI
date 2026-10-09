# v5 menubar focus-broadcast fix (#185, build b47b813)

Fresh account, 1920x1080 (no scale set — native), violet default.

1. Focus swaps: click Files -> menubar icon + bold "Files" + File/Window/Help (v5-46-menubar-files.png);
   click Settings -> swaps to "Settings" (v5-47-menubar-settings.png). PASS — before fix the bar was
   empty even with a focused window (traffic lights lit).
2. Bare-desktop click (700,700): bar keeps last app ("Files") — doesn't clear. Noted as-is.
3. Menu open: clicking a menu title drops a real 2-item menu (Keyboard Shortcuts / Search Apps and
   Files) — v5-48-menubar-menu.png. PASS.
4. 0 panics in serial.log; RAM 612 MiB used with Files+Settings+menu open (not fresh-idle; fresh-idle
   on prior builds ~553-575).
