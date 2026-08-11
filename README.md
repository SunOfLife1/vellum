# Vellum

`vellum` is a small native Wayland overlay for drawing directly over the live desktop. It is
designed for niri and works with niri's built-in screenshot tool.

https://github.com/user-attachments/assets/f8171063-16a8-497f-ba20-9e11bc50727e

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
| Hold right click | Open the tool wheel |
| Release right click in the center | Open the color wheel |
| Middle drag | Temporarily erase annotations |
| Mouse wheel | Change stroke width or text size |
| `Ctrl` + wheel | Change opacity |
| `Shift` + wheel | Change roundness |
| `Ctrl` + click in Select | Add or remove an annotation from the selection |
| Double-click selected text | Edit it |
| `Ctrl+Z` / `Ctrl+Shift+Z` | Undo / redo |
| `Escape` | Cancel, clear the selection, or leave drawing mode |

Drag selection handles to reshape supported elements. While drawing, `Shift` constrains geometry
and `Alt` draws rectangles and ellipses from their center.

Run `vellum --help` for startup options and socket commands.

## Building from source

Arch:

```sh
sudo pacman -S --needed base-devel git rust wayland libxkbcommon libglvnd vulkan-icd-loader

git clone https://github.com/greyxp1/vellum
cd vellum
cargo build --release --locked
```

Nix: `nix build` (or `nix develop` for a development shell).

## Home Manager

```nix
{
  imports = [inputs.vellum.homeModules.default];
  services.vellum.enable = true;
}
```

Vellum began as a fork of [Chameleos](https://github.com/Treeniks/chameleos) by Thomas Lindae.
