//! Native resizable editor panes with independently retained editing views.
use crate::{
    app::{Command, MenuKind},
    dialogs::Commands,
    editor::Editor,
    ui::{self, Palette, px},
};
use nsh_base::{h_resizable, resizable_panel, v_resizable};
use nsh_core::prelude::*;
use std::rc::Rc;

/// The supported arrangements of independently selectable document panes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Layout {
    #[default]
    Single,
    Columns2,
    Rows2,
    Columns3,
    Rows3,
    Grid4,
    Columns4,
    Rows4,
}
impl Layout {
    pub const fn count(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Columns2 | Self::Rows2 => 2,
            Self::Columns3 | Self::Rows3 => 3,
            Self::Grid4 | Self::Columns4 | Self::Rows4 => 4,
        }
    }
    pub const fn label(self) -> &'static str {
        match self {
            Self::Single => "Single Pane",
            Self::Columns2 => "2 Columns",
            Self::Rows2 => "2 Rows",
            Self::Columns3 => "3 Columns",
            Self::Rows3 => "3 Rows",
            Self::Grid4 => "2 × 2 Grid",
            Self::Columns4 => "4 Columns",
            Self::Rows4 => "4 Rows",
        }
    }
}

pub(crate) struct Pane {
    pub document_id: String,
    pub editor: Rc<Editor>,
}

type ReadPane = Rc<dyn Fn(usize) -> Option<(String, Rc<Editor>, bool)>>;

#[derive(Clone)]
struct EditorMount(Rc<Editor>);
impl PartialEq for EditorMount {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}

fn pane(
    index: usize,
    read: ReadPane,
    revision: Signal<u64>,
    commands: Commands,
    colors: Signal<Palette>,
) -> AnyElement {
    let active_border = read.clone();
    let active_header = read.clone();
    let title = read.clone();
    let body = read.clone();
    let focus_reader = read.clone();
    let choose = commands.clone();
    view()
        .key(format!("document-pane-{index}"))
        .flex_col()
        .size_full()
        .min_w(px(0.))
        .min_h(px(0.))
        .overflow_hidden()
        .border_1()
        .border_color(move || {
            revision.get();
            let colors = colors.get();
            ui::rgb(
                if active_border(index).is_some_and(|(_, _, active)| active) {
                    colors.accent
                } else {
                    colors.border
                },
            )
        })
        .child(
            view()
                .h(px(26.))
                .flex_shrink(0.)
                .min_w(px(0.))
                .bg(move || ui::rgb(colors.get().toolbar))
                .border_b_1()
                .border_color(move || ui::rgb(colors.get().border))
                .child(
                    nsh_base::button()
                        .key(format!("pane-document-{index}"))
                        .accessibility_label(format!("Choose document for pane {}", index + 1))
                        .size_full()
                        .min_w(px(0.))
                        .px(px(8.))
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(6.))
                        .text_color(move || ui::rgb(colors.get().ink))
                        .selected(move || {
                            revision.get();
                            active_header(index).is_some_and(|(_, _, active)| active)
                        })
                        .hover(move |style| style.bg(ui::rgb(colors.get().field)))
                        .focus_visible(move |style| {
                            style
                                .outline(px(2.))
                                .outline_color(ui::rgb(colors.get().accent))
                        })
                        .child(
                            text(move || {
                                revision.get();
                                title(index)
                                    .map(|(name, _, _)| name)
                                    .unwrap_or_else(|| "No document".into())
                            })
                            .text_size(px(11.))
                            .line_height(px(16.))
                            .truncate()
                            .flex_1()
                            .min_w(px(0.)),
                        )
                        .child(
                            text("▾")
                                .text_size(px(10.))
                                .text_color(move || ui::rgb(colors.get().muted)),
                        )
                        .on_press(move || {
                            let mut commands = choose.borrow_mut();
                            commands.push_back(Command::FocusPane(index));
                            commands.push_back(Command::Menu(MenuKind::PaneTabs(index)));
                        }),
                ),
        )
        .child(
            view()
                .flex_1()
                .min_w(px(0.))
                .min_h(px(0.))
                .overflow_hidden()
                // A title, focus or theme update cannot replace a retained editor's native view.
                .child(each(
                    move || {
                        revision.get();
                        body(index)
                            .map(|(_, editor, _)| EditorMount(editor))
                            .into_iter()
                            .collect::<Vec<_>>()
                    },
                    |mount: &EditorMount| Rc::as_ptr(&mount.0) as usize,
                    move |mount, _| {
                        let editor = mount.get_untracked().0;
                        let commands = commands.clone();
                        let active = focus_reader.clone();
                        crate::editor::surface::pane_surface(
                            editor,
                            move || {
                                commands.borrow_mut().push_back(Command::FocusPane(index));
                            },
                            move || {
                                revision.get();
                                active(index).is_some_and(|(_, _, active)| active)
                            },
                        )
                    },
                )),
        )
        .into_element()
}

fn split(columns: bool, panes: Vec<AnyElement>, colors: Signal<Palette>) -> AnyElement {
    let group = if columns {
        h_resizable()
    } else {
        v_resizable()
    };
    group
        .size_full()
        .min_w(px(0.))
        .min_h(px(0.))
        .handle_accessibility_label(if columns {
            "Resize document pane widths"
        } else {
            "Resize document pane heights"
        })
        .handle_style(move |style| style.bg(ui::rgb(colors.get().border)))
        // Unspecified sizes share the available space equally until a handle is dragged.
        .children(panes.into_iter().map(move |pane| {
            resizable_panel()
                .min_size(px(if columns { 64. } else { 48. }))
                .child(pane)
        }))
        .into_element()
}

pub(crate) fn render(
    layout: Layout,
    read: ReadPane,
    revision: Signal<u64>,
    commands: Commands,
    colors: Signal<Palette>,
) -> AnyElement {
    let mut panes = (0..layout.count())
        .map(|index| pane(index, read.clone(), revision, commands.clone(), colors));
    match layout {
        Layout::Single => panes.next().unwrap_or_else(|| view().into_element()),
        Layout::Grid4 => {
            let top = split(true, panes.by_ref().take(2).collect(), colors);
            let bottom = split(true, panes.collect(), colors);
            split(false, vec![top, bottom], colors)
        }
        Layout::Columns2 | Layout::Columns3 | Layout::Columns4 => {
            split(true, panes.collect(), colors)
        }
        Layout::Rows2 | Layout::Rows3 | Layout::Rows4 => split(false, panes.collect(), colors),
    }
}

#[cfg(test)]
mod tests {
    use super::Layout;
    #[test]
    fn layouts_have_the_expected_cardinality_and_distinct_menu_labels() {
        let layouts = [
            (Layout::Single, 1),
            (Layout::Columns2, 2),
            (Layout::Rows2, 2),
            (Layout::Columns3, 3),
            (Layout::Rows3, 3),
            (Layout::Grid4, 4),
            (Layout::Columns4, 4),
            (Layout::Rows4, 4),
        ];
        for (index, (layout, count)) in layouts.iter().enumerate() {
            assert_eq!(layout.count(), *count, "layout {layout:?}");
            assert!(!layout.label().is_empty());
            assert!(
                layouts[..index]
                    .iter()
                    .all(|(previous, _)| previous.label() != layout.label())
            );
        }
        assert_eq!(Layout::default(), Layout::Single);
    }
}
