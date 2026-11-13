//! Native NSH rich-text surface and app-owned typography for its semantic parts.
use super::Editor;
use nsh_base::{
    rich_editor::{
        model::{Mark, Node},
        view::{PartStyles, RichEditorClasses, RichEditorStyles, rich_editor},
    },
    text_view::PartStyle,
};
use nsh_core::{
    StyleKey, StyleValue,
    element::{Style, StyleRefinement},
    prelude::*,
};
use serde_json::Value;
use std::{collections::BTreeMap, rc::Rc, time::Duration};

fn part(style: StyleRefinement) -> PartStyle {
    Style::from(style).into()
}
fn color(value: u32) -> Hsla {
    rgb(value & 0xffffff).into()
}
fn declaration(key: StyleKey, value: StyleValue<'_>) -> Style {
    key.parse_value(value)
        .map(|v| Style::default().with(key, v))
        .unwrap_or_default()
}
fn string_style(key: StyleKey, value: &str) -> Style {
    declaration(key, StyleValue::Str(value))
}
fn number_style(key: StyleKey, value: f32) -> Style {
    declaration(key, StyleValue::Number(value))
}
fn font_size(value: &str) -> Option<f32> {
    let value = value.trim();
    let size = value
        .trim_end_matches("pt")
        .trim_end_matches("px")
        .parse::<f32>()
        .ok()?;
    (size.is_finite() && (1.0..=1638.).contains(&size)).then_some(size)
}
fn mark_styles(mark: &Mark, styles: &mut BTreeMap<String, Style>, zoom: f32) {
    for (key, value) in mark.attrs() {
        let Some(value) = value.as_str() else {
            continue;
        };
        let style = match key.as_str() {
            "fontFamily" => string_style(StyleKey::FontFamily, value),
            "fontSize" => {
                let Some(size) = font_size(value) else {
                    continue;
                };
                number_style(StyleKey::FontSize, size * zoom)
            }
            "color" => string_style(
                if mark.ty().name() == "highlight" {
                    StyleKey::BackgroundColor
                } else {
                    StyleKey::Color
                },
                value,
            ),
            "backgroundColor" => string_style(StyleKey::BackgroundColor, value),
            _ => continue,
        };
        styles.insert(format!("{}[{key}={value}]", mark.ty().name()), style);
    }
}
fn node_styles(node: &Node, styles: &mut BTreeMap<String, Style>) {
    for (key, value) in node.attrs() {
        let Some(value) = value.as_str() else {
            continue;
        };
        let style = match key.as_str() {
            "textAlign" => string_style(StyleKey::TextAlign, value),
            "lineHeight" => string_style(StyleKey::LineHeight, value),
            "indent" => string_style(StyleKey::MarginLeft, value),
            "direction" => string_style(StyleKey::WritingDirection, value),
            _ => continue,
        };
        styles.insert(format!("{}[{key}={value}]", node.ty().name()), style);
    }
}
fn styles(editor: &Editor) -> RichEditorStyles {
    let display = editor.display_state();
    let zoom = f32::from(display.zoom) / 100.;
    let format = editor.formatting();
    let mut styles = RichEditorStyles::default();
    let mut content = StyleRefinement::default()
        .p(px(20. * zoom))
        .text_size(px(13. * zoom))
        .font_family(if display.plain {
            format.font
        } else {
            crate::fonts::default_rich_font().to_owned()
        })
        .text_color(color(display.text_override.unwrap_or(if display.plain {
            format.foreground
        } else {
            0
        })));
    if display.plain {
        content = content.text_size(px(format.size * zoom));
    }
    if display.wrap_to_page {
        content = content
            .w(px(editor.page_settings().width_twips as f32 / 20. * zoom))
            .mx(auto());
    } else if !display.wrap {
        // shortcut: NSH has no unwrapped RichEditor prop; expand the native content box for long lines.
        let width = editor
            .plain_text_snapshot()
            .lines()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0) as f32
            * format.size
            * zoom
            + 40.;
        content = content.min_w(px(width.max(editor.viewport_width.get())));
    }
    styles.content = Some(part(content));
    styles.root = Some(part(
        StyleRefinement::default().bg(color(display.background)),
    ));
    styles.caret = Some(part(
        StyleRefinement::default().bg(color(display.text_override.unwrap_or(0))),
    ));
    let mut nodes = BTreeMap::new();
    let mut marks = BTreeMap::new();
    nodes.insert(
        "paragraph".into(),
        number_style(StyleKey::MarginBottom, 4. * zoom),
    );
    for (level, size) in [(1, 28.), (2, 23.), (3, 19.), (4, 16.), (5, 14.), (6, 13.)] {
        nodes.insert(
            format!("heading[level={level}]"),
            number_style(StyleKey::FontSize, size * zoom)
                .then(&number_style(StyleKey::FontWeight, 700.))
                .then(&number_style(StyleKey::MarginTop, 12. * zoom))
                .then(&number_style(StyleKey::MarginBottom, 5. * zoom)),
        );
    }
    marks.insert("bold".into(), number_style(StyleKey::FontWeight, 700.));
    marks.insert("italic".into(), string_style(StyleKey::FontStyle, "italic"));
    marks.insert(
        "underline".into(),
        string_style(StyleKey::TextDecorationLine, "underline"),
    );
    marks.insert(
        "strike".into(),
        string_style(StyleKey::TextDecorationLine, "line-through"),
    );
    marks.insert(
        "code".into(),
        string_style(StyleKey::FontFamily, crate::fonts::default_plain_font()),
    );
    marks.insert(
        "link".into(),
        string_style(StyleKey::TextDecorationLine, "underline")
            .then(&string_style(StyleKey::Color, "#2259bd")),
    );
    marks.insert(
        "subscript".into(),
        number_style(StyleKey::FontSize, 10. * zoom)
            .then(&string_style(StyleKey::VerticalAlign, "text-bottom")),
    );
    marks.insert(
        "superscript".into(),
        number_style(StyleKey::FontSize, 10. * zoom)
            .then(&string_style(StyleKey::VerticalAlign, "text-top")),
    );
    let state = editor.native_session().state();
    state.doc().descendants(&mut |node, _, _, _| {
        node_styles(node, &mut nodes);
        for mark in node.marks() {
            mark_styles(mark, &mut marks, zoom);
        }
        true
    });
    styles.nodes = Some(PartStyles(nodes));
    styles.marks = Some(PartStyles(marks));
    styles
}
fn completion_popup(editor: Rc<Editor>, revision: Signal<u64>) -> impl IntoElement {
    dynamic(move || {
        revision.get();
        let Some((completion, selected)) = editor.completion_state() else {
            return view().into_element();
        };
        for (key, delta) in [("up", -1), ("down", 1)] {
            let editor = editor.clone();
            on_key_with(
                key,
                KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    repeat: true,
                    ..Default::default()
                },
                move |_| editor.move_completion(delta),
            );
        }
        for key in ["tab", "enter"] {
            let editor = editor.clone();
            on_key_with(
                key,
                KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    ..Default::default()
                },
                move |_| {
                    if let Err(error) = editor.accept_completion(None) {
                        editor.report_error(error.to_string());
                    }
                },
            );
        }
        let dismiss = editor.clone();
        on_key_with(
            "escape",
            KeyOptions {
                when: nsh_core::keymap::When::Always,
                ..Default::default()
            },
            move |_| dismiss.dismiss_completion(),
        );
        let colors = crate::ui::Palette::current();
        view()
            .flex_col()
            .min_w(px(200.))
            .max_w(px(360.))
            .p(px(4.))
            .gap(px(2.))
            .rounded(px(5.))
            .border_1()
            .border_color(crate::ui::rgb(colors.border))
            .bg(crate::ui::rgb(colors.field))
            .children(
                completion
                    .candidates
                    .into_iter()
                    .enumerate()
                    .map(|(index, candidate)| {
                        let editor = editor.clone();
                        crate::ui::button(format!("completion-{index}"), candidate, 12.)
                            .justify_start()
                            .bg(crate::ui::rgb(if index == selected {
                                colors.accent
                            } else {
                                colors.field
                            }))
                            .text_color(crate::ui::rgb(if index == selected {
                                colors.on_accent
                            } else {
                                colors.ink
                            }))
                            .on_press(move |_: &PressEvent| {
                                if let Err(error) = editor.accept_completion(Some(index)) {
                                    editor.report_error(error.to_string());
                                }
                            })
                            .into_element()
                    })
                    .collect::<Vec<_>>(),
            )
            .into_element()
    })
}

