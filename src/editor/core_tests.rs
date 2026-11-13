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
    editor.set_proofing_marks(&[4..9]).unwrap();
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
