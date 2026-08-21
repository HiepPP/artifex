//! Atelier design tokens, transcribed from `atelier/DESIGN.md`.
//!
//! The POC keeps every token in one global so the surfaces read the same values
//! the Swift application reads from `AtelierPalette`. Light and dark are both
//! carried; `AtelierTokens::resolve` picks the active pair.

use gpui::{App, Global, Hsla, Pixels, Rgba, Window, px, rgb};
use gpui_component::{Theme, ThemeMode};

/// One light/dark colour pair.
#[derive(Clone, Copy)]
pub struct Duo {
    pub light: u32,
    pub dark: u32,
}

impl Duo {
    const fn new(light: u32, dark: u32) -> Self {
        Self { light, dark }
    }

    fn pick(self, dark: bool) -> Hsla {
        let raw: Rgba = rgb(if dark { self.dark } else { self.light });
        raw.into()
    }
}

macro_rules! tokens {
    ($($name:ident = ($light:literal, $dark:literal);)*) => {
        /// Raw token table. One entry per `DESIGN.md` colour token.
        pub struct Palette;
        impl Palette {
            $(pub const $name: Duo = Duo::new($light, $dark);)*
        }

        /// Resolved colours for the active appearance.
        #[derive(Clone, Copy)]
        pub struct Colors {
            $(pub $name: Hsla,)*
        }

        impl Colors {
            fn resolve(dark: bool) -> Self {
                Self { $($name: Palette::$name.pick(dark),)* }
            }

            /// Light colours, for tests that need a palette without a window.
            #[cfg(test)]
            pub fn for_test() -> Self {
                Self::resolve(false)
            }
        }
    };
}

tokens! {
    toolbar                 = (0x292F37, 0x20252C);
    chrome                  = (0xE7E3DD, 0x23262A);
    canvas                  = (0xDEDAD3, 0x181A1D);
    sidebar                 = (0xEEEBE3, 0x202328);
    panel                   = (0xF2F0EC, 0x292C30);
    raised                  = (0xD4D0C9, 0x34383D);
    editor                  = (0xF8F7F4, 0x191B1E);
    tab_inactive            = (0xE5E1DB, 0x25282C);
    border                  = (0xBFBAB2, 0x42474D);
    selection               = (0xDED1C6, 0x4B3730);
    chrome_selection        = (0xF7F3EE, 0x4D4742);
    chrome_selection_ink    = (0x2B2724, 0xF2EFEA);
    hover                   = (0xD8D4CD, 0x383C41);
    pressed                 = (0xCCC7BF, 0x44494F);
    accent                  = (0xA44F32, 0xD79570);
    accent_ink              = (0xFFF9F2, 0x21150F);
    // GitHub Primer link blues; the Markdown preview matches github.com.
    link                    = (0x0969DA, 0x4493F8);
    text_selection          = (0x4D8DCA, 0x4D8DCA);
    workflow_done           = (0x4E6C55, 0x7FA98A);
    workflow_todo           = (0x8A652B, 0xCAA15B);
    workflow_blocked        = (0x934941, 0xD17B72);
    rail_top                = (0x1D232B, 0x171C22);
    rail_bottom             = (0x2D3B45, 0x202D35);
    rail_solid              = (0x252D35, 0x1D252C);
    rail_foreground         = (0xF3F1EC, 0xF3F1EC);
    rail_secondary          = (0xB6BEC3, 0xADB7BD);
    rail_selection          = (0x4C565F, 0x46505A);
    rail_hover              = (0x333C44, 0x2D373F);
    rail_pressed            = (0x46515A, 0x404B54);
    rail_border             = (0x59636B, 0x4C575F);
    file_tree_foreground    = (0x302E2B, 0xE8E4DE);
    git_added               = (0x356B43, 0x7FC58C);
    git_modified            = (0x8A5B21, 0xD4A45D);
    git_deleted             = (0xA13E37, 0xE17B70);
    git_untracked           = (0x286E68, 0x63C3B8);
    // Label colours. DESIGN.md defers to the native label colours; the POC
    // pins the two levels it needs so the surfaces stay legible on both bases.
    ink                     = (0x1E1C1A, 0xE9E5DF);
    ink_secondary           = (0x6A6560, 0x9A948C);
}

/// `DESIGN.md` > Spacing and Shape Tokens.
pub struct Space;
impl Space {
    pub const XS: Pixels = px(4.);
    pub const S: Pixels = px(8.);
    pub const M: Pixels = px(12.);
    pub const L: Pixels = px(16.);
    pub const XL: Pixels = px(24.);
    pub const XXL: Pixels = px(32.);
}

