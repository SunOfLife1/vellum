# Configuration

Vellum reads `~/.config/vellum/config.toml` by default. It respects `$XDG_CONFIG_HOME` and,
when no user config exists, checks `$XDG_CONFIG_DIRS` (defaulting to
`/etc/xdg/vellum/config.toml`). Use `--config PATH` to select another file or `--no-config`
to load no file.

## Options

| Option | Type | Default | Description |
| --- | --- | --- | --- |
| `default_tool` | string | `"pen"` | Tool selected on startup: `pen`, `line`, `arrow`, `triangle`, `rectangle`, `ellipse`, `text`, `eraser`, or `select` |
| `remember_last_tool` | boolean | `true` | Keep the selected tool when drawing mode is reopened |
| `stroke_width` | number | `5.0` | Initial size for pen and shape tools; each keeps its own adjusted size during the session |
| `default_color` | string | First palette color | Initial `#RRGGBB` color; must be present in `palette` |
| `palette` | array of strings | Eight standard colors | Between 2 and 12 `#RRGGBB` colors |
| `feedback_duration_ms` | integer | `500` | How long property feedback remains visible, from `0` to `60000` milliseconds |
| `clear_on_escape` | boolean | `false` | Clear annotations when Escape deactivates drawing mode |
| `default_fill_shapes` | boolean | `false` | Initially fill triangles, rectangles, and ellipses |

## Defaults

```toml
default_tool = "pen"
remember_last_tool = true
stroke_width = 5.0
default_color = "#E84046"
feedback_duration_ms = 500
clear_on_escape = false
default_fill_shapes = false
palette = [
  "#E84046",
  "#EC8948",
  "#EED049",
  "#3ED73C",
  "#0283FC",
  "#7C57EB",
  "#FFFFFF",
  "#000000",
]
```
