//! Atelier GPUI feasibility POC.
//!
//! ```text
//! artifex            Phase 1..3 application shell
//! artifex gate1      Phase 0 gate 1: Vietnamese IME input
//! artifex gate2      Phase 0 gate 2: embedded web preview (wry)
//! artifex gate3      Phase 0 gate 3: zsh terminal (alacritty_terminal)
//! ```

mod app;
mod gates;
mod services;
mod terminal;
#[cfg(test)]
mod tests;
mod theme;

use std::borrow::Cow;

use gpui::{
    AnyView, App, AppContext as _, AssetSource, Bounds, Pixels, Result, SharedString, Size,
    TitlebarOptions, WindowBounds, WindowOptions, point, px, size,
};
use gpui_component::{Root, Theme};
use gpui_component_assets::Assets;

use crate::services::material_icons::{MaterialAssets, PREFIX};

/// The window's single asset source. GPUI resolves every embedded resource
/// through one source, so the ported Material icons and the gpui-component icon
/// set are served side by side: a `material-icons/` path hits the ported theme,
/// everything else falls through to the component assets.
struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> Result<Option<Cow<'static, [u8]>>> {
        if let Some(rest) = path.strip_prefix(PREFIX) {
            return Ok(MaterialAssets::get(rest).map(|f| f.data));
        }
        Assets.load(path)
    }

    fn list(&self, path: &str) -> Result<Vec<SharedString>> {
        let mut items = Assets.list(path)?;
        items.extend(
            MaterialAssets::iter()
                .map(|p| SharedString::from(format!("{PREFIX}{p}")))
                .filter(|p| p.starts_with(path)),
        );
        Ok(items)
    }
}

/// Margin kept between the window and the edge of the usable screen area.
const SCREEN_MARGIN: f32 = 24.;

/// Centres the window inside the primary display's usable area, shrinking it
/// when the requested size does not fit.
///
/// `WindowBounds::centered` centres against the display's full bounds and never
/// clamps the size, so on a multi-display arrangement, or on a display smaller
/// than the request, part of the window lands outside the visible area.
fn centered_in_visible_area(requested: Size<Pixels>, cx: &App) -> WindowBounds {
    let Some(display) = cx.primary_display() else {
        return WindowBounds::centered(requested, cx);
    };
    let visible = display.visible_bounds();

    let margin = SCREEN_MARGIN * 2.;
    let available_width = (f32::from(visible.size.width) - margin).max(0.);
    let available_height = (f32::from(visible.size.height) - margin).max(0.);
    let width = f32::from(requested.width).min(available_width);
    let height = f32::from(requested.height).min(available_height);

    let origin = point(
        px(f32::from(visible.origin.x) + (f32::from(visible.size.width) - width) * 0.5),
        px(f32::from(visible.origin.y) + (f32::from(visible.size.height) - height) * 0.5),
    );

    WindowBounds::Windowed(Bounds {
        origin,
        size: size(px(width), px(height)),
    })
}

fn main() {
    let mode = std::env::args().nth(1).unwrap_or_default();
    let application = gpui_platform::application().with_assets(AppAssets);

    application.run(move |cx: &mut App| {
        gpui_component::init(cx);

        let options = WindowOptions {
            window_bounds: Some(centered_in_visible_area(size(px(1440.), px(900.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("Artifex".into()),
                appears_transparent: true,
                // Centred against the unified 40-point toolbar row: the
                // lights are ~12 points tall, so 14 puts their middle at 20.
                traffic_light_position: Some(point(px(12.), px(14.))),
            }),
            window_min_size: Some(size(px(760.), px(512.))),
            // The chrome draws its own title bar band, so it owns dragging and
            // the double-click zoom that AppKit would otherwise provide.
            app_owns_titlebar_drag: true,
            ..Default::default()
        };

        cx.spawn(async move |cx| {
            let opened = cx.open_window(options, |window, cx| {
                Theme::sync_system_appearance(Some(window), cx);
                let dark = Theme::global(cx).is_dark();
                theme::init(dark, cx);

                let view: AnyView = match mode.as_str() {
                    "gate1" => cx
                        .new(|cx| gates::gate1_input::Gate1::new(window, cx))
                        .into(),
                    "gate2" => cx
                        .new(|cx| gates::gate2_webview::Gate2::new(window, cx))
                        .into(),
                    "gate3" => gates::gate3_terminal::build(window, cx).into(),
                    _ => app::shell::Shell::build(window, cx).into(),
                };

                let root = cx.new(|cx| Root::new(view, window, cx));
                // Registered last on purpose. When two bindings match at the
                // same context depth the later registration wins, and the
                // component kit registers its own `escape` for the query field
                // while the root is being built.
                app::shell::bind_keys(cx);
                app::menu::init(cx);
                app::quick_settings::init(cx);
                app::global_hotkey::init(cx);
                root
            });

            if let Err(err) = opened {
                eprintln!("artifex: failed to open window: {err}");
            }
        })
        .detach();

        cx.activate(true);
    });
}
