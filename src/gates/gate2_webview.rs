//! Gate 2: an embedded web preview inside a tab.
//!
//! Checks: does the page scroll, does it follow a window resize, and does it
//! respect the rounded clip of its GPUI container. The webview is a native
//! child view, so the third question is the interesting one.

use std::path::PathBuf;

use gpui::prelude::*;
use gpui::{
    App, Context, Entity, IntoElement, ParentElement, Render, Styled as _, Window, div, px,
};
use gpui_component::{h_flex, v_flex};
use gpui_wry::WebView;

use crate::theme::{ActiveTokens as _, Metrics, Radius, Space, Type};

const PAGE: &str = r#"<!doctype html>
<html><head><meta charset="utf-8"><style>
 body { font: 16px -apple-system, system-ui; margin: 0; padding: 32px;
        background: #F8F7F4; color: #1E1C1A; }
 h1 { font-size: 28px; margin: 0 0 16px; }
 .row { padding: 12px 0; border-bottom: 1px solid #BFBAB2; }
 code { font-family: 'JetBrains Mono', Menlo, monospace; background: #EEEBE3;
        padding: 2px 5px; border-radius: 4px; }
 @media (prefers-color-scheme: dark) {
   body { background: #191B1E; color: #E9E5DF; }
   .row { border-color: #42474D; }
   code { background: #292C30; }
 }
</style></head><body>
<h1>Rustelier - webview gate</h1>
<p>Scroll this page. Resize the window. Watch the rounded corners of the host card.</p>
<div id="rows"></div>
<script>
 const host = document.getElementById('rows');
 for (let i = 1; i <= 120; i++) {
   const d = document.createElement('div');
   d.className = 'row';
   d.innerHTML = 'row ' + i + ' - <code>window.innerWidth = ' + window.innerWidth + '</code>';
   host.appendChild(d);
 }
 window.addEventListener('resize', () => {
   document.querySelectorAll('.row').forEach((d, i) => {
     d.innerHTML = 'row ' + (i + 1) + ' - <code>window.innerWidth = ' + window.innerWidth + '</code>';
   });
 });
</script>
</body></html>"#;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Preview,
    Notes,
}

pub struct Gate2 {
    webview: Entity<WebView>,
    tab: Tab,
    rounded: bool,
}

impl Gate2 {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let path = write_page();
        let webview = cx.new(|cx| {
            let builder = wry::WebViewBuilder::new().with_devtools(true);
            let handle = raw_window_handle::HasWindowHandle::window_handle(window)
                .expect("no window handle");
            let raw = builder
                .build_as_child(&handle)
                .expect("failed to build webview");
            WebView::new(raw, window, cx)
        });
        let url = format!("file://{}", path.display());
        webview.update(cx, |view, _| view.load_url(&url));

        Self {
            webview,
            tab: Tab::Preview,
            rounded: true,
        }
    }

    fn select(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.tab = tab;
        // A native child view keeps painting unless it is hidden explicitly.
        self.webview.update(cx, |view, _| {
            if tab == Tab::Preview {
                view.show();
            } else {
                view.hide();
            }
        });
        cx.notify();
    }
}

impl Render for Gate2 {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let tab = self.tab;

        v_flex()
            .size_full()
            .bg(c.canvas)
            .text_color(c.ink)
            .child(
                h_flex()
                    .h(Metrics::TAB_BAR + Metrics::TITLE_BAR)
                    .pt(Metrics::TITLE_BAR)
                    .w_full()
                    .bg(c.chrome)
                    .px(Space::XS)
                    .gap(Space::XS)
                    .border_b_1()
                    .border_color(c.border)
                    .child(tab_cell("Preview", tab == Tab::Preview, c, {
                        cx.listener(|this, _, _, cx| this.select(Tab::Preview, cx))
                    }))
                    .child(tab_cell("Notes", tab == Tab::Notes, c, {
                        cx.listener(|this, _, _, cx| this.select(Tab::Notes, cx))
                    }))
                    .child(div().flex_1())
                    .child(
                        div()
                            .id("toggle-round")
                            .cursor_pointer()
                            .px(Space::S)
                            .py(px(3.))
                            .rounded(Radius::ROW)
                            .bg(if self.rounded { c.accent } else { c.raised })
                            .text_color(if self.rounded { c.accent_ink } else { c.ink })
                            .text_size(Type::MICRO)
                            .child("rounded clip")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.rounded = !this.rounded;
                                cx.notify();
                            })),
                    ),
            )
            .child(
                div().flex_1().p(Space::L).child(
                    div()
                        .size_full()
                        .overflow_hidden()
                        .border_1()
                        .border_color(c.border)
                        .bg(c.editor)
                        .when(self.rounded, |this| this.rounded(px(24.)))
                        .when(tab == Tab::Preview, |this| this.child(self.webview.clone()))
                        .when(tab == Tab::Notes, |this| {
                            this.p(Space::L).child(
                                v_flex()
                                    .gap(Space::S)
                                    .child(div().text_size(Type::TITLE).child("Notes tab"))
                                    .child(
                                        div()
                                            .text_size(Type::BODY)
                                            .text_color(c.ink_secondary)
                                            .child(
                                                "The webview must be fully gone here. If any web \
                                                 content still shows through, the native child \
                                                 view is not being hidden with the tab.",
                                            ),
                                    ),
                            )
                        }),
                ),
            )
    }
}

fn tab_cell(
    label: &'static str,
    selected: bool,
    c: crate::theme::Colors,
    on_click: impl Fn(&gpui::ClickEvent, &mut Window, &mut App) + 'static,
) -> impl IntoElement {
    div()
        .id(label)
        .cursor_pointer()
        .h_full()
        .flex()
        .items_center()
        .px(Space::M)
        .rounded(Radius::ROW)
        .my(Space::XS)
        .text_size(Type::LABEL)
        .when(selected, |this| {
            this.bg(c.chrome_selection)
                .text_color(c.chrome_selection_ink)
        })
        .when(!selected, |this| this.text_color(c.ink_secondary))
        .child(label)
        .on_click(on_click)
}

fn write_page() -> PathBuf {
    let path = std::env::temp_dir().join("rustelier-gate2.html");
    let _ = std::fs::write(&path, PAGE);
    path
}
