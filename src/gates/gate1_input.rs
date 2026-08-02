//! Gate 1: Vietnamese text entry inside a GPUI multi-line field.
//!
//! The panel on the right reports exactly what reached the model, so a
//! composition bug shows up as codepoints rather than as a feeling. Checks:
//!
//! 1. Telex (`tieengs` -> `tiếng`) and VNI (`tie61ng` -> `tiếng`).
//! 2. A tone mark applied after an already multi-byte character.
//! 3. Undo (`Cmd-Z`) while a syllable is still being composed.
//! 4. Paste of pre-composed text with diacritics.

use gpui::prelude::*;
use gpui::{
    App, ClipboardItem, Context, Entity, IntoElement, ParentElement, Render, SharedString,
    Styled as _, Subscription, Window, div, px,
};
use gpui_component::input::{Input, InputEvent, InputState};
use gpui_component::{h_flex, v_flex};

use crate::theme::{ActiveTokens as _, Metrics, Radius, Space, Type};

const SAMPLE: &str = "Chào buổi sáng. Nguyễn Phước Hiệp đang thử gõ tiếng Việt \
trong GPUI. Dấu ngã, dấu hỏi, dấu nặng: ã ả ạ. Chữ đ và Đ.";

pub struct Gate1 {
    editor: Entity<InputState>,
    single: Entity<InputState>,
    log: Vec<SharedString>,
    changes: usize,
    _subs: Vec<Subscription>,
}

impl Gate1 {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let editor = cx.new(|cx| {
            InputState::new(window, cx)
                .multi_line(true)
                .soft_wrap(true)
                .placeholder("Gõ tiếng Việt ở đây (Telex hoặc VNI)...")
        });
        let single = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Ô một dòng, để so sánh hành vi IME")
        });

        let mut subs = Vec::new();
        subs.push(cx.subscribe_in(&editor, window, {
            move |this: &mut Self, state, ev: &InputEvent, _w, cx| match ev {
                InputEvent::Change => {
                    this.changes += 1;
                    let value = state.read(cx).value();
                    // Printed as well as shown, so the gate result is a log
                    // line rather than a screenshot.
                    println!(
                        "[gate1] change #{} value={:?} codepoints={}",
                        this.changes,
                        value.as_ref(),
                        value
                            .chars()
                            .map(|c| format!("U+{:04X}", c as u32))
                            .collect::<Vec<_>>()
                            .join(" ")
                    );
                    this.push_log(format!(
                        "change #{}: {} chars / {} bytes",
                        this.changes,
                        value.chars().count(),
                        value.len()
                    ));
                    cx.notify();
                }
                InputEvent::Focus => this.push_log("focus".into()),
                InputEvent::Blur => this.push_log("blur".into()),
                InputEvent::PressEnter { .. } => this.push_log("enter".into()),
            }
        }));

        Self {
            editor,
            single,
            log: Vec::new(),
            changes: 0,
            _subs: subs,
        }
    }

    fn push_log(&mut self, line: String) {
        self.log.push(line.into());
        if self.log.len() > 12 {
            self.log.remove(0);
        }
    }

    /// Codepoints of the caret's line. A decomposed result shows the base
    /// letter followed by a U+03xx combining mark; a pre-composed result shows
    /// one U+1Exx codepoint.
    fn caret_line_codepoints(&self, cx: &App) -> (SharedString, SharedString) {
        let value = self.editor.read(cx).value();
        let line = value.lines().last().unwrap_or("").to_string();
        let shown: String = line
            .chars()
            .rev()
            .take(24)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        let points = shown
            .chars()
            .map(|c| format!("U+{:04X}", c as u32))
            .collect::<Vec<_>>()
            .join(" ");
        (shown.into(), points.into())
    }
}

impl Render for Gate1 {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let c = cx.tokens().c;
        let value = self.editor.read(cx).value();
        let (tail, points) = self.caret_line_codepoints(cx);
        let is_nfc = !value
            .chars()
            .any(|ch| ('\u{0300}'..='\u{036F}').contains(&ch));

