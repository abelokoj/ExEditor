use super::*;

#[test]
fn utf16_ranges_cover_empty_paragraphs_without_splitting_surrogates() {
    let editor = Editor::new().unwrap();
    editor.set_text("A😀\n\nB").unwrap();
    assert_eq!(editor.text().unwrap(), "A😀\n\nB");
    for offset in [0, 1, 3, 4, 5, 6] {
        let position = editor.position_at_utf16(offset).unwrap();
        assert_eq!(editor.utf16_at_position(&position), Some(offset));
    }
    assert!(editor.position_at_utf16(2).is_err());
    editor.select_range(1..3).unwrap();
    assert_eq!(editor.selected_text().unwrap(), "😀");
    editor.replace_current("café\nnext").unwrap();
    assert_eq!(editor.text().unwrap(), "Acafé\nnext\n\nB");
    editor.execute(EditCommand::Undo).unwrap();
    assert_eq!(editor.text().unwrap(), "A😀\n\nB");
}

#[test]
fn rich_native_snapshots_keep_marks_tables_and_undo_across_view_changes() {
    let editor = Editor::new().unwrap();
    editor.set_text("bold café 😀").unwrap();
    editor.select_range(0..4).unwrap();
    editor.execute(EditCommand::Bold).unwrap();
    let session = editor.native_session();
    editor.set_visible(false);
    editor.set_zoom(150);
    editor.set_visible(true);
    assert_eq!(session, editor.native_session());
    assert!(editor.html().unwrap().contains("<strong>bold</strong>"));
    editor
        .select_range(editor.text_length()..editor.text_length())
        .unwrap();
    editor.insert_table(2, 2).unwrap();
    let snapshot = editor.snapshot().unwrap();
    let restored = Editor::new().unwrap();
    restored.set_snapshot(&snapshot).unwrap();
    assert_eq!(editor.html().unwrap(), restored.html().unwrap());
    assert!(restored.html().unwrap().contains("<table"));
    editor.execute(EditCommand::Undo).unwrap();
    assert!(!editor.html().unwrap().contains("<table"));
}

#[test]
fn plain_mode_has_no_rich_schema_or_input_rules() {
    let editor = Editor::new().unwrap();
    editor.set_plain_text_mode(true).unwrap();
    let state = editor.native_session().state();
    assert!(state.schema().mark_type("bold").is_none());
    assert!(state.schema().node_type("heading").is_none());
    editor.set_text("# title\n**plain**\n- item").unwrap();
    assert_eq!(editor.text().unwrap(), "# title\n**plain**\n- item");
    assert!(editor.execute(EditCommand::Bold).is_err());
    assert!(editor.insert_table(1, 1).is_err());
}

#[test]
fn copied_native_fragments_rebind_to_another_documents_schema() {
    let source = Editor::new().unwrap();
    source.set_text("rich café 😀").unwrap();
    source.select_range(0..source.text_length()).unwrap();
    source.execute(EditCommand::Bold).unwrap();
    let (_, _, _, slice) = source.selected_clipboard().unwrap();
    let target = Editor::new().unwrap();
    target.paste_slice(slice).unwrap();
    assert_eq!(target.text().unwrap(), "rich café 😀");
    assert!(target.html().unwrap().contains("<strong>"));
}

#[test]
fn source_coloring_is_display_only_and_read_only_rejects_edits() {
    let editor = Editor::new().unwrap();
    editor.set_plain_text_mode(true).unwrap();
    editor.set_text("let value = 1;").unwrap();
    editor.mark_saved();
    let snapshot = editor.snapshot();
    editor
        .apply_spans(&[crate::syntax::Span {
            start: 0,
            len: 3,
            color: 0x8844ff,
            bold: true,
            italic: false,
        }])
        .unwrap();
    let misspelled = 4..9;
    editor
        .set_proofing_marks(std::slice::from_ref(&misspelled))
        .unwrap();
    assert_eq!(snapshot, editor.snapshot());
    assert!(!editor.is_modified());
    editor.set_read_only(true);
    assert!(editor.replace_current("no").is_err());
    assert!(editor.execute(EditCommand::Undo).is_err());
    editor.select_range(0..3).unwrap();
    assert_eq!(editor.selected_text().unwrap(), "let");
}

