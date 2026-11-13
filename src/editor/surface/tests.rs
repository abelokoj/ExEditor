use super::{input::utf16_range, local_command, render_image};

#[test]
fn native_ranges_keep_surrogate_pairs_intact() {
    let text = "A😀e\u{301}日";
    assert_eq!(utf16_range(text, 1..3), (1..3, 1..5));
    assert_eq!(utf16_range(text, 2..2), (1..3, 1..5));
    assert_eq!(utf16_range(text, 0..usize::MAX), (0..6, 0..text.len()));
    assert_eq!(utf16_range(text, 5..5), (5..5, 8..8));
    assert_eq!(utf16_range(text, 99..100), (6..6, text.len()..text.len()));
    assert_eq!(utf16_range("", 0..1), (0..0, 0..0));
}

#[test]
fn document_shortcuts_remain_available_to_the_window() {
    for key in [
        "s", "o", "n", "w", "z", "c", "x", "v", "a", "k", "tab", "up", "down", "pageup",
        "pagedown", "enter",
    ] {
        assert_eq!(
            local_command(key, true, false, false),
            None,
            "Ctrl/Cmd+{key} must bubble"
        );
    }
    assert_eq!(
        local_command("left", true, true, false),
        Some("caret.wordLeft")
    );
    assert_eq!(
        local_command("home", true, true, false),
        Some("caret.docStart")
    );
    assert_eq!(
        local_command("backspace", true, false, false),
        Some("text.deleteWordBack")
    );
    assert_eq!(
        local_command("left", true, false, true),
        None,
        "AltGr must reach the input method"
    );
    assert_eq!(local_command("delete", false, true, false), None);
    assert_eq!(
        local_command("enter", false, true, false),
        Some("text.lineBreak")
    );
}

#[test]
fn raster_channels_match_gpui_without_losing_alpha() {
    let image = render_image(wordcraft_render::Rendered {
        width: 2,
        height: 1,
        pixels: vec![0x40, 0x20, 0x10, 0x80, 0xff, 0x80, 0, 0xff],
    })
    .expect("a well-sized raster must become a GPUI image");
    assert_eq!(
        image.as_bytes(0),
        Some([0x10, 0x20, 0x40, 0x80, 0, 0x80, 0xff, 0xff].as_slice())
    );
    assert!(
        render_image(wordcraft_render::Rendered {
            width: 1,
            height: 1,
            pixels: vec![0]
        })
        .is_none()
    );
    assert!(render_image(wordcraft_render::Rendered::default()).is_none());
}

use super::{Editor, EditorSurface};
use gpui_kit::{
    AppContext as _, Context, Entity, FocusHandle, InteractiveElement as _, IntoElement,
    KeyDownEvent, Keystroke, Modifiers, ParentElement as _, Render, Styled as _, TestAppContext,
    Window, div,
};
use std::{cell::RefCell, rc::Rc, time::Instant};

struct Harness {
    surface: Entity<EditorSurface>,
    focus: FocusHandle,
    actions: Rc<RefCell<Vec<String>>>,
    keys: crate::keyboard::State,
    execute: bool,
}
impl Render for Harness {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .track_focus(&self.focus)
            .on_key_down(cx.listener(|this, event: &KeyDownEvent, window, cx| {
                if let crate::keyboard::Outcome::Action(action) = this
                    .keys
                    .resolve_keystroke(&event.keystroke, Instant::now())
                {
                    this.actions.borrow_mut().push(action.into());
                    if this.execute
                        && let Some(crate::app::Command::Edit(command)) =
                            crate::app::shortcut_command(action)
                    {
                        this.surface.read(cx).editor.execute(command).unwrap();
                    }
                    cx.stop_propagation();
                    window.prevent_default();
                }
            }))
            .child(self.surface.clone())
    }
}
fn setup(
    cx: &mut TestAppContext,
    editor: Rc<Editor>,
    execute: bool,
) -> (
    Entity<Harness>,
    &mut gpui_kit::VisualTestContext,
    Rc<RefCell<Vec<String>>>,
) {
    cx.update(gpui_kit::init);
    let actions = Rc::new(RefCell::new(Vec::new()));
    let recorded = actions.clone();
    let (view, cx) = cx.add_window_view(move |window, cx| {
        let surface = cx.new(|cx| EditorSurface::new(editor, cx));
        let focus = surface.read(cx).focus.clone();
        window.focus(&focus, cx);
        Harness {
            surface,
            focus: cx.focus_handle(),
            actions: recorded,
            keys: crate::keyboard::State::default(),
            execute,
        }
    });
    (view, cx, actions)
}
fn stroke(binding: &crate::keyboard::Binding, platform: bool) -> Keystroke {
    let key = match binding.key {
        n @ 0x41..=0x5a => char::from_u32(n).unwrap().to_ascii_lowercase().to_string(),
        n @ 0x30..=0x39 => char::from_u32(n).unwrap().to_string(),
        n @ 0x70..=0x87 => format!("f{}", n - 0x6f),
        0x08 => "backspace".into(),
        0x09 => "tab".into(),
        0x0d => "enter".into(),
        0x1b => "escape".into(),
        0x20 => "space".into(),
        0x21 => "pageup".into(),
        0x22 => "pagedown".into(),
        0x23 => "end".into(),
        0x24 => "home".into(),
        0x25 => "left".into(),
        0x26 => "up".into(),
        0x27 => "right".into(),
        0x28 => "down".into(),
        0x2d => "insert".into(),
        0x2e => "delete".into(),
        0x6b => "numpadadd".into(),
        0x6d => "numpadsubtract".into(),
        0xba => ";".into(),
        0xbb => "=".into(),
        0xbc => ",".into(),
        0xbd => "-".into(),
        0xbe => ".".into(),
        0xbf => "/".into(),
        0xc0 => "`".into(),
        0xdb => "[".into(),
        0xdc => "\\".into(),
        0xdd => "]".into(),
        0xde => "'".into(),
        key => panic!("unmapped catalog key {key:#x}"),
    };
    Keystroke {
        key,
        key_char: None,
        modifiers: Modifiers {
            control: binding.modifiers.control && !platform,
            platform: binding.modifiers.control && platform,
            shift: binding.modifiers.shift,
            alt: binding.modifiers.alt,
            ..Default::default()
        },
    }
}

