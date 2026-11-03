//! Decoding the `redraw` notification (`ext_linegrid` protocol) into typed
//! events (T1.2). Events RingMD does not use are skipped.

use rmpv::Value;

use crate::model::grid::{CursorShape, HlAttr, ModeInfo};

/// One cell run inside a `grid_line` event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineCell {
    pub text: String,
    /// Highlight id; `None` means "same as the previous cell".
    pub hl: Option<u64>,
    pub repeat: usize,
}

/// A UI event Neovim sends inside `redraw`.
#[derive(Debug, Clone, PartialEq)]
pub enum RedrawEvent {
    GridResize {
        grid: u64,
        width: usize,
        height: usize,
    },
    DefaultColorsSet {
        fg: Option<u32>,
        bg: Option<u32>,
        sp: Option<u32>,
    },
    HlAttrDefine {
        id: u64,
        attr: HlAttr,
    },
    GridLine {
        grid: u64,
        row: usize,
        col_start: usize,
        cells: Vec<LineCell>,
    },
    GridClear {
        grid: u64,
    },
    GridCursorGoto {
        grid: u64,
        row: usize,
        col: usize,
    },
    GridScroll {
        grid: u64,
        top: usize,
        bot: usize,
        left: usize,
        right: usize,
        rows: i64,
    },
    ModeInfoSet {
        modes: Vec<ModeInfo>,
    },
    ModeChange {
        name: String,
        index: usize,
    },
    Flush,
}

/// Decode the params of a `redraw` notification. Each param is a batch
/// `[name, args, args, …]`.
pub fn decode(params: &[Value]) -> Vec<RedrawEvent> {
    let mut events = Vec::new();
    for batch in params {
        let Some(items) = batch.as_array() else {
            continue;
        };
        let Some(name) = items.first().and_then(Value::as_str) else {
            continue;
        };
        for args in &items[1..] {
            let args = args.as_array().map(Vec::as_slice).unwrap_or_default();
            match decode_one(name, args) {
                Some(Some(event)) => events.push(event),
                Some(None) => {}
                None => tracing::warn!(name, ?args, "malformed redraw event"),
            }
        }
    }
    events
}

/// `Some(None)` for events RingMD ignores, `None` for malformed ones.
fn decode_one(name: &str, args: &[Value]) -> Option<Option<RedrawEvent>> {
    let event = match name {
        "grid_resize" => RedrawEvent::GridResize {
            grid: u64_at(args, 0)?,
            width: usize_at(args, 1)?,
            height: usize_at(args, 2)?,
        },
        "default_colors_set" => RedrawEvent::DefaultColorsSet {
            fg: color(args.first()?),
            bg: color(args.get(1)?),
            sp: color(args.get(2)?),
        },
        "hl_attr_define" => RedrawEvent::HlAttrDefine {
            id: u64_at(args, 0)?,
            attr: hl_attr(args.get(1)?)?,
        },
        "grid_line" => RedrawEvent::GridLine {
            grid: u64_at(args, 0)?,
            row: usize_at(args, 1)?,
            col_start: usize_at(args, 2)?,
            cells: args
                .get(3)?
                .as_array()?
                .iter()
                .map(line_cell)
                .collect::<Option<_>>()?,
        },
        "grid_clear" => RedrawEvent::GridClear {
            grid: u64_at(args, 0)?,
        },
        "grid_cursor_goto" => RedrawEvent::GridCursorGoto {
            grid: u64_at(args, 0)?,
            row: usize_at(args, 1)?,
            col: usize_at(args, 2)?,
        },
        "grid_scroll" => RedrawEvent::GridScroll {
            grid: u64_at(args, 0)?,
            top: usize_at(args, 1)?,
            bot: usize_at(args, 2)?,
            left: usize_at(args, 3)?,
            right: usize_at(args, 4)?,
            rows: args.get(5)?.as_i64()?,
        },
        "mode_info_set" => RedrawEvent::ModeInfoSet {
            modes: args.get(1)?.as_array()?.iter().map(mode_info).collect(),
        },
        "mode_change" => RedrawEvent::ModeChange {
            name: args.first()?.as_str()?.to_owned(),
            index: usize_at(args, 1)?,
        },
        "flush" => RedrawEvent::Flush,
        _ => return Some(None),
    };
    Some(Some(event))
}