/// Binds a native RichEditor to the document's existing session.
pub fn surface(editor: Rc<Editor>) -> impl IntoElement {
    let revision = signal(editor.revision());
    let tick = editor.clone();
    set_interval(Duration::from_millis(40), move || {
        tick.refresh_completion();
        revision.set(tick.revision());
    });
    let editable = editor.clone();
    let styled = editor.clone();
    let files = editor.clone();
    rich_editor()
        .editor(editor.native_session())
        .size_full()
        .bubble_menu(completion_popup(editor.clone(), revision))
        .bubble_menu_placement(Placement::Bottom)
        .bubble_menu_offset(px(4.))
        .accessibility_label("Document editor")
        .editable(move || {
            revision.get();
            !editable.is_read_only()
        })
        .styles(move || {
            revision.get();
            styles(&styled)
        })
        .classes({
            let mut classes = RichEditorClasses::default();
            classes.content = Some("min-w-0".into());
            classes.task_checkbox = Some("w-4 h-4 border rounded".into());
            classes.nodes = Some(BTreeMap::from([
                ("blockquote".into(), "border-l-2 pl-4".into()),
                ("codeBlock".into(), "p-3".into()),
                ("tableCell".into(), "border p-2".into()),
                ("tableHeader".into(), "border p-2 font-bold".into()),
            ]));
            classes
        })
        .on_paste_files(
            move |paste: &nsh_base::rich_editor::view::RichEditorPastedFiles| {
                let result = (|| -> anyhow::Result<()> {
                    for file in &paste.files {
                        files.insert_image_bytes(&file.data)?;
                    }
                    for uri in &paste.uris {
                        let url = url::Url::parse(uri)?;
                        let path = url.to_file_path().map_err(|_| {
                            anyhow::anyhow!("The copied image has no local file path")
                        })?;
                        files.insert_image(&path)?;
                    }
                    Ok(())
                })();
                if let Err(error) = result {
                    files.report_error(error.to_string());
                }
            },
        )
}