#[test]
fn replace_all_is_one_unicode_aware_undoable_edit() {
    let editor = Editor::new().unwrap();
    editor.set_text("CAFÉ café cafeteria café").unwrap();
    assert_eq!(
        editor.replace_all("café", "😀\nnew", false, true).unwrap(),
        3
    );
    assert_eq!(editor.text().unwrap(), "😀\nnew 😀\nnew cafeteria 😀\nnew");
    editor.execute(EditCommand::Undo).unwrap();
    assert_eq!(editor.text().unwrap(), "CAFÉ café cafeteria café");
}

#[test]
fn line_movement_keeps_native_mark_provenance() {
    let editor = Editor::new().unwrap();
    editor.set_text("bold\nplain\n").unwrap();
    editor.select_range(0..4).unwrap();
    editor.execute(EditCommand::Bold).unwrap();
    let edit = crate::editing::plan(
        &editor.text().unwrap(),
        0..4,
        crate::editing::LineAction::MoveDown,
        "",
    )
    .unwrap()
    .unwrap();
    editor.edit_lines(edit).unwrap();
    assert_eq!(editor.text().unwrap(), "plain\nbold\n");
    assert!(editor.html().unwrap().contains("<strong>bold</strong>"));
    editor.execute(EditCommand::Undo).unwrap();
    assert_eq!(editor.text().unwrap(), "bold\nplain\n");
}

#[test]
fn completion_popup_opens_without_editing_and_accepts_the_selected_candidate() {
    let editor = Editor::new().unwrap();
    editor.set_text("alphabet alphanumeric\nalp").unwrap();
    let caret = editor.text_length();
    editor.select_range(caret..caret).unwrap();
    editor.mark_saved();
    let snapshot = editor.snapshot();
    editor.complete().unwrap();
    let (completion, selected) = editor.completion_state().unwrap();
    assert_eq!(selected, 0);
    assert!(completion.candidates.len() >= 2);
    assert_eq!(snapshot, editor.snapshot());
    assert!(!editor.is_modified());
    editor.move_completion(1);
    let (completion, selected) = editor.completion_state().unwrap();
    let expected = completion.candidates[selected].clone();
    editor.accept_completion(None).unwrap();
    assert!(editor.text().unwrap().ends_with(&expected));
    assert!(editor.completion_state().is_none());
    editor.execute(EditCommand::Undo).unwrap();
    assert!(editor.text().unwrap().ends_with("alp"));
}
#[test]
fn automatic_completion_requires_three_graphemes_and_escape_stays_dismissed() {
    let editor = Editor::new().unwrap();
    editor.native_session().set_focused(true);
    editor.set_text("alphabet\nal").unwrap();
    let caret = editor.text_length();
    editor.select_range(caret..caret).unwrap();
    editor.refresh_completion();
    assert!(editor.completion_state().is_none());
    editor.replace_current("p").unwrap();
    editor.refresh_completion();
    assert!(editor.completion_state().is_some());
    editor.dismiss_completion();
    editor.refresh_completion();
    assert!(editor.completion_state().is_none());
    editor.complete().unwrap();
    assert!(editor.completion_state().is_some());
}

