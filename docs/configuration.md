# Configuration

Vellum reads `~/.config/vellum/config.toml` by default. It respects `$XDG_CONFIG_HOME` and,
when no user config exists, checks `$XDG_CONFIG_DIRS` (defaulting to
`/etc/xdg/vellum/config.toml`). Use `--config PATH` to select another file or `--no-config`
to load no file.

## Options

| Option | Type | Default | Description |
| --- | --- | --- | --- |
| `default_tool` | string | `"pen"` | Tool selected on startup: `pen`, `line`, `arrow`, `rectangle`, `ellipse`, `text`, `eraser`, or `select` |
| `remember_last_tool` | boolean | `true` | Keep the selected tool when drawing mode is reopened |
| `stroke_width` | number | `5.0` | Initial size for pen and shape tools; each keeps its own adjusted size during the session |
| `default_color` | string | `"#FF0000"` | Initial `#RRGGBB` color |
| `palette` | array of strings | Eight standard colors | Between 2 and 12 `#RRGGBB` colors |
| `feedback_duration_ms` | integer | `500` | How long property feedback remains visible, from `0` to `60000` milliseconds |

## Defaults

```toml
default_tool = "pen"
remember_last_tool = true
stroke_width = 5.0
default_color = "#FF0000"
feedback_duration_ms = 500
palette = [
  "#FF0000",
  "#FFFF00",
  "#00FF00",
  "#00FFFF",
  "#0000FF",
  "#FF00FF",
  "#FFFFFF",
  "#000000",
]
```
