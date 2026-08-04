//! The ported Material file-icon theme.
//!
//! `atelier` ships the VS Code Material icon theme as a JSON manifest plus a
//! folder of SVGs, resolves a path to one icon, and rasterises it. This is the
//! Rust port of that resolver. The assets ride inside the binary through
//! `rust-embed`, and GPUI's `img` element rasterises the SVG in full colour, so
//! the tree reads with the same glyphs the Swift build shows.
//!
//! The manifest is parsed once behind a `OnceLock`. Every lookup is by the
//! lowercased file or folder name, matching the manifest's own casing rule.
//! Resource paths are the public form GPUI loads through [`crate::AppAssets`],
//! for example `material-icons/icons/rust.svg`.

use std::collections::HashMap;
use std::sync::OnceLock;

use gpui::SharedString;
use rust_embed::RustEmbed;
use serde::Deserialize;

/// The embedded manifest and icon SVGs, rooted at `assets/material-icons`.
#[derive(RustEmbed)]
#[folder = "assets/material-icons"]
pub struct MaterialAssets;

/// The public resource prefix GPUI resolves back to [`MaterialAssets`].
pub const PREFIX: &str = "material-icons/";

const FILE_FALLBACK: &str = "material-icons/icons/file.svg";
const FOLDER_FALLBACK: &str = "material-icons/icons/folder.svg";
const FOLDER_OPEN_FALLBACK: &str = "material-icons/icons/folder-open.svg";

#[derive(Deserialize)]
struct RawIconDefinition {
    #[serde(rename = "iconPath")]
    icon_path: String,
}

#[derive(Deserialize, Default)]
struct RawAppearance {
    #[serde(rename = "fileExtensions")]
    file_extensions: Option<HashMap<String, String>>,
    #[serde(rename = "fileNames")]
    file_names: Option<HashMap<String, String>>,
    #[serde(rename = "folderNames")]
    folder_names: Option<HashMap<String, String>>,
    #[serde(rename = "folderNamesExpanded")]
    folder_names_expanded: Option<HashMap<String, String>>,
}

#[derive(Deserialize)]
struct RawManifest {
    #[serde(rename = "iconDefinitions")]
    icon_definitions: HashMap<String, RawIconDefinition>,
    #[serde(rename = "fileExtensions")]
    file_extensions: HashMap<String, String>,
    #[serde(rename = "fileNames")]
    file_names: HashMap<String, String>,
    #[serde(rename = "folderNames")]
    folder_names: HashMap<String, String>,
    #[serde(rename = "folderNamesExpanded")]
    folder_names_expanded: HashMap<String, String>,
    light: Option<RawAppearance>,
    file: String,
    folder: String,
    #[serde(rename = "folderExpanded")]
    folder_expanded: String,
}

/// The resolved theme: icon key to resource path, plus the association tables
/// for each appearance.
struct Theme {
    icon_paths: HashMap<String, SharedString>,
    file_extensions: HashMap<String, String>,
    file_names: HashMap<String, String>,
    folder_names: HashMap<String, String>,
    folder_names_expanded: HashMap<String, String>,
    light_file_extensions: HashMap<String, String>,
    light_file_names: HashMap<String, String>,
    light_folder_names: HashMap<String, String>,
    light_folder_names_expanded: HashMap<String, String>,
    default_file: String,
    default_folder: String,
    default_folder_expanded: String,
}

fn theme() -> Option<&'static Theme> {
    static THEME: OnceLock<Option<Theme>> = OnceLock::new();
    THEME.get_or_init(load).as_ref()
}

