//! A fixed native gutter follows editor geometry without entering the document schema.

use super::Editor;
use nsh_core::prelude::*;
use std::{ops::Range, rc::Rc, sync::Arc, time::Duration};

// shortcut: cap dense viewport work; replace with native virtualized gutter rows when NSH exposes them.
const MAX_CANDIDATES: usize = 4096;
const MAX_LABELS: usize = 2048;

#[derive(Clone, Debug, PartialEq)]
struct Settings {
    enabled: bool,
    width: f32,
    font: String,
    size: f32,
    background: u32,
    foreground: u32,
    border: u32,
}

impl Settings {
    fn read(editor: &Editor) -> Self {
        let display = editor.display_state();
        let background: Hsla = rgb(display.background & 0xffffff).into();
        let dark = background.luminance() < 0.25;
        Self {
            enabled: display.show_line_numbers && display.visible,
            width: if display.line_number_width.is_finite() && display.line_number_width > 0. {
                display.line_number_width
            } else {
                48.
            },
            font: if display.line_number_font.trim().is_empty() {
                crate::fonts::default_plain_font().into()
            } else {
                display.line_number_font
            },
            size: if display.line_number_size.is_finite() && display.line_number_size > 0. {
                display.line_number_size
            } else {
                12.
            } * f32::from(display.zoom)
                / 100.,
            background: display.background & 0xffffff,
            foreground: display
                .text_override
                .unwrap_or(if dark { 0xb6b6b6 } else { 0x606060 }),
            border: if dark { 0x545454 } else { 0xdddddd },
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct RowBox {
    top: f32,
    height: f32,
}

impl RowBox {
    fn native(bounds: Bounds<Points>) -> Option<Self> {
        let row = Self {
            top: bounds.origin.y.0,
            height: bounds.size.height.0,
        };
        (row.top.is_finite() && row.height.is_finite() && row.height > 0.).then_some(row)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Label {
    number: usize,
    top: f32,
    height: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamp {
    document: u64,
    style: (u64, u64),
    viewport: [u32; 4],
    anchors: [Option<(u32, u32)>; 2],
}

fn position_range(starts: &[usize], top: usize, bottom: usize) -> Range<usize> {
    let first = top.min(bottom);
    let last = top.max(bottom);
    let from = starts
        .partition_point(|&position| position <= first)
        .saturating_sub(2);
    let to = starts
        .partition_point(|&position| position <= last)
        .saturating_add(2)
        .min(starts.len());
    from..to
}

fn geometry_bound(
    starts: &[usize],
    target: f32,
    rect: &mut impl FnMut(usize) -> Option<RowBox>,
) -> Option<usize> {
    let (mut from, mut to) = (0, starts.len());
    while from < to {
        let middle = from + (to - from) / 2;
        let row = rect(starts[middle])?;
        if row.top < target {
            from = middle + 1;
        } else {
            to = middle;
        }
    }
    Some(from)
}

struct Visible {
    labels: Vec<Label>,
    anchors: [usize; 2],
}

fn visible_rows(
    starts: &[usize],
    top: f32,
    height: f32,
    hit_range: Option<(usize, usize)>,
    mut rect: impl FnMut(usize) -> Option<RowBox>,
) -> Visible {
    let range = hit_range
        .map(|(first, last)| position_range(starts, first, last))
        .or_else(|| {
            // Hit testing avoids assuming monotone Y coordinates inside multi-column tables.
            let from = geometry_bound(starts, top, &mut rect)?.saturating_sub(1);
            let to = geometry_bound(starts, top + height, &mut rect)?
                .saturating_add(1)
                .min(starts.len());
            Some(from..to)
        });
    let Some(range) = range else {
        return Visible {
            labels: Vec::new(),
            anchors: [0, starts.len().saturating_sub(1)],
        };
    };
    let mut labels = Vec::new();
    let mut anchors = [range.start, range.start];
    let mut first = true;
    for index in range.take(MAX_CANDIDATES) {
        let Some(row) = rect(starts[index]) else {
            continue;
        };
        if first {
            anchors[0] = index;
            first = false;
        }
        anchors[1] = index;
        if row.top + row.height > top && row.top < top + height {
            labels.push(Label {
                number: index + 1,
                top: row.top - top,
                height: row.height,
            });
        }
    }
    labels.sort_by(|a, b| a.top.total_cmp(&b.top).then(a.number.cmp(&b.number)));
    // A gutter has one column: coincident table-cell rows share the first logical label.
    labels.dedup_by(|next, previous| (next.top - previous.top).abs() < 0.5);
    labels.truncate(MAX_LABELS);
    Visible { labels, anchors }
}

fn anchor_stamp(
    starts: &[usize],
    indices: [usize; 2],
    rect: &mut impl FnMut(usize) -> Option<RowBox>,
) -> [Option<(u32, u32)>; 2] {
    indices.map(|index| {
        starts
            .get(index)
            .and_then(|&pos| rect(pos))
            .map(|row| (row.top.to_bits(), row.height.to_bits()))
    })
}

/// Places a fixed, noninteractive gutter beside the existing native editor surface.
/// The owner of this wrapper also owns and cancels its geometry-sampling task.
pub(super) fn wrap(
    editor: Rc<Editor>,
    content: AnyElement,
    revision: Signal<(u64, u64)>,
) -> AnyElement {
    let settings = signal(Settings::read(&editor));
    let rows = signal(Vec::<Label>::new());
    let content_ref = element_ref();
    let configured = editor.clone();
    effect(move || {
        revision.get();
        settings.set(Settings::read(&configured));
    });
    let sampled = editor.clone();
    // shortcut: poll geometry every 16 ms; replace with native scroll callbacks when NSH exposes them.
    spawn(async move {
        let mut cached_revision = None;
        let mut starts = Arc::new(Vec::new());
        let mut previous = None;
        let mut anchors = [0, 0];
        loop {
            timer(Duration::from_millis(16)).await;
            let config = settings.get_untracked();
            if !config.enabled {
                if previous.take().is_some() {
                    rows.set(Vec::new());
                }
                continue;
            }
            let Some(measurement) = content_ref.measure().await else {
                previous = None;
                rows.set(Vec::new());
                continue;
            };
            let height = measurement.frame.size.height.0;
            let width = measurement.frame.size.width.0;
            if !height.is_finite() || !width.is_finite() || height <= 0. || width <= 0. {
                continue;
            }
            let document = sampled.document_revision();
            if cached_revision != Some(document) {
                starts = sampled.logical_line_positions();
                cached_revision = Some(document);
                anchors = [0, starts.len().saturating_sub(1)];
                previous = None;
            }
            let session = sampled.native_session();
            let mut rect = |position| {
                session
                    .page_coords_at_pos(position)
                    .and_then(RowBox::native)
            };
            let stamp = Stamp {
                document,
                style: sampled.style_revision(),
                viewport: [
                    measurement.page.x.0.to_bits(),
                    measurement.page.y.0.to_bits(),
                    width.to_bits(),
                    height.to_bits(),
                ],
                anchors: anchor_stamp(&starts, anchors, &mut rect),
            };
            if previous == Some(stamp) {
                continue;
            }
            let left = measurement.page.x;
            let top = measurement.page.y;
            let first = session.pos_at_page_coords(Point::new(left + px(1.), top + px(1.)));
            let last = session
                .pos_at_page_coords(Point::new(left + px(1.), top + px((height - 1.).max(0.))));
            let visible = visible_rows(&starts, top.0, height, first.zip(last), &mut rect);
            anchors = visible.anchors;
            rows.set(visible.labels);
            previous = Some(Stamp {
                anchors: anchor_stamp(&starts, anchors, &mut rect),
                ..stamp
            });
        }
    });
    let gutter = view()
        .relative()
        .w(move || {
            let config = settings.get();
            px(if config.enabled { config.width } else { 0. })
        })
        .h_full()
        .flex_shrink_0()
        .overflow_hidden()
        .pointer_events(PointerEvents::None)
        .selectable(false)
        .accessibility_elements_hidden(true)
        .bg(move || rgb(settings.get().background))
        .border_r(move || px(if settings.get().enabled { 1. } else { 0. }))
        .border_color(move || rgb(settings.get().border))
        .child(dynamic(move || {
            let config = settings.get();
            if !config.enabled {
                return view().into_element();
            }
            view()
                .size_full()
                .children(
                    rows.get()
                        .into_iter()
                        .map(|row| {
                            view()
                                .absolute()
                                .left(px(0.))
                                .top(px(row.top))
                                .w_full()
                                .h(px(row.height))
                                .flex_row()
                                .items_center()
                                .justify_end()
                                .pr(px(6.))
                                .overflow_hidden()
                                .pointer_events(PointerEvents::None)
                                .child(
                                    text(row.number.to_string())
                                        .font_family(config.font.clone())
                                        .text_size(px(config.size))
                                        .line_height(px(config.size))
                                        .text_color(rgb(config.foreground))
                                        .number_of_lines(1u32)
                                        .flex_shrink_0()
                                        .selectable(false),
                                )
                                .into_element()
                        })
                        .collect::<Vec<_>>(),
                )
                .into_element()
        }));
    view()
        .flex_row()
        .size_full()
        .min_w(px(0.))
        .overflow_hidden()
        .child(gutter)
        .child(
            view()
                .element_ref(content_ref)
                .flex_1()
                .min_w(px(0.))
                .h_full()
                .overflow_hidden()
                .child(content),
        )
        .into_element()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn structural_positions_are_binary_searched_without_offset_assumptions() {
        let starts = [1, 12, 22, 41, 90, 110, 150];
        assert_eq!(position_range(&starts, 43, 91), 2..7);
        assert_eq!(position_range(&starts, 91, 43), 2..7);
        assert_eq!(position_range(&starts, 0, 0), 0..2);
    }

    #[test]
    fn numbers_follow_native_variable_heights_and_soft_wraps() {
        let starts = [1, 10, 30, 60];
        let geometry = [
            (1, -15., 14.),
            (10, 50., 30.),
            (30, 90., 12.),
            (60, 115., 16.),
        ];
        let result = visible_rows(&starts, 0., 100., Some((5, 45)), |pos| {
            geometry.iter().find(|row| row.0 == pos).map(|row| RowBox {
                top: row.1,
                height: row.2,
            })
        });
        assert_eq!(
            result.labels,
            vec![
                Label {
                    number: 2,
                    top: 50.,
                    height: 30.
                },
                Label {
                    number: 3,
                    top: 90.,
                    height: 12.
                }
            ]
        );
        // A continuation of the first logical line has no invented number.
        assert!(!result.labels.iter().any(|row| row.number == 1));
    }

    #[test]
    fn page_coordinates_become_viewport_relative_and_partial_rows_survive() {
        let result = visible_rows(&[1, 5, 10], 200., 40., Some((1, 10)), |pos| {
            Some(RowBox {
                top: match pos {
                    1 => 195.,
                    5 => 215.,
                    _ => 250.,
                },
                height: 16.,
            })
        });
        assert_eq!(
            result.labels.iter().map(|row| row.top).collect::<Vec<_>>(),
            [-5., 15.]
        );
    }

    #[test]
    fn missing_geometry_never_fabricates_line_positions() {
        let result = visible_rows(&[1, 5, 10], 0., 40., None, |_| None);
        assert!(result.labels.is_empty());
    }

    #[test]
    fn table_cell_rows_do_not_stack_gutter_labels() {
        let result = visible_rows(&[1, 5, 10, 15], 0., 50., Some((1, 15)), |pos| {
            Some(RowBox {
                top: if pos == 5 || pos == 15 { 20. } else { 0. },
                height: 16.,
            })
        });
        assert_eq!(
            result
                .labels
                .iter()
                .map(|row| row.number)
                .collect::<Vec<_>>(),
            [1, 2]
        );
    }

    #[test]
    fn candidate_work_is_bounded_for_dense_documents() {
        let starts: Vec<_> = (0..100_000).collect();
        let mut queries = 0;
        let result = visible_rows(&starts, 0., 100_000., Some((0, 99_999)), |pos| {
            queries += 1;
            Some(RowBox {
                top: pos as f32,
                height: 1.,
            })
        });
        assert!(queries <= MAX_CANDIDATES);
        assert!(result.labels.len() <= MAX_LABELS);
    }
}