#[test]
fn paragraph_styles_survive_native_recovery_and_html_roundtrip() {
    let editor = Editor::new().unwrap();
    editor.set_text("مرحبا 😀").unwrap();
    editor.execute(EditCommand::LineSpacing(1.5)).unwrap();
    editor.execute(EditCommand::Indent(480)).unwrap();
    editor.execute(EditCommand::Direction(true)).unwrap();
    let html = editor.html().unwrap();
    let restored = Editor::new().unwrap();
    restored.set_snapshot(&editor.snapshot().unwrap()).unwrap();
    assert_eq!(restored.text().unwrap(), "مرحبا 😀");
    assert_eq!(restored.html().unwrap(), html);

    let parsed = DomParser::from_schema(restored.manager.borrow().schema())
        .parse(&html, &ParseOptions::default());
    let attrs = parsed.first_child().expect("formatted paragraph").attrs();
    assert_eq!(attrs.get("lineHeight").and_then(Value::as_f64), Some(1.5));
    for (key, expected) in [("indent", "24px"), ("direction", "rtl")] {
        assert_eq!(
            attrs.get(key).and_then(Value::as_str),
            Some(expected),
            "{key}"
        );
    }
}

#[cfg(windows)]
mod mounted_surface {
    use crate::editor::{Editor, surface::surface};
    use nsh_core::{
        Size,
        element::Root,
        platform::{Idiom, Os},
        prelude::*,
    };
    use nsh_engine::EngineConfig;
    use nsh_testing::Headless;
    use std::{rc::Rc, time::Duration};

    #[derive(Clone, PartialEq, Props)]
    struct SurfaceProps {
        plain: bool,
    }

    #[component]
    fn MountedSurface(props: SurfaceProps) -> impl IntoElement {
        // The editor and its native session are created on the engine thread,
        // just as they are in the application, rather than passed across threads.
        let editor = Rc::new(Editor::new().expect("native editor"));
        editor.set_plain_text_mode(props.plain).unwrap();
        let observed = editor.clone();
        let application_keys = signal(0_u32);
        let editor_focused = signal(false);
        let key_editor = editor.clone();
        set_interval(Duration::from_millis(10), move || {
            editor_focused.set(key_editor.native_session().is_focused())
        });
        effect(move || {
            editor_focused.get();
            on_key_with(
                "mod+=",
                KeyOptions {
                    when: nsh_core::keymap::When::Always,
                    ..Default::default()
                },
                move |_| application_keys.update(|n| *n += 1),
            );
        });
        let shown = signal((String::new(), String::new(), false));
        set_interval(Duration::from_millis(10), move || {
            shown.set((
                observed.text().unwrap(),
                observed.html().unwrap(),
                observed.native_session().is_focused(),
            ));
        });
        view()
            .flex_col()
            .child(view().w(px(600.)).h(px(360.)).child(surface(editor)))
            .child(text(move || format!("Document {:?}", shown.get().0)))
            .child(text(move || format!("Markup {}", shown.get().1)))
            .child(text(move || format!("Editor focus {}", shown.get().2)))
            .child(text(move || {
                format!("Application zoom keys {}", application_keys.get())
            }))
    }

    #[test]
    fn mounted_native_surface_accepts_initial_plain_and_rich_typing_and_undo() {
        for plain in [true, false] {
            let root = Root::new::<MountedSurface>(move || SurfaceProps { plain });
            let mut config = EngineConfig {
                viewport: Size::new(px(760.), px(620.)),
                ..EngineConfig::new(crate::fonts::configure_root(root))
            };
            config.environment.os = Os::Windows;
            config.environment.idiom = Idiom::Desktop;
            let mut app = Headless::start(config).expect("mounted native surface");
            // No click, type_text(), manual focus or model edit is allowed before
            // this assertion: removing auto_focus must make the regression fail.
            app.wait_for("text:Editor focus true").unwrap();
            app.press_key("ctrl+=").unwrap();
            app.wait_for("text:Application zoom keys 1").unwrap();
            if !plain {
                app.press_key("ctrl+b").unwrap();
            }
            for key in ["c", "a", "f"] {
                app.press_key(key).unwrap();
            }
            app.wait_for("text:Document \"caf\"").unwrap();
            app.type_text("label:Document editor", "é 😀").unwrap();
            app.wait_for("text:Document \"café 😀\"").unwrap();
            if !plain {
                app.wait_for("text:/<strong>café 😀/").unwrap();
            }
            app.press_key("ctrl+z").unwrap();
            app.wait_for("text:Document \"\"").unwrap();
        }
    }
}