        h_flex()
            .size_full()
            .bg(c.canvas)
            .text_color(c.ink)
            .child(
                v_flex()
                    .flex_1()
                    .h_full()
                    .p(Space::L)
                    .pt(Metrics::TITLE_BAR)
                    .gap(Space::M)
                    .child(
                        div()
                            .text_size(Type::TITLE)
                            .child("Gate 1 - Vietnamese input"),
                    )
                    .child(
                        div()
                            .text_size(Type::CAPTION)
                            .text_color(c.ink_secondary)
                            .child(
                                "Telex: tieengs -> tiếng. VNI: tie61ng -> tiếng. \
                                 Cmd-Z giữa lúc ghép. Cmd-V để dán.",
                            ),
                    )
                    .child(
                        div()
                            .flex_1()
                            .rounded(Radius::CONTROL)
                            .border_1()
                            .border_color(c.border)
                            .bg(c.editor)
                            .p(Space::S)
                            .child(Input::new(&self.editor).h_full().appearance(false)),
                    )
                    .child(div().h(Metrics::FIELD).child(Input::new(&self.single)))
                    .child(
                        h_flex()
                            .gap(Space::S)
                            .child(
                                div()
                                    .id("copy-sample")
                                    .cursor_pointer()
                                    .px(Space::M)
                                    .py(Space::XS)
                                    .rounded(Radius::ROW)
                                    .bg(c.accent)
                                    .text_color(c.accent_ink)
                                    .text_size(Type::LABEL)
                                    .child("Copy Vietnamese sample")
                                    .on_click(cx.listener(|_, _, _, cx| {
                                        cx.write_to_clipboard(ClipboardItem::new_string(
                                            SAMPLE.to_string(),
                                        ));
                                    })),
                            )
                            .child(
                                div()
                                    .text_size(Type::MICRO)
                                    .text_color(c.ink_secondary)
                                    .child("then press Cmd-V in the field above"),
                            ),
                    ),
            )
            .child(div().w(px(1.)).h_full().bg(c.border))
            .child(
                v_flex()
                    .w(px(360.))
                    .h_full()
                    .bg(c.sidebar)
                    .p(Space::L)
                    .gap(Space::M)
                    .child(div().text_size(Type::HEADLINE).child("What arrived"))
                    .child(stat(c.ink_secondary, "chars", value.chars().count()))
                    .child(stat(c.ink_secondary, "bytes", value.len()))
                    .child(stat(c.ink_secondary, "lines", value.lines().count()))
                    .child(stat(c.ink_secondary, "change events", self.changes))
                    .child(
                        div()
                            .text_size(Type::CAPTION)
                            .text_color(if is_nfc { c.git_added } else { c.git_deleted })
                            .child(if is_nfc {
                                "pre-composed (NFC): no combining marks"
                            } else {
                                "decomposed: combining marks present"
                            }),
                    )
                    .child(div().h(px(1.)).w_full().bg(c.border))
                    .child(
                        div()
                            .text_size(Type::MICRO)
                            .text_color(c.ink_secondary)
                            .child("caret line tail"),
                    )
                    .child(
                        div()
                            .font_family("Menlo")
                            .text_size(Type::CAPTION)
                            .child(tail),
                    )
                    .child(
                        div()
                            .font_family("Menlo")
                            .text_size(Type::MICRO)
                            .text_color(c.ink_secondary)
                            .child(points),
                    )
                    .child(div().h(px(1.)).w_full().bg(c.border))
                    .child(
                        div()
                            .text_size(Type::MICRO)
                            .text_color(c.ink_secondary)
                            .child("event log"),
                    )
                    .child(
                        v_flex()
                            .gap(px(2.))
                            .children(self.log.iter().rev().take(10).map(|line| {
                                div()
                                    .font_family("Menlo")
                                    .text_size(Type::MICRO)
                                    .text_color(c.ink_secondary)
                                    .child(line.clone())
                            })),
                    )
                    .when(self.log.is_empty(), |this| {
                        this.child(
                            div()
                                .text_size(Type::MICRO)
                                .text_color(c.ink_secondary)
                                .child("(empty)"),
                        )
                    }),
            )
    }
}

fn stat(secondary: gpui::Hsla, label: &'static str, value: usize) -> impl IntoElement {
    h_flex()
        .justify_between()
        .child(
            div()
                .text_size(Type::CAPTION)
                .text_color(secondary)
                .child(label),
        )
        .child(
            div()
                .font_family("Menlo")
                .text_size(Type::CAPTION)
                .child(value.to_string()),
        )
}
