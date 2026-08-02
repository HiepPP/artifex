//! Gate 3: a real `zsh` under `alacritty_terminal`.
//!
//! Checks: 256-colour and truecolor output, arrow keys and TUI navigation,
//! resize, and Vietnamese input through the IME.

use gpui::prelude::*;
use gpui::{Context, Entity, IntoElement, ParentElement, Render, Styled as _, Window, div, px};
use gpui_component::{h_flex, v_flex};

use crate::terminal::TerminalView;
use crate::theme::{ActiveTokens as _, Metrics, Radius, Space, Type};

pub struct Gate3 {
    terminal: Option<Entity<TerminalView>>,
    error: Option<String>,
    show_terminal: bool,
}

/// Builds the gate, keeping the fallible terminal construction outside the
/// entity so a spawn failure is reportable instead of fatal.
pub fn build(window: &mut Window, cx: &mut gpui::App) -> Entity<Gate3> {
    let cwd = std::env::current_dir().unwrap_or_else(|_| "/".into());
    let mut error = None;
    let terminal = match TerminalView::open(cwd, window, cx) {
        Ok(entity) => Some(entity),
        Err(err) => {
            error = Some(err.to_string());
            None
        }
    };
    cx.new(|_| Gate3 {
        terminal,
        error,
        show_terminal: true,
    })
}

impl Render for Gate3 {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let terminal = self.terminal.clone();

        if let Some(terminal) = terminal.as_ref() {
            let active = self.show_terminal;
            terminal.update(cx, |view, _| view.set_active(active));
        }

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
                    .px(Space::S)
                    .gap(Space::S)
                    .items_center()
                    .border_b_1()
                    .border_color(c.border)
                    .child(
                        div()
                            .id("tab-terminal")
                            .cursor_pointer()
                            .px(Space::M)
                            .py(px(4.))
                            .rounded(Radius::ROW)
                            .text_size(Type::LABEL)
                            .when(self.show_terminal, |this| {
                                this.bg(c.chrome_selection)
                                    .text_color(c.chrome_selection_ink)
                            })
                            .child("Terminal")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_terminal = true;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("tab-other")
                            .cursor_pointer()
                            .px(Space::M)
                            .py(px(4.))
                            .rounded(Radius::ROW)
                            .text_size(Type::LABEL)
                            .when(!self.show_terminal, |this| {
                                this.bg(c.chrome_selection)
                                    .text_color(c.chrome_selection_ink)
                            })
                            .child("Other tab")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_terminal = false;
                                cx.notify();
                            })),
                    )
                    .child(div().flex_1())
                    .child(
                        div()
                            .text_size(Type::MICRO)
                            .text_color(c.ink_secondary)
                            .child(
                                terminal
                                    .as_ref()
                                    .map(|t| t.read(cx).session.title.to_string())
                                    .unwrap_or_default(),
                            ),
                    ),
            )
            .child(
                div()
                    .flex_1()
                    .relative()
                    .when_some(self.error.clone(), |this, err| {
                        this.child(
                            div()
                                .p(Space::XL)
                                .text_color(c.git_deleted)
                                .child(format!("terminal failed to start: {err}")),
                        )
                    })
                    .when_some(terminal, |this, terminal| {
                        // The process stays mounted while the other tab is
                        // selected; only its allocated size changes.
                        this.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .when(!self.show_terminal, |el| el.invisible())
                                .child(terminal),
                        )
                    })
                    .when(!self.show_terminal, |this| {
                        this.child(
                            div()
                                .absolute()
                                .top_0()
                                .left_0()
                                .size_full()
                                .bg(c.canvas)
                                .p(Space::XL)
                                .child(
                                    v_flex()
                                        .gap(Space::S)
                                        .child(div().text_size(Type::TITLE).child("Other tab"))
                                        .child(
                                            div()
                                                .text_size(Type::BODY)
                                                .text_color(c.ink_secondary)
                                                .child(
                                                    "Switch back. The shell must still hold its \
                                                     scrollback and its running process.",
                                                ),
                                        ),
                                ),
                        )
                    }),
            )
            .child(
                h_flex()
                    .h(Metrics::STATUS_BAR)
                    .w_full()
                    .bg(c.chrome)
                    .px(Space::M)
                    .items_center()
                    .gap(Space::M)
                    .border_t_1()
                    .border_color(c.border)
                    .text_size(Type::MICRO)
                    .text_color(c.ink_secondary)
                    .child("try: ls --color=always | head -40")
                    .child("printf '\\e[38;2;200;90;40mtruecolor\\e[0m\\n'")
                    .child("htop / less / vim for arrow keys"),
            )
    }
}