#[test]
fn font_attributes_accept_numeric_sizes_and_strip_css_family_quotes() {
    assert_eq!(super::font_size(&serde_json::json!(12)), Some(12.));
    assert_eq!(super::font_size(&serde_json::json!(" 12pt ")), Some(12.));
    for value in [
        serde_json::json!("NaN"),
        serde_json::json!(0),
        serde_json::json!(2000),
    ] {
        assert_eq!(super::font_size(&value), None);
    }
    assert_eq!(super::font_family("'Pretendard Std'"), "Pretendard Std");
    assert_eq!(super::font_family("Maple Mono NF"), "Maple Mono NF");
    let editor = Editor::new().unwrap();
    assert_eq!(editor.formatting().size, 12.);
    editor.set_plain_text_mode(true).unwrap();
    assert_eq!(editor.formatting().size, 12.);
}

#[test]
fn selections_reuse_the_document_text_map_and_preserve_snapshots() {
    let editor = Editor::new().unwrap();
    editor.set_text(&"A😀 café\n".repeat(1000)).unwrap();
    editor.mark_saved();
    let first = editor.map();
    let snapshot = editor.snapshot();
    for range in [0..1, 1..3, 4..8, 0..8000] {
        editor.select_range(range).unwrap();
        assert!(Arc::ptr_eq(&first, &editor.map()));
    }
    assert_eq!(snapshot, editor.snapshot());
    assert!(!editor.is_modified());
    editor.select_range(0..1).unwrap();
    editor.replace_current("B").unwrap();
    assert!(!Arc::ptr_eq(&first, &editor.map()));
}

#[test]
fn native_split_views_share_edits_format_and_history_but_keep_selections() {
    let editor = Editor::new().unwrap();
    editor.set_text("A😀 café").unwrap();
    editor.mark_saved();
    let peer = editor.split_view().unwrap();
    assert!(editor.same_document(&peer));
    editor.select_range(0..1).unwrap();
    peer.select_range(4..8).unwrap();
    assert_eq!(editor.selected_text().unwrap(), "A");
    assert_eq!(peer.selected_text().unwrap(), "café");
    peer.replace_current("tea").unwrap();
    assert_eq!(editor.text().unwrap(), "A😀 tea");
    assert_eq!(peer.text().unwrap(), editor.text().unwrap());
    editor.execute(EditCommand::Undo).unwrap();
    assert_eq!(peer.text().unwrap(), "A😀 café");
    peer.set_plain_text_mode(true).unwrap();
    assert!(editor.is_plain());
    peer.execute(EditCommand::FontSize(18.)).unwrap();
    assert_eq!(editor.formatting().size, 18.);
    assert_eq!(peer.snapshot(), editor.snapshot());
}

#[test]
fn line_number_settings_never_change_document_or_undo_content() {
    let editor = Editor::new().unwrap();
    editor.set_text("one\ntwo\n").unwrap();
    editor.mark_saved();
    let snapshot = editor.snapshot();
    let lines = editor.logical_line_positions();
    assert_eq!(lines.len(), 3);
    let style = editor.style_revision();
    let gutter = editor.gutter_revision();
    editor.set_line_numbers(true, "Inter", 16., 72.);
    assert_eq!(editor.style_revision(), style);
    assert_ne!(editor.gutter_revision(), gutter);
    assert_eq!(snapshot, editor.snapshot());
    assert!(!editor.is_modified());
    assert!(Arc::ptr_eq(&lines, &editor.logical_line_positions()));
}