/// `DESIGN.md` > Shape and Depth.
pub struct Radius;
impl Radius {
    pub const PANEL: Pixels = px(12.);
    pub const CONTROL: Pixels = px(8.);
    pub const ROW: Pixels = px(6.);
}

/// `DESIGN.md` > Fixed Heights and Panel Widths.
pub struct Metrics;
impl Metrics {
    pub const TOP_CHROME: Pixels = px(52.);
    pub const PANEL_HEADER: Pixels = px(48.);
    pub const READER_LOCATOR: Pixels = px(36.);
    pub const SECTION_HEADER: Pixels = px(40.);
    pub const TAB_BAR: Pixels = px(44.);
    pub const STATUS_BAR: Pixels = px(28.);
    pub const FIELD: Pixels = px(32.);
    pub const CONTROL: Pixels = px(28.);
    pub const COMPACT_CONTROL: Pixels = px(24.);
    pub const ROW: Pixels = px(28.);
    pub const TREE_ROW: Pixels = px(32.);
    pub const RAIL_WIDTH: Pixels = px(230.);
    pub const RAIL_ITEM_HEIGHT: Pixels = px(44.);
    pub const RAIL_ITEM_GAP: Pixels = px(4.);

    pub const SIDEBAR_MIN: Pixels = px(288.);
    pub const SIDEBAR_IDEAL: Pixels = px(288.);
    pub const SIDEBAR_MAX: Pixels = px(420.);
    pub const CENTER_MIN: Pixels = px(420.);
    pub const INSPECTOR_MIN: Pixels = px(280.);
    pub const INSPECTOR_IDEAL: Pixels = px(300.);
    pub const INSPECTOR_MAX: Pixels = px(420.);

    pub const PALETTE_WIDTH: Pixels = px(640.);
    pub const PALETTE_HEIGHT: Pixels = px(410.);
    pub const PALETTE_FIELD: Pixels = px(52.);
    /// `DESIGN.md` > global search trigger.
    pub const PROJECT_MENU_WIDTH: Pixels = px(568.);
    /// Height the content must clear when the window draws a transparent title
    /// bar. Matches the traffic-light origin plus the button and its bottom gap.
    /// Only correct while a title bar exists; use `title_bar_inset`.
    pub const TITLE_BAR: Pixels = px(28.);

    /// `DESIGN.md` > Window and Modes.
    pub const COMPACT_MAX: f32 = 900.;
    pub const WIDE_MIN: f32 = 1280.;
}

/// `DESIGN.md` > Typography.
pub struct Type;
impl Type {
    pub const MICRO: Pixels = px(11.);
    pub const CAPTION: Pixels = px(12.);
    pub const LABEL: Pixels = px(12.5);
    pub const BODY: Pixels = px(13.5);
    pub const UI: Pixels = px(14.);
    pub const HEADLINE: Pixels = px(16.);
    pub const TITLE: Pixels = px(17.);
    pub const DISPLAY: Pixels = px(24.);
    pub const EDITOR: Pixels = px(16.);
}

/// Layout mode derived from container width. `DESIGN.md` > Window and Modes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutMode {
    Compact,
    Standard,
    Wide,
}

impl LayoutMode {
    pub fn for_width(width: f32) -> Self {
        if width < Metrics::COMPACT_MAX {
            Self::Compact
        } else if width < Metrics::WIDE_MIN {
            Self::Standard
        } else {
            Self::Wide
        }
    }

    pub fn allows_sidebar(self) -> bool {
        !matches!(self, Self::Compact)
    }

    pub fn allows_inspector(self) -> bool {
        matches!(self, Self::Wide)
    }
}

/// Space the top chrome must leave for the window's title bar.
///
/// A full-screen window has no title bar and no traffic lights, so reserving
/// `Metrics::TITLE_BAR` there pushes every header against the menu bar. The
/// window knows which state it is in, so ask it instead of assuming.
pub fn title_bar_inset(window: &Window) -> Pixels {
    if window.is_fullscreen() {
        px(0.)
    } else {
        Metrics::TITLE_BAR
    }
}

/// The resolved token set for the current appearance.
pub struct AtelierTokens {
    pub dark: bool,
    pub c: Colors,
}

impl Global for AtelierTokens {}

/// Content text scale from `Cmd-=`/`Cmd--`. Editor, diff, terminal, and Markdown
/// read it at render time to scale their shared `Type::EDITOR` base.
#[derive(Clone, Copy)]
pub struct EditorZoom(pub f32);

