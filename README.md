# Vellum

`vellum` is a small native Wayland overlay for drawing directly over the live desktop, tested on niri.

https://github.com/user-attachments/assets/f8171063-16a8-497f-ba20-9e11bc50727e

Vellum began as a fork of [Chameleos](https://github.com/Treeniks/chameleos) by Thomas Lindae.

## Usage

Start the overlay once, then toggle drawing from a compositor shortcut:

```sh
vellum &
vellum toggle
```

For example, in niri:

```kdl
Mod+A { spawn "vellum" "toggle"; }
```

## Controls

| Input | Action |
| --- | --- |
| Left drag | Draw or manipulate the selection |
| Hold right click, move, and release | Choose a tool or color |
| Right click without moving | Keep the wheel open for click selection |
| Middle drag | Erase annotations |
| Pen barrel button while hovering | Open the wheel |
| Pen barrel button while drawing | Temporarily erase |
| Pen eraser | Erase annotations |
| Mouse wheel | Change stroke width or text size |
| `Ctrl` + wheel | Change opacity |
| `Shift` + wheel | Change roundness |
| `Ctrl` + click in Select | Add or remove an annotation from the selection |
| Double-click selected text | Edit it |
| `Ctrl+Z` | Undo |
| `Ctrl+Shift+Z` or `Ctrl+Y` | Redo |
| `Ctrl+A` | Select all annotations |
| `Backspace` or `Delete` | Delete selected annotations |
| `Escape` | Cancel, clear the selection, or leave drawing mode |

When the wheel is left open, move without holding a button and left click a tool or color.
Left clicking the center, right clicking again, or pressing `Escape` dismisses it.

Drag selection handles to reshape supported elements. While drawing, `Shift` constrains geometry
and `Alt` draws rectangles and ellipses from their center.

Run `vellum --help` for startup options and commands. Packaged releases also provide
`man vellum` with the same command-line reference.

## Configuration

See [Configuration](docs/configuration.md) for the available options, defaults, and file lookup order.

## Home Manager

```nix
{
  imports = [inputs.vellum.homeModules.default];
  services.vellum.enable = true;
}
```

## Building from source

### Arch

```sh
sudo pacman -S --needed base-devel git rust wayland libxkbcommon libglvnd vulkan-icd-loader
```

### Fedora

```sh
sudo dnf install cargo gcc git pkgconf-pkg-config wayland-devel libxkbcommon-devel libglvnd-egl vulkan-loader
```

Then build:

```sh
git clone https://github.com/greyxp1/vellum
cd vellum
cargo build --release --locked
```

With Nix, skip the system dependencies and run `nix build`, or use `nix develop` for a
development shell.