#[test]
fn plain_output_preserves_spaces_tabs_and_chosen_font_size() {
    let editor = Editor::new().unwrap();
    editor.set_plain_text_mode(true).unwrap();
    editor.set_text("  indented\tvalue\nnext").unwrap();
    editor.execute(EditCommand::FontSize(14.)).unwrap();
    let snapshot = editor.document_snapshot().unwrap();
    assert!(snapshot.html.contains("white-space:pre-wrap"));
    assert!(snapshot.html.contains("font-size:14pt"));
    assert!(snapshot.html.contains("  indented\tvalue"));
}

#[test]
fn rejected_recovery_preserves_document_and_page_geometry() {
    let editor = Editor::new().unwrap();
    editor.set_text("keep this document").unwrap();
    let before = editor.snapshot();
    let mut page = editor.page_settings();
    page.width_twips += 720;
    let bad = serde_json::json!({"doc":{"type":"doc","content":[{"type":"image","attrs":{"src":"https://example.org/never-fetch.png"}}]},"page":page});
    assert!(
        editor
            .set_snapshot(&serde_json::to_vec(&bad).unwrap())
            .is_err()
    );
    assert_eq!(before, editor.snapshot());
}

#[test]
fn formatting_across_large_selection_is_fast_and_accurate() {
    let editor = Editor::new().unwrap();
    let text = "Paragraph line with text content to test selection performance.\n".repeat(500);
    editor.set_text(&text).unwrap();
    editor.select_range(0..text.len()).unwrap();
    let start = std::time::Instant::now();
    let format = editor.formatting();
    let elapsed = start.elapsed();
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "formatting took {elapsed:?}, expected < 50ms"
    );
    assert_eq!(format.size, 12.);
    assert!(!format.bold);
    assert!(!format.italic);
}

#[test]
fn block_styles_line_numbers_replace_one_and_pt_indents_roundtrip_cleanly() {
    let editor = Editor::new().unwrap();
    editor
        .set_text("First heading\n😀 alpha\nalpha end")
        .unwrap();

    editor.select_range(0..13).unwrap();
    assert_eq!(editor.line_at_selection(), 1);
    editor
        .execute(EditCommand::BlockStyle(BlockStyle::Heading(2)))
        .unwrap();
    assert_eq!(editor.formatting().block_style, BlockStyle::Heading(2));

    editor
        .execute(EditCommand::BlockStyle(BlockStyle::Blockquote))
        .unwrap();
    assert_eq!(editor.formatting().block_style, BlockStyle::Blockquote);

    editor
        .execute(EditCommand::BlockStyle(BlockStyle::CodeBlock))
        .unwrap();
    assert_eq!(editor.formatting().block_style, BlockStyle::CodeBlock);

    editor
        .execute(EditCommand::BlockStyle(BlockStyle::Body))
        .unwrap();
    assert_eq!(editor.formatting().block_style, BlockStyle::Body);

    editor.select_range(14..14).unwrap();
    assert_eq!(editor.line_at_selection(), 2);
    assert!(editor.replace_one("alpha", "beta", true, true).unwrap());
    assert_eq!(editor.text().unwrap(), "First heading\n😀 beta\nalpha end");
    assert_eq!(editor.selected_text().unwrap(), "alpha");
    assert_eq!(editor.line_at_selection(), 3);
    assert!(editor.replace_one("alpha", "omega", true, true).unwrap());
    assert_eq!(editor.text().unwrap(), "First heading\n😀 beta\nomega end");
    assert!(!editor.replace_one("alpha", "none", true, true).unwrap());

    editor
        .set_document(Document {
            html: "<p style=\"margin-left:18pt\">Indented</p>".into(),
            page: PageSettings::default(),
        })
        .unwrap();
    editor.select_range(0..0).unwrap();
    editor.execute(EditCommand::Indent(360)).unwrap();
    let html = editor.html().unwrap();
    assert!(
        html.contains("margin-left:42px") || html.contains("margin-left: 42px"),
        "unexpected html after pt+twip indent: {html}"
    );
}