#[gpui_kit::test]
fn every_application_shortcut_reaches_the_focused_window(cx: &mut TestAppContext) {
    let editor = Rc::new(Editor::new().unwrap());
    editor.set_text("a document").unwrap();
    let (view, cx, actions) = setup(cx, editor, false);
    for platform in [false, true] {
        for binding in crate::keyboard::BINDINGS
            .iter()
            .filter(|binding| !matches!(binding.action, "chord" | "complete"))
        {
            view.update(cx, |view, _| view.keys.reset());
            actions.borrow_mut().clear();
            if binding.chord {
                cx.simulate_keystrokes(if platform { "cmd-k" } else { "ctrl-k" });
            }
            let keystroke = stroke(binding, platform);
            cx.update(|window, cx| {
                window.dispatch_keystroke(keystroke, cx);
            });
            cx.run_until_parked();
            assert_eq!(
                actions.borrow().as_slice(),
                [binding.action],
                "{binding:?}, platform={platform}"
            );
        }
    }
}

#[gpui_kit::test]
fn typing_formatting_navigation_and_undo_use_native_input(cx: &mut TestAppContext) {
    let editor = Rc::new(Editor::new().unwrap());
    let (_, cx, _) = setup(cx, editor.clone(), true);
    cx.simulate_input("abc");
    assert_eq!(editor.text().unwrap(), "abc");
    cx.simulate_keystrokes("ctrl-a ctrl-b");
    assert!(editor.formatting().bold);
    cx.simulate_keystrokes("ctrl-z");
    assert!(!editor.formatting().bold);
    cx.simulate_keystrokes("end left backspace");
    assert_eq!(editor.text().unwrap(), "ac");
}

#[gpui_kit::test]
fn completion_accepts_tab_and_escape_does_not_insert_a_tab(cx: &mut TestAppContext) {
    let editor = Rc::new(Editor::new().unwrap());
    editor.set_text("example exa").unwrap();
    editor.select_range(11..11).unwrap();
    let (view, cx, _) = setup(cx, editor.clone(), false);
    view.update(cx, |view, cx| {
        view.surface.update(cx, |surface, _| {
            surface.focused = true;
            surface.refresh_completion(true);
        });
    });
    cx.simulate_keystrokes("tab");
    assert_eq!(editor.text().unwrap(), "example example");
    editor.set_text("example exa").unwrap();
    editor.select_range(11..11).unwrap();
    view.update(cx, |view, cx| {
        view.surface.update(cx, |surface, _| {
            surface.focused = true;
            surface.refresh_completion(true);
        });
    });
    cx.simulate_keystrokes("escape");
    assert_eq!(editor.text().unwrap(), "example exa");
}