fn load() -> Option<Theme> {
    let file = MaterialAssets::get("material-icons.json")?;
    let manifest: RawManifest = serde_json::from_slice(&file.data).ok()?;

    // The manifest points each definition at `./../icons/name.svg`; keep the
    // slice from the last `icons/` and hang it under the public prefix.
    let icon_paths = manifest
        .icon_definitions
        .into_iter()
        .filter_map(|(key, def)| {
            let start = def.icon_path.rfind("icons/")?;
            let resource = format!("{PREFIX}{}", &def.icon_path[start..]);
            Some((key, SharedString::from(resource)))
        })
        .collect();

    let file_extensions = normalized(manifest.file_extensions);
    let file_names = normalized(manifest.file_names);
    let folder_names = normalized(manifest.folder_names);
    let folder_names_expanded = normalized(manifest.folder_names_expanded);
    let light = manifest.light.unwrap_or_default();

    Some(Theme {
        light_file_extensions: merged(&file_extensions, light.file_extensions),
        light_file_names: merged(&file_names, light.file_names),
        light_folder_names: merged(&folder_names, light.folder_names),
        light_folder_names_expanded: merged(&folder_names_expanded, light.folder_names_expanded),
        file_extensions,
        file_names,
        folder_names,
        folder_names_expanded,
        icon_paths,
        default_file: manifest.file,
        default_folder: manifest.folder,
        default_folder_expanded: manifest.folder_expanded,
    })
}

/// Lowercases every key so lookups match the manifest's own casing rule.
fn normalized(source: HashMap<String, String>) -> HashMap<String, String> {
    source
        .into_iter()
        .map(|(k, v)| (k.to_lowercase(), v))
        .collect()
}

/// The dark table overlaid with the light overrides, so a light lookup falls
/// back to the shared association when no override exists.
fn merged(base: &HashMap<String, String>, light: Option<HashMap<String, String>>) -> HashMap<String, String> {
    let mut out = base.clone();
    for (k, v) in normalized(light.unwrap_or_default()) {
        out.insert(k, v);
    }
    out
}

/// The resource path for a file's icon, chosen by exact name then by the
/// longest matching extension, matching `atelier`'s resolver.
pub fn file_icon(name: &str, light: bool) -> SharedString {
    let Some(theme) = theme() else {
        return SharedString::from(FILE_FALLBACK);
    };
    let name = name.to_lowercase();
    let names = if light { &theme.light_file_names } else { &theme.file_names };
    let extensions = if light { &theme.light_file_extensions } else { &theme.file_extensions };

    let key = names.get(&name).or_else(|| {
        // `foo.test.ts` tries `test.ts`, then `ts`.
        let parts: Vec<&str> = name.split('.').collect();
        (1..parts.len()).find_map(|i| extensions.get(&parts[i..].join(".")))
    });
    resolve(theme, key, &theme.default_file, FILE_FALLBACK)
}

/// The resource path for a folder's icon, split by open state, chosen by exact
/// name then by the appearance default.
pub fn folder_icon(name: &str, expanded: bool, light: bool) -> SharedString {
    let Some(theme) = theme() else {
        return SharedString::from(if expanded { FOLDER_OPEN_FALLBACK } else { FOLDER_FALLBACK });
    };
    let name = name.to_lowercase();
    let (names, default_key, fallback) = if expanded {
        let names = if light { &theme.light_folder_names_expanded } else { &theme.folder_names_expanded };
        (names, &theme.default_folder_expanded, FOLDER_OPEN_FALLBACK)
    } else {
        let names = if light { &theme.light_folder_names } else { &theme.folder_names };
        (names, &theme.default_folder, FOLDER_FALLBACK)
    };
    resolve(theme, names.get(&name), default_key, fallback)
}

/// Maps a resolved icon key (or the appearance default) to its resource path,
/// dropping to the static fallback when the key carries no definition.
fn resolve(theme: &Theme, key: Option<&String>, default_key: &str, fallback: &'static str) -> SharedString {
    let key = key.map(String::as_str).unwrap_or(default_key);
    theme
        .icon_paths
        .get(key)
        .or_else(|| theme.icon_paths.get(default_key))
        .cloned()
        .unwrap_or_else(|| SharedString::from(fallback))
}
