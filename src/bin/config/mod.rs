use super::cli::{Backend, Cli};
use super::{Rgb, state};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::time::Duration;

const CONFIG_FILE: &str = "vellum/config.toml";
const DEFAULT_PALETTE: [&str; 8] = [
    "#E84046", "#EC8948", "#EED049", "#3ED73C", "#0283FC", "#7C57EB", "#FFFFFF", "#000000",
];

#[derive(Debug, Default, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct FileConfig {
    default_tool: Option<String>,
    remember_last_tool: Option<bool>,
    stroke_width: Option<f32>,
    default_color: Option<String>,
    palette: Option<Vec<String>>,
    feedback_duration_ms: Option<u64>,
    clear_on_escape: Option<bool>,
    default_fill_shapes: Option<bool>,
}

pub(super) struct Settings {
    pub(super) stroke_width: f32,
    pub(super) stroke_color: Rgb,
    pub(super) force_backend: Option<Backend>,
    pub(super) default_tool: state::Tool,
    pub(super) remember_last_tool: bool,
    pub(super) palette: Vec<Rgb>,
    pub(super) feedback_duration: Duration,
    pub(super) clear_on_escape: bool,
    pub(super) default_fill_shapes: bool,
}

impl Settings {
    pub(super) fn load(cli: Cli) -> Result<Self, String> {
        let file = if cli.no_config {
            FileConfig::default()
        } else if let Some(path) = &cli.config {
            read_config(path)?
        } else {
            read_first_config(default_config_paths())?
        };

        let stroke_width = file.stroke_width.unwrap_or(5.0);
        if !valid_width(stroke_width) {
            return Err("stroke_width must be a positive finite number".into());
        }

        let default_tool = file
            .default_tool
            .unwrap_or_else(|| "pen".into())
            .to_ascii_lowercase()
            .parse()?;

        let palette_text = file
            .palette
            .unwrap_or_else(|| DEFAULT_PALETTE.iter().map(ToString::to_string).collect());
        if !(2..=12).contains(&palette_text.len()) {
            return Err("palette must contain between 2 and 12 colors".into());
        }
        let palette = palette_text
            .iter()
            .enumerate()
            .map(|(index, color)| parse_named_color(&format!("palette[{index}]"), color))
            .collect::<Result<Vec<_>, _>>()?;
        let stroke_color = match file.default_color {
            Some(color) => {
                let color = parse_named_color("default_color", &color)?;
                if !palette.contains(&color) {
                    return Err("default_color must be present in palette".into());
                }
                color
            }
            None => palette[0],
        };

        let feedback_duration_ms = file.feedback_duration_ms.unwrap_or(500);
        if feedback_duration_ms > 60_000 {
            return Err("feedback_duration_ms must not exceed 60000".into());
        }

        Ok(Self {
            stroke_width,
            stroke_color,
            force_backend: cli.force_backend,
            default_tool,
            remember_last_tool: file.remember_last_tool.unwrap_or(true),
            palette,
            feedback_duration: Duration::from_millis(feedback_duration_ms),
            clear_on_escape: file.clear_on_escape.unwrap_or(false),
            default_fill_shapes: file.default_fill_shapes.unwrap_or(false),
        })
    }
}

fn parse_named_color(name: &str, value: &str) -> Result<Rgb, String> {
    parse_color(value).map_err(|error| format!("invalid {name} {value:?}: {error}"))
}

fn valid_width(width: f32) -> bool {
    width.is_finite() && width > 0.0
}

fn parse_color(value: &str) -> Result<Rgb, &'static str> {
    let hex = value.strip_prefix('#').ok_or("color must start with #")?;
    if hex.len() != 6 {
        return Err("color must be #RRGGBB");
    }
    let value = u32::from_str_radix(hex, 16).map_err(|_| "color contains a non-hex digit")?;
    Ok([
        ((value >> 16) & 0xff) as f32 / 255.0,
        ((value >> 8) & 0xff) as f32 / 255.0,
        (value & 0xff) as f32 / 255.0,
    ])
}

fn read_config(path: &Path) -> Result<FileConfig, String> {
    let contents = std::fs::read_to_string(path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    parse_config(path, &contents)
}

fn read_optional_config(path: &Path) -> Result<Option<FileConfig>, String> {
    match std::fs::read_to_string(path) {
        Ok(contents) => parse_config(path, &contents).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("could not read {}: {error}", path.display())),
    }
}

fn parse_config(path: &Path, contents: &str) -> Result<FileConfig, String> {
    toml::from_str(contents).map_err(|error| format!("invalid {}: {error}", path.display()))
}

fn read_first_config(paths: impl IntoIterator<Item = PathBuf>) -> Result<FileConfig, String> {
    for path in paths {
        if let Some(config) = read_optional_config(&path)? {
            return Ok(config);
        }
    }
    Ok(FileConfig::default())
}

fn default_config_paths() -> Vec<PathBuf> {
    config_paths(
        std::env::var_os("XDG_CONFIG_HOME"),
        std::env::var_os("HOME"),
        std::env::var_os("XDG_CONFIG_DIRS"),
    )
}

fn config_paths(
    xdg_config_home: Option<OsString>,
    home: Option<OsString>,
    xdg_config_dirs: Option<OsString>,
) -> Vec<PathBuf> {
    let user = absolute_path(xdg_config_home)
        .or_else(|| absolute_path(home).map(|path| path.join(".config")));
    let mut paths: Vec<_> = user
        .into_iter()
        .map(|path| path.join(CONFIG_FILE))
        .collect();

    match xdg_config_dirs.filter(|value| !value.is_empty()) {
        Some(dirs) => paths.extend(
            std::env::split_paths(&dirs)
                .filter(|path| path.is_absolute())
                .map(|path| path.join(CONFIG_FILE)),
        ),
        None => paths.push(PathBuf::from("/etc/xdg").join(CONFIG_FILE)),
    }
    paths
}

fn absolute_path(value: Option<OsString>) -> Option<PathBuf> {
    value.map(PathBuf::from).filter(|path| path.is_absolute())
}
