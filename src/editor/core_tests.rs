use super::*;

fn editor(text: &str) -> Editor {
    let editor = Editor::new().expect("WordCraft session");
    editor.set_text(text).unwrap();
    editor.mark_saved();
    editor
}

#[test]
fn unicode_selection_and_replacement_preserve_complete_graphemes() {
    let editor = editor("A😀 café\nsecond");
    editor.select_range(1..3).unwrap();
    assert_eq!(editor.selected_text().unwrap(), "😀");
    assert!(editor.select_range(2..3).is_err());
    editor.replace_current("🦀").unwrap();
    assert_eq!(editor.text().unwrap(), "A🦀 café\nsecond");
    editor.execute(EditCommand::Undo).unwrap();
    assert_eq!(editor.text().unwrap(), "A😀 café\nsecond");
}

#[test]
fn formatting_is_undoable_and_buffer_acknowledgement_is_not_a_save() {
    let editor = editor("words");
    editor.execute(EditCommand::SelectAll).unwrap();
    editor.execute(EditCommand::Bold).unwrap();
    assert!(editor.formatting().bold);
    assert!(editor.is_modified());
    assert!(editor.has_buffered_changes());
    editor.acknowledge_buffered_changes();
    assert!(!editor.has_buffered_changes());
    assert!(editor.is_modified());
    editor.execute(EditCommand::Undo).unwrap();
    assert!(!editor.formatting().bold);
    editor.execute(EditCommand::Redo).unwrap();
    assert!(editor.formatting().bold);
}

#[test]
fn rich_html_paste_is_one_undo_step_and_bad_imports_are_atomic() {
    let editor = editor("before ");
    editor.select_range(7..7).unwrap();
    editor.insert_html("<p><b>bold café 😀</b></p>").unwrap();
    assert!(editor.text().unwrap().contains("bold café 😀"));
    editor.execute(EditCommand::Undo).unwrap();
    assert_eq!(editor.text().unwrap(), "before ");
    let document = editor.snapshot();
    assert!(editor.set_rtf(br"{\rtf1 truncated").is_err());
    assert_eq!(editor.snapshot(), document);
    assert!(editor.insert_html("bad\0paste").is_err());
    assert_eq!(editor.snapshot(), document);
}

#[test]
fn dark_display_and_syntax_do_not_change_saved_or_recovered_styles() {
    let editor = editor("let value = 1;\n");
    editor.set_plain_text_mode(true).unwrap();
    editor
        .execute(EditCommand::Font(crate::fonts::PLAIN_FONT.into()))
        .unwrap();
    editor.mark_saved();
    let saved = editor.rtf().unwrap();
    let native = editor.snapshot();
    editor.set_background(0x1e1e1e);
    editor.set_text_override(Some(0xeeeeee)).unwrap();
    editor
        .apply_spans(&[crate::syntax::Span {
            start: 0,
            len: 3,
            color: 0xff4488,
            bold: true,
            italic: false,
        }])
        .unwrap();
    let display = editor.display_snapshot();
    assert_ne!(display, native);
    assert_eq!(editor.snapshot(), native);
    assert_eq!(editor.rtf().unwrap(), saved);
    assert!(!editor.is_modified());
    let restored = Editor::new().unwrap();
    restored.set_document(native.clone()).unwrap();
    assert_eq!(restored.snapshot(), native);
    assert_eq!(restored.text().unwrap(), editor.text().unwrap());
}

#[test]
fn rtf_and_native_snapshot_keep_tables_and_formatting() {
    let editor = editor("content");
    editor.execute(EditCommand::SelectAll).unwrap();
    editor.execute(EditCommand::Italic).unwrap();
    editor.select_range(7..7).unwrap();
    editor.insert_table(2, 2).unwrap();
    let native = editor.snapshot();
    let snapshot = serde_json::to_vec(&native).unwrap();
    let restored = Editor::new().unwrap();
    restored
        .set_document(serde_json::from_slice(&snapshot).unwrap())
        .unwrap();
    assert_eq!(restored.snapshot(), native);
    let rtf = editor.rtf().unwrap();
    restored.set_rtf(&rtf).unwrap();
    assert!(
        restored
            .snapshot()
            .body
            .iter()
            .any(|block| matches!(block.as_ref(), wordcraft_doc::Block::Table(_)))
    );
    assert!(restored.text().unwrap().contains("content"));
}

#[test]
fn search_and_replace_respect_utf16_positions_and_single_undo() {
    let editor = editor("😀 beta BETA beta");
    editor.select_range(0..0).unwrap();
    assert!(editor.find("beta", true, true, false).unwrap());
    assert_eq!(editor.selection(), 3..7);
    assert_eq!(editor.replace_all("beta", "word", false, true).unwrap(), 3);
    assert_eq!(editor.text().unwrap(), "😀 word word word");
    editor.execute(EditCommand::Undo).unwrap();
    assert_eq!(editor.text().unwrap(), "😀 beta BETA beta");
}

#[test]
fn page_setup_and_completion_preferences_invalidate_display_once() {
    let editor = editor("page");
    let page = PageSettings {
        width_twips: 14400,
        ..PageSettings::default()
    };
    editor.set_page_settings(&page).unwrap();
    assert_eq!(editor.page_settings().width_twips, 14400);
    editor.execute(EditCommand::Undo).unwrap();
    assert_ne!(editor.page_settings().width_twips, 14400);
    let revision = editor.revision();
    editor.set_autocomplete_enabled(false);
    assert_ne!(editor.revision(), revision);
    let revision = editor.revision();
    editor.set_autocomplete_enabled(false);
    assert_eq!(editor.revision(), revision);
}