fn u64_at(args: &[Value], index: usize) -> Option<u64> {
    args.get(index)?.as_u64()
}

fn usize_at(args: &[Value], index: usize) -> Option<usize> {
    usize::try_from(u64_at(args, index)?).ok()
}

/// Neovim sends `-1` for "not set".
fn color(value: &Value) -> Option<u32> {
    value.as_i64().and_then(|c| u32::try_from(c).ok())
}

fn map_get<'a>(map: &'a [(Value, Value)], key: &str) -> Option<&'a Value> {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key))
        .map(|(_, v)| v)
}

fn flag(map: &[(Value, Value)], key: &str) -> bool {
    map_get(map, key).and_then(Value::as_bool).unwrap_or(false)
}

fn hl_attr(value: &Value) -> Option<HlAttr> {
    let map = value.as_map()?;
    Some(HlAttr {
        fg: map_get(map, "foreground").and_then(color),
        bg: map_get(map, "background").and_then(color),
        sp: map_get(map, "special").and_then(color),
        reverse: flag(map, "reverse"),
        bold: flag(map, "bold"),
        italic: flag(map, "italic"),
        underline: flag(map, "underline")
            || flag(map, "underdouble")
            || flag(map, "underdotted")
            || flag(map, "underdashed"),
        undercurl: flag(map, "undercurl"),
        strikethrough: flag(map, "strikethrough"),
    })
}

fn line_cell(value: &Value) -> Option<LineCell> {
    let items = value.as_array()?;
    Some(LineCell {
        text: items.first()?.as_str()?.to_owned(),
        hl: items.get(1).and_then(Value::as_u64),
        repeat: items
            .get(2)
            .and_then(Value::as_u64)
            .and_then(|r| usize::try_from(r).ok())
            .unwrap_or(1),
    })
}