impl Global for EditorZoom {}

/// Interface text scale for chrome, panels, overlays, and component widgets.
#[derive(Clone, Copy)]
pub struct UiZoom(pub f32);

impl Global for UiZoom {}

/// Publishes the content scale so every document surface picks it up next render.
pub fn set_editor_zoom(zoom: f32, cx: &mut App) {
    cx.set_global(EditorZoom(zoom));
}

/// Publishes the interface scale and updates gpui-component widgets.
pub fn set_ui_zoom(zoom: f32, cx: &mut App) {
    let zoom = zoom.clamp(0.8, 1.4);
    cx.set_global(UiZoom(zoom));
    let theme = Theme::global_mut(cx);
    theme.font_size = Type::UI * zoom;
    theme.mono_font_size = Type::BODY * zoom;
}

pub fn ui_zoom(cx: &App) -> f32 {
    cx.try_global::<UiZoom>().map_or(1.0, |zoom| zoom.0)
}

impl AtelierTokens {
    fn build(dark: bool) -> Self {
        Self {
            dark,
            c: Colors::resolve(dark),
        }
    }
}

pub trait ActiveTokens {
    fn tokens(&self) -> &AtelierTokens;
}

impl ActiveTokens for App {
    fn tokens(&self) -> &AtelierTokens {
        self.global::<AtelierTokens>()
    }
}

/// Installs the Atelier tokens and folds the important ones into the
/// gpui-component theme so its own widgets sit on the same surfaces.
pub fn init(dark: bool, cx: &mut App) {
    let editor_zoom = cx.try_global::<EditorZoom>().map_or(1.0, |zoom| zoom.0);
    let ui_zoom = cx.try_global::<UiZoom>().map_or(1.0, |zoom| zoom.0);
    Theme::change(
        if dark {
            ThemeMode::Dark
        } else {
            ThemeMode::Light
        },
        None,
        cx,
    );

    let tokens = AtelierTokens::build(dark);
    let c = tokens.c;
    cx.set_global(tokens);
    cx.set_global(EditorZoom(editor_zoom));
    cx.set_global(UiZoom(ui_zoom));

    let theme = Theme::global_mut(cx);
    theme.radius = Radius::CONTROL;
    theme.radius_lg = Radius::PANEL;
    theme.font_size = Type::UI * ui_zoom;
    theme.mono_font_size = Type::BODY * ui_zoom;
    theme.shadow = false;

    let colors = &mut theme.colors;
    colors.background = c.canvas;
    colors.foreground = c.ink;
    colors.border = c.border;
    colors.muted = c.raised;
    colors.muted_foreground = c.ink_secondary;
    colors.popover = c.panel;
    colors.popover_foreground = c.ink;
    colors.input = c.border;
    colors.ring = c.accent;
    colors.link = c.link;
    colors.link_hover = c.link;
    colors.link_active = c.link;
    // The kit paints inline-code chips in `TextView` with this accent. A
    // neutral ink wash reads like GitHub's grey chip in both appearances,
    // instead of the warm selection tan.
    colors.accent = c.ink.opacity(0.06);
    colors.accent_foreground = c.ink;
    colors.primary = c.accent;
    colors.primary_foreground = c.accent_ink;
    colors.primary_hover = c.accent;
    colors.primary_active = c.accent;
    colors.secondary = c.raised;
    colors.secondary_foreground = c.ink;
    colors.secondary_hover = c.hover;
    colors.secondary_active = c.pressed;
    // Translucent so the selected glyphs stay legible: the kit paints this quad
    // over the text, and an opaque token would blank the line out. Prose text
    // selection is the cool blue at 22 percent; the editor, diff, and terminal
    // paint their own warm `selection` fill.
    colors.selection = c.text_selection.opacity(0.22);
    colors.list = c.sidebar;
    colors.list_active = c.selection;
    colors.list_hover = c.hover;
    colors.sidebar = c.sidebar;
    colors.sidebar_foreground = c.ink;
    colors.sidebar_border = c.border;
    colors.sidebar_accent = c.selection;
    colors.title_bar = c.chrome;
    colors.tab_bar = c.chrome;
    colors.tab = c.tab_inactive;
    colors.tab_active = c.chrome_selection;
    colors.tab_foreground = c.ink_secondary;
    colors.tab_active_foreground = c.chrome_selection_ink;
    colors.scrollbar_thumb = c.border;
}

/// Re-resolves the token set after the system appearance flips.
pub fn set_dark(dark: bool, cx: &mut App) {
    init(dark, cx);
}
