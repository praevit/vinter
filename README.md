<p align="center">
  <img src="site/public/logo.svg" alt="Vinter logo" width="96" height="96">
</p>

<h1 align="center">Vinter</h1>

<p align="center"><strong>Windows apps, poured onto your Mac.</strong><br>
Run Windows programs on macOS and Linux, without a Windows license or a virtual machine.</p>

<p align="center"><a href="https://getvinter.app">getvinter.app</a></p>

---

Vinter is a desktop app that sets up and runs Windows programs using
[Wine](https://www.winehq.org), the open-source compatibility layer. It
does not reimplement Windows: it downloads Wine builds, keeps each app in its
own self-contained environment (a *bottle*), installs apps from their official
installers, and lets you launch them like any other app on your computer.

> **Status: early development.** Plenty of Windows software runs well and
> plenty does not yet. There is no packaged release yet; you can build it from
> source (below).

## Features

- **Bottles**: one Windows environment per app, with its own C: drive and
  Windows version. Create, duplicate and delete them in seconds.
- **One-click recipes**: pick an app (Notepad++, 7-Zip, PuTTY, …) and Vinter
  downloads its official installer, checks it, and sets everything up. You can
  write your own recipes.
- **Install anything else**: run any Windows installer (`.exe` or `.msi`) and
  Vinter finds the programs it installed, with their icons.
- **Graphics backends**: choose Wine's built-in renderer or DXVK (Direct3D 11 over
  Vulkan) per bottle, and test what you get with one click.
- **Native launchers**: pin a program and start it from Spotlight, Launchpad or
  your Linux app menu, without opening Vinter first.
- **Logs**: every run gets its own log, with live output and a debug report you
  can copy into a bug report.
- **Wine runtimes**: downloaded from their publishers and verified by checksum
  before anything is installed.

## Platforms

| Platform | Status |
|---|---|
| macOS on Apple Silicon | Main target. Today's Wine builds for Mac need Rosetta 2, which Vinter offers to install. |
| Linux x86_64 | Supported, less tested. |
| Intel Macs | Not supported yet. |

Apple is limiting Rosetta 2 in a future macOS release; native Apple Silicon
runtimes are on the roadmap.

## Building from source

Requirements: [Rust](https://rustup.rs) (stable), Node.js 22+, and the Tauri CLI:

```sh
cargo install tauri-cli --version "^2" --locked
```

Then:

```sh
git clone https://github.com/praevit/vinter.git
cd vinter/app && npm install && cd ..
cargo tauri dev            # run the app in development mode
```

On Linux you also need the usual Tauri dependencies (WebKitGTK and friends);
see the [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/).

Links from launchers (`vinter://`) only work from an installed app bundle, not
in development mode.

### Command line

Everything the app does is also available from the `vinter` command-line tool:

```sh
cargo run -p vinter-cli -- --help
cargo run -p vinter-cli -- check                          # is this machine ready?
cargo run -p vinter-cli -- recipe list
cargo run -p vinter-cli -- recipe apply notepad-plus-plus  # one-click install, from the terminal
cargo run -p vinter-cli -- bottle list
```

### Checks

```sh
scripts/check.sh   # formatting, lints, tests and the frontend build
```

## Project layout

```
crates/vinter-core   all system logic: runtimes, bottles, recipes, graphics, launchers
crates/vinter-cli    the `vinter` command-line tool
app/src-tauri        the desktop app (Tauri v2): thin commands over vinter-core
app/src              the interface (React, TypeScript, Tailwind)
recipes/             built-in app recipes
site/                the getvinter.app website
tools/d3dprobe       tiny Windows program used by "Test graphics"
```

## Recipes

A recipe is a small TOML file describing how to set up one app: where its
installer comes from, which settings and components it needs, and which
programs to add. See the built-in ones in [`recipes/`](recipes/). Your own
go in the `recipes` folder inside Vinter's data folder, and can be checked with
`vinter recipe check <file>`.

## Contributing

Bug reports and recipes for apps you got working are very welcome:
[open an issue](https://github.com/praevit/vinter/issues). Please include the
debug report from the bottle's **Logs** tab.

## Credits and licenses

Vinter is built on [Wine](https://www.winehq.org), licensed under the LGPL.
Graphics use [DXVK](https://github.com/doitsujin/dxvk) (zlib) and MoltenVK
(Apache 2.0); some recipes use [winetricks](https://github.com/Winetricks/winetricks)
(LGPL). These are downloaded from their publishers, not bundled.

Vinter is not affiliated with Wine, Apple or Microsoft.

Made in Iceland by Prævit ehf. Contact: hello@getvinter.app