#[gpui_kit::test]
fn native_composition_suppresses_suggestions_and_undoes_as_one_edit(cx: &mut TestAppContext) {
    use gpui_kit::EntityInputHandler as _;
    let editor = Rc::new(Editor::new().unwrap());
    editor.set_text("example ").unwrap();
    editor.select_range(8..8).unwrap();
    editor.mark_saved();
    let (view, cx, _) = setup(cx, editor.clone(), true);
    for text in ["exa", "exam"] {
        cx.update(|window, cx| {
            view.update(cx, |view, cx| {
                view.surface.update(cx, |surface, cx| {
                    surface.replace_and_mark_text_in_range(None, text, None, window, cx);
                    assert!(surface.completion.is_none());
                })
            });
        });
    }
    editor.set_background(0x202530);
    editor.set_text_override(Some(0xeeeeee)).unwrap();
    {
        use gpui_kit::test::TestWindowExt as _;
        cx.update(|window, cx| window.render_frame(cx));
    }
    view.update(cx, |view, cx| {
        assert!(
            view.surface.read(cx).is_composing(),
            "theme changes must not cancel native composition"
        )
    });
    assert_eq!(editor.text().unwrap(), "example exam");
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.surface.update(cx, |surface, cx| {
                surface.replace_text_in_range(None, "exam", window, cx);
                assert!(surface.marked.is_none());
            })
        });
    });
    cx.simulate_keystrokes("ctrl-z");
    assert_eq!(editor.text().unwrap(), "example ");
}

#[gpui_kit::test]
fn dark_theme_reaches_the_drawn_page_without_rewriting_the_document(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let editor = Rc::new(Editor::new().unwrap());
    editor.set_text("Theme sample").unwrap();
    editor.mark_saved();
    let original = editor.snapshot();
    let (view, cx, _) = setup(cx, editor.clone(), false);
    editor.set_background(0x202530);
    editor.set_text_override(Some(0xeeeeee)).unwrap();
    cx.update(|window, cx| window.render_frame(cx));
    view.update(cx, |view, cx| {
        view.surface.update(cx, |surface, _| {
            assert_eq!(surface.palette().paper, crate::ui::Palette::new(true).paper);
            assert!(
                surface
                    .cache
                    .values()
                    .any(|image| image.as_bytes(0).is_some_and(|bytes| bytes
                        .as_chunks::<4>()
                        .0
                        .iter()
                        .any(|pixel| pixel == &[0x30, 0x25, 0x20, 0xff])))
            );
        })
    });
    assert_eq!(editor.snapshot(), original);
    assert!(!editor.is_modified());
}

#[gpui_kit::test]
fn moving_the_caret_does_not_open_suggestions_and_manual_completion_survives_the_toggle(
    cx: &mut TestAppContext,
) {
    use gpui_kit::test::TestWindowExt as _;
    let editor = Rc::new(Editor::new().unwrap());
    editor.set_text("example exa").unwrap();
    editor.select_range(11..11).unwrap();
    let (view, cx, _) = setup(cx, editor.clone(), false);
    cx.update(|window, cx| window.render_frame(cx));
    view.update(cx, |view, cx| {
        assert!(view.surface.read(cx).completion.is_none())
    });
    editor.set_autocomplete_enabled(false);
    cx.update(|window, cx| {
        view.update(cx, |view, cx| {
            view.surface
                .update(cx, |surface, cx| surface.complete(window, cx))
        })
    });
    cx.update(|window, cx| window.render_frame(cx));
    view.update(cx, |view, cx| {
        assert!(view.surface.read(cx).completion.is_some())
    });
    cx.simulate_keystrokes("tab");
    assert_eq!(editor.text().unwrap(), "example example");
}

#[gpui_kit::test]
fn long_document_tiles_release_gpu_entries_when_scrolled_and_invalidated(cx: &mut TestAppContext) {
    use gpui_kit::test::TestWindowExt as _;
    let editor = Rc::new(Editor::new().unwrap());
    editor
        .set_text(&"A line in a long Markdown-like document.\n".repeat(850))
        .unwrap();
    editor.set_wrap_to_page(true);
    let (view, cx, _) = setup(cx, editor.clone(), false);
    cx.update(|window, cx| window.render_frame(cx));
    assert!(editor.layout().pages.len() > 10);
    let old = view.update(cx, |view, cx| {
        view.surface
            .read(cx)
            .cache
            .values()
            .cloned()
            .collect::<Vec<_>>()
    });
    assert!(!old.is_empty());
    cx.update(|window, _| assert!(old.iter().all(|image| window.has_image_atlas_entry(image))));
    view.update(cx, |view, cx| {
        view.surface.update(cx, |surface, cx| {
            surface.scroll.y = gpui_kit::px(4_000.);
            cx.notify();
        })
    });
    cx.update(|window, cx| window.render_frame(cx));
    cx.update(|window, _| {
        assert!(
            old.iter().all(|image| !window.has_image_atlas_entry(image)),
            "keeping CPU Arcs alive must not keep off-screen GPU entries"
        )
    });
    let old = view.update(cx, |view, cx| {
        view.surface
            .read(cx)
            .cache
            .values()
            .cloned()
            .collect::<Vec<_>>()
    });
    editor.set_background(0x202530);
    cx.update(|window, cx| window.render_frame(cx));
    cx.update(|window, _| {
        assert!(
            old.iter().all(|image| !window.has_image_atlas_entry(image)),
            "re-layout and theme changes must release the old GPU tiles"
        )
    });
}
