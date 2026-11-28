# Cupertino Files

Vendored from [orafaelfragoso/cupertino](https://github.com/orafaelfragoso/cupertino)
at commit `3222b6bac8030aa3a0129af53e95f6894b27c0ee5`.

The workspace contains the normal dependency closure of
`cupertino-files --features app` for Linux:

- `cupertino-animation`
- `cupertino-app-protocols`
- `cupertino-apps`
- `cupertino-assets`
- `cupertino-defaults`
- `cupertino-diagnostic-events`
- `cupertino-files`
- `cupertino-geometry`
- `cupertino-gpu`
- `cupertino-icons`
- `cupertino-keyboard`
- `cupertino-kit`
- `cupertino-menu-model`
- `cupertino-raster-math`
- `cupertino-theme`
- `cupertino-trash`
- `cupertino-ui-controls`
- `cupertino-ui-core`
- `cupertino-ui-files`
- `cupertino-ui-render`
- `cupertino-ui-text`
- `cupertino-ui-wayland`
- `cupertino-wayland-client`

Dev-dependencies are excluded. `cupertino-ui-test` is test-only, and
`cupertino-gpu-shaders` is not in the normal app-feature dependency tree.
The crates.io `wayland-client` dependency is not a Cupertino workspace crate.

The upstream `LICENSE` and additional license texts are retained alongside
the source. `cupertino-wayland-client` and its normal dependency closure are
MPL-2.0; the application is GPL-3.0-or-later.

Solyra keeps the upstream runtime assets and theme data, and adds a first-load
asset pack for the Geist UI font under `data/assets/solyra`. The window-effects
protocol is optional; without it, CupertinoKit paints its material fills in
the window itself.