fn mode_info(value: &Value) -> ModeInfo {
    let empty = Vec::new();
    let map = value.as_map().unwrap_or(&empty);
    let text = |key| map_get(map, key).and_then(Value::as_str).unwrap_or("");
    ModeInfo {
        name: text("name").to_owned(),
        cursor_shape: match text("cursor_shape") {
            "horizontal" => CursorShape::Horizontal,
            "vertical" => CursorShape::Vertical,
            _ => CursorShape::Block,
        },
        cell_percentage: map_get(map, "cell_percentage")
            .and_then(Value::as_u64)
            .and_then(|p| u32::try_from(p).ok())
            .unwrap_or(100),
        attr_id: map_get(map, "attr_id").and_then(Value::as_u64).unwrap_or(0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn batch(name: &str, args: Vec<Value>) -> Value {
        let mut items = vec![Value::from(name)];
        items.extend(args);
        Value::Array(items)
    }

    fn arr(items: Vec<Value>) -> Value {
        Value::Array(items)
    }

    #[test]
    fn decodes_core_events() {
        let params = vec![
            batch(
                "grid_resize",
                vec![arr(vec![1.into(), 80.into(), 24.into()])],
            ),
            batch(
                "default_colors_set",
                vec![arr(vec![
                    0xebdbb2.into(),
                    0x282828.into(),
                    Value::from(-1),
                    0.into(),
                    0.into(),
                ])],
            ),
            batch(
                "hl_attr_define",
                vec![arr(vec![
                    3.into(),
                    Value::Map(vec![
                        ("foreground".into(), 0xfb4934.into()),
                        ("bold".into(), true.into()),
                        ("undercurl".into(), true.into()),
                    ]),
                    Value::Map(vec![]),
                    arr(vec![]),
                ])],
            ),
            batch(
                "grid_line",
                vec![arr(vec![
                    1.into(),
                    2.into(),
                    4.into(),
                    arr(vec![
                        arr(vec!["a".into(), 3.into()]),
                        arr(vec!["b".into()]),
                        arr(vec![" ".into(), 0.into(), 3.into()]),
                    ]),
                    false.into(),
                ])],
            ),
            batch(
                "grid_cursor_goto",
                vec![arr(vec![1.into(), 2.into(), 5.into()])],
            ),
            batch(
                "grid_scroll",
                vec![arr(vec![
                    1.into(),
                    0.into(),
                    23.into(),
                    0.into(),
                    80.into(),
                    Value::from(-2),
                    0.into(),
                ])],
            ),
            batch("mode_change", vec![arr(vec!["insert".into(), 1.into()])]),
            batch("set_title", vec![arr(vec!["ignored".into()])]),
            batch("flush", vec![arr(vec![])]),
        ];

        let events = decode(&params);
        assert_eq!(
            events,
            vec![
                RedrawEvent::GridResize {
                    grid: 1,
                    width: 80,
                    height: 24
                },
                RedrawEvent::DefaultColorsSet {
                    fg: Some(0xebdbb2),
                    bg: Some(0x282828),
                    sp: None
                },
                RedrawEvent::HlAttrDefine {
                    id: 3,
                    attr: HlAttr {
                        fg: Some(0xfb4934),
                        bold: true,
                        undercurl: true,
                        ..HlAttr::default()
                    }
                },
                RedrawEvent::GridLine {
                    grid: 1,
                    row: 2,
                    col_start: 4,
                    cells: vec![
                        LineCell {
                            text: "a".into(),
                            hl: Some(3),
                            repeat: 1
                        },
                        LineCell {
                            text: "b".into(),
                            hl: None,
                            repeat: 1
                        },
                        LineCell {
                            text: " ".into(),
                            hl: Some(0),
                            repeat: 3
                        },
                    ]
                },
                RedrawEvent::GridCursorGoto {
                    grid: 1,
                    row: 2,
                    col: 5
                },
                RedrawEvent::GridScroll {
                    grid: 1,
                    top: 0,
                    bot: 23,
                    left: 0,
                    right: 80,
                    rows: -2
                },
                RedrawEvent::ModeChange {
                    name: "insert".into(),
                    index: 1
                },
                RedrawEvent::Flush,
            ]
        );
    }

    #[test]
    fn decodes_mode_info() {
        let params = vec![batch(
            "mode_info_set",
            vec![arr(vec![
                true.into(),
                arr(vec![
                    Value::Map(vec![
                        ("name".into(), "normal".into()),
                        ("cursor_shape".into(), "block".into()),
                    ]),
                    Value::Map(vec![
                        ("name".into(), "insert".into()),
                        ("cursor_shape".into(), "vertical".into()),
                        ("cell_percentage".into(), 25.into()),
                        ("attr_id".into(), 7.into()),
                    ]),
                ]),
            ])],
        )];
        let events = decode(&params);
        let [RedrawEvent::ModeInfoSet { modes }] = events.as_slice() else {
            panic!("expected one mode_info_set, got {events:?}");
        };
        assert_eq!(modes[0].cursor_shape, CursorShape::Block);
        assert_eq!(modes[0].cell_percentage, 100);
        assert_eq!(modes[1].cursor_shape, CursorShape::Vertical);
        assert_eq!(modes[1].cell_percentage, 25);
        assert_eq!(modes[1].attr_id, 7);
    }

    #[test]
    fn skips_malformed_events() {
        let params = vec![
            batch("grid_resize", vec![arr(vec!["oops".into()])]),
            Value::from(5),
            batch("flush", vec![arr(vec![])]),
        ];
        assert_eq!(decode(&params), vec![RedrawEvent::Flush]);
    }
}
