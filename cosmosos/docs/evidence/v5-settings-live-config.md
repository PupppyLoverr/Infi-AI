# Settings live-config re-read (branch devin/1791558049-settings-live-config)

@5d45f02 + merge w/ main (2209e97). Fresh image + fresh account, 125%.

Check: Settings > Appearance open the whole time; toggle Lite mode in
Control Centre, then move the pointer over the Settings window.

- CC Lite ON → hover → Lite mode switch shows ON (purple). PASS — v5-livecfg-on.png
- CC Lite OFF → hover → Lite mode switch shows OFF (grey). PASS — v5-livecfg-off.png
- config.json "lite_mode": false after the OFF flip; glass/menubar tint restored.
- 0 panics.

Previous defect (long-lived cosmos-settings never refreshing external config
changes) is fixed. Recommend merge.
