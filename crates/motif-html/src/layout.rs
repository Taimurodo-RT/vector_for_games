//! Small deterministic flow layout: inline runs, explicit breaks, block children and flex.
//! It deliberately does not implement viewport sizing or browser word wrapping.
use crate::{
    BoxNode, Element,
    style::{Position, Style},
    text_size,
};

struct Content {
    width: f64,
    height: f64,
    baseline: f64,
    children: Vec<(usize, f64, f64)>,
}

fn content(i: usize, nodes: &[Element], styles: &[Style], boxes: &[BoxNode]) -> Content {
    let s = &styles[i];
    let children: Vec<_> = nodes[i]
        .children
        .iter()
        .copied()
        .filter(|c| {
            !styles[*c].hidden && styles[*c].position != Position::Absolute && (nodes[*c].tag != "#text" || !nodes[*c].text.is_empty() || nodes[*c].text_part)
        })
        .collect();
    let mut out = Content { width: 0.0, height: 0.0, baseline: 0.0, children: Vec::new() };
    if s.flex {
        for (at, &c) in children.iter().enumerate() {
            let b = &boxes[c];
            let m = styles[c].margin;
            let gap = if at == 0 { 0.0 } else { s.gap };
            if s.row {
                out.width += gap;
                out.children.push((c, out.width + m[3], m[0]));
                out.width += b.w + m[1] + m[3];
                out.height = out.height.max(b.h + m[0] + m[2]);
            } else {
                out.height += gap;
                out.children.push((c, m[3], out.height + m[0]));
                out.width = out.width.max(b.w + m[1] + m[3]);
                out.height += b.h + m[0] + m[2];
            }
        }
        out.baseline = out.height;
        return out;
    }
    let mut line: Vec<usize> = Vec::new();
    let flush = |line: &mut Vec<usize>, out: &mut Content, empty: bool| {
        if line.is_empty() {
            if empty {
                out.height += s.font_size * s.line_height;
            }
            return;
        }
        let mut ascent = 0.0_f64;
        let mut descent = 0.0_f64;
        for &c in line.iter() {
            let b = &boxes[c];
            let cs = &styles[c];
            let shift = cs.baseline_shift * cs.font_size;
            ascent = ascent.max(b.baseline - shift + cs.margin[0]);
            descent = descent.max(b.h - b.baseline + shift + cs.margin[2]);
        }
        let mut x = 0.0;
        for (at, &c) in line.iter().enumerate() {
            let cs = &styles[c];
            let b = &boxes[c];
            if at > 0 {
                x += s.gap;
            }
            x += cs.margin[3];
            out.children.push((c, x, out.height + ascent - b.baseline + cs.baseline_shift * cs.font_size));
            x += b.w + cs.margin[1];
        }
        if out.children.len() == line.len() {
            out.baseline = out.height + ascent;
        }
        out.width = out.width.max(x);
        out.height += ascent + descent;
        line.clear();
    };
    for c in children {
        if nodes[c].tag == "br" {
            flush(&mut line, &mut out, true);
        } else if styles[c].inline {
            line.push(c);
        } else {
            flush(&mut line, &mut out, false);
            let m = styles[c].margin;
            if out.height > 0.0 {
                out.height += s.gap;
            }
            out.children.push((c, m[3], out.height + m[0]));
            out.height += m[0] + boxes[c].h + m[2];
            out.width = out.width.max(m[3] + boxes[c].w + m[1]);
        }
    }
    flush(&mut line, &mut out, false);
    out
}

pub(crate) fn measure(i: usize, nodes: &[Element], styles: &[Style], boxes: &mut [BoxNode]) {
    measure_inner(i, nodes, styles, boxes, [None; 2], [None; 2]);
    // Absolute descendants do not contribute to intrinsic flow. Once their auto-sized
    // positioned ancestor is measured, its padding box can still resolve percentages.
    resolve_absolutes(i, nodes, styles, boxes, [boxes[i].w, boxes[i].h]);
}

fn extras(s: &Style) -> [f64; 2] {
    [s.padding[1] + s.padding[3] + 2.0 * s.border_width, s.padding[0] + s.padding[2] + 2.0 * s.border_width]
}

fn content_size(s: &Style, b: &BoxNode) -> [f64; 2] {
    let e = extras(s);
    [(b.w - e[0]).max(0.0), (b.h - e[1]).max(0.0)]
}

fn padding_size(s: &Style, b: &BoxNode) -> [f64; 2] {
    [(b.w - 2.0 * s.border_width).max(0.0), (b.h - 2.0 * s.border_width).max(0.0)]
}

fn measure_inner(i: usize, nodes: &[Element], styles: &[Style], boxes: &mut [BoxNode], parent: [Option<f64>; 2], containing: [Option<f64>; 2]) {
    let s = &styles[i];
    if s.hidden {
        boxes[i] = BoxNode::default();
        return;
    }
    if nodes[i].tag == "#text" {
        let (w, h) = text_size(&nodes[i].text, s);
        let shape = motif_core::TextShape::new("", if s.bold { motif_core::text::BOLD_FONT } else { motif_core::text::DEFAULT_FONT }).unwrap();
        boxes[i].w = w;
        boxes[i].h = h;
        boxes[i].baseline = s.font_size * (s.line_height + shape.ascender - shape.descender) / 2.0;
        return;
    }
    let available = if s.position == Position::Absolute { containing } else { parent };
    let e = extras(s);
    let dimension = |px: Option<f64>, percent: Option<f64>, basis: Option<f64>, extra: f64| {
        let value = px.or_else(|| percent.zip(basis).map(|(p, b)| p * b));
        value.map(|v| if s.border_box { v.max(extra) } else { v + extra })
    };
    let mut width = dimension(s.width, s.width_percent, available[0], e[0]);
    let mut height = dimension(s.height, s.height_percent, available[1], e[1]);
    if s.position == Position::Absolute {
        if width.is_none()
            && let (Some(left), Some(right), Some(basis)) = (s.offsets[3], s.offsets[1], available[0])
        {
            width = Some((basis - left.resolve(basis) - right.resolve(basis) - s.margin[1] - s.margin[3]).max(e[0]));
        }
        if height.is_none()
            && let (Some(top), Some(bottom), Some(basis)) = (s.offsets[0], s.offsets[2], available[1])
        {
            height = Some((basis - top.resolve(basis) - bottom.resolve(basis) - s.margin[0] - s.margin[2]).max(e[1]));
        }
    }
    let child_parent = [width.map(|v| (v - e[0]).max(0.0)), height.map(|v| (v - e[1]).max(0.0))];
    let child_containing =
        if s.positioned() { [width.map(|v| (v - 2.0 * s.border_width).max(0.0)), height.map(|v| (v - 2.0 * s.border_width).max(0.0))] } else { containing };
    for &child in &nodes[i].children {
        measure_inner(child, nodes, styles, boxes, child_parent, child_containing);
    }
    let c = content(i, nodes, styles, boxes);
    boxes[i].w = width.unwrap_or(c.width + e[0]).clamp(0.0, 8192.0);
    boxes[i].h = height.unwrap_or(c.height + e[1]).clamp(0.0, 8192.0);
    boxes[i].baseline = if c.height > 0.0 { c.baseline + s.padding[0] + s.border_width } else { boxes[i].h };
}

fn resolve_absolutes(i: usize, nodes: &[Element], styles: &[Style], boxes: &mut [BoxNode], containing: [f64; 2]) {
    if styles[i].hidden {
        return;
    }
    let own_containing = if styles[i].positioned() { padding_size(&styles[i], &boxes[i]) } else { containing };
    let parent = content_size(&styles[i], &boxes[i]).map(Some);
    for &child in &nodes[i].children {
        if styles[child].position == Position::Absolute {
            measure_inner(child, nodes, styles, boxes, parent, own_containing.map(Some));
        }
        resolve_absolutes(child, nodes, styles, boxes, own_containing);
    }
}

pub(crate) fn position(i: usize, x: f64, y: f64, nodes: &[Element], styles: &[Style], boxes: &mut [BoxNode]) {
    position_inner(i, [x, y], nodes, styles, boxes, [boxes[i].w, boxes[i].h], [x, y, boxes[i].w, boxes[i].h]);
}

fn position_inner(i: usize, at: [f64; 2], nodes: &[Element], styles: &[Style], boxes: &mut [BoxNode], parent_size: [f64; 2], containing: [f64; 4]) {
    let [mut x, mut y] = at;
    let s = &styles[i];
    if s.hidden {
        return;
    }
    if s.position == Position::Relative {
        x += s.offsets[3].map(|v| v.resolve(parent_size[0])).or_else(|| s.offsets[1].map(|v| -v.resolve(parent_size[0]))).unwrap_or(0.0);
        y += s.offsets[0].map(|v| v.resolve(parent_size[1])).or_else(|| s.offsets[2].map(|v| -v.resolve(parent_size[1]))).unwrap_or(0.0);
    }
    boxes[i].x = x.clamp(-8192.0, 8192.0);
    boxes[i].y = y.clamp(-8192.0, 8192.0);
    x = boxes[i].x;
    y = boxes[i].y;
    let c = content(i, nodes, styles, boxes);
    let iw = (boxes[i].w - s.padding[1] - s.padding[3] - 2.0 * s.border_width).max(0.0);
    let ih = (boxes[i].h - s.padding[0] - s.padding[2] - 2.0 * s.border_width).max(0.0);
    for (child, mut cx, mut cy) in c.children {
        if s.flex {
            if s.row {
                if s.justify_center {
                    cx += (iw - c.width) / 2.0;
                }
                if s.align_center {
                    cy = (ih - boxes[child].h) / 2.0;
                }
            } else {
                if s.justify_center {
                    cy += (ih - c.height) / 2.0;
                }
                if s.align_center {
                    cx = (iw - boxes[child].w) / 2.0;
                }
            }
        } else {
            if s.align_center {
                cx += (iw - c.width) / 2.0;
            }
            if s.justify_center {
                cy += (ih - c.height) / 2.0;
            }
        }
        let size = content_size(s, &boxes[i]);
        let padding = padding_size(s, &boxes[i]);
        let own_containing = if s.positioned() { [x + s.border_width, y + s.border_width, padding[0], padding[1]] } else { containing };
        position_inner(child, [x + s.padding[3] + s.border_width + cx, y + s.padding[0] + s.border_width + cy], nodes, styles, boxes, size, own_containing);
    }
    let size = content_size(s, &boxes[i]);
    let padding = padding_size(s, &boxes[i]);
    let cb = if s.positioned() { [x + s.border_width, y + s.border_width, padding[0], padding[1]] } else { containing };
    for &child in &nodes[i].children {
        let cs = &styles[child];
        if cs.hidden || cs.position != Position::Absolute {
            continue;
        }
        let b = &boxes[child];
        let cx = cs.offsets[3]
            .map(|v| v.resolve(cb[2]) + cs.margin[3])
            .or_else(|| cs.offsets[1].map(|v| cb[2] - v.resolve(cb[2]) - cs.margin[1] - b.w))
            .unwrap_or(0.0);
        let cy = cs.offsets[0]
            .map(|v| v.resolve(cb[3]) + cs.margin[0])
            .or_else(|| cs.offsets[2].map(|v| cb[3] - v.resolve(cb[3]) - cs.margin[2] - b.h))
            .unwrap_or(0.0);
        position_inner(child, [cb[0] + cx, cb[1] + cy], nodes, styles, boxes, size, cb);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn layout(html: &str, css: &str) -> (Vec<Element>, Vec<BoxNode>) {
        let mut warnings = Vec::new();
        let (nodes, _) = crate::parse_html(html, &mut warnings).unwrap();
        let rules = crate::css::parse(css, &mut warnings).unwrap();
        let styles = crate::computed(&nodes, &rules, crate::css::State::Base, &mut warnings);
        let mut boxes = vec![BoxNode::default(); nodes.len()];
        measure(0, &nodes, &styles, &mut boxes);
        position(0, 0.0, 0.0, &nodes, &styles, &mut boxes);
        assert!(warnings.is_empty(), "{warnings:?}");
        (nodes, boxes)
    }
    fn rect<'a>(nodes: &[Element], boxes: &'a [BoxNode], id: &str) -> &'a BoxNode {
        &boxes[nodes.iter().position(|n| n.attrs.get("id").is_some_and(|v| v == id)).unwrap()]
    }
    #[test]
    fn absolute_percent_uses_nearest_positioned_padding_box_through_static_wrappers() {
        let (nodes, boxes) = layout(
            "<div id='anchor'><div id='wrapper'><i id='abs'></i></div></div>",
            "#anchor{position:relative;width:200px;height:100px;box-sizing:border-box;padding:10px;border:4px solid black}#wrapper{width:80px;height:40px}#abs{position:absolute;left:25%;top:50%;width:50%;height:25%}",
        );
        let b = rect(&nodes, &boxes, "abs");
        assert_eq!((b.x, b.y, b.w, b.h), (52.0, 50.0, 96.0, 23.0));
    }
    #[test]
    fn absolute_offsets_support_right_bottom_and_stretch_without_changing_flow() {
        let (nodes, boxes) = layout(
            "<div id='anchor'><div id='flow'></div><div id='right'></div><div id='stretch'></div></div><div id='after'></div>",
            "#anchor{position:relative;width:200px;height:100px}#flow{height:10px}#right{position:absolute;right:10px;bottom:20px;width:25%;height:10px}#stretch{position:absolute;left:10px;right:20px;top:5px;bottom:15px}#after{height:5px}",
        );
        let b = rect(&nodes, &boxes, "right");
        assert_eq!((b.x, b.y, b.w, b.h), (140.0, 70.0, 50.0, 10.0));
        let b = rect(&nodes, &boxes, "stretch");
        assert_eq!((b.x, b.y, b.w, b.h), (10.0, 5.0, 170.0, 80.0));
        assert_eq!(rect(&nodes, &boxes, "after").y, 100.0);
    }
    #[test]
    fn auto_containing_height_resolves_absolute_percent_and_relative_offsets_keep_flow() {
        let (nodes, boxes) = layout(
            "<div id='anchor'><div id='flow'></div><div id='abs'></div></div><div id='after'></div>",
            "#anchor{position:relative;left:10px;top:20px;width:200px}#flow{height:40px}#abs{position:absolute;left:0;top:0;width:50%;height:50%}#after{height:5px}",
        );
        let b = rect(&nodes, &boxes, "abs");
        assert_eq!((b.x, b.y, b.w, b.h), (10.0, 20.0, 100.0, 20.0));
        assert_eq!(rect(&nodes, &boxes, "anchor").h, 40.0);
        assert_eq!(rect(&nodes, &boxes, "after").y, 40.0);
    }
    #[test]
    fn ordinary_percent_uses_content_box_and_transformed_static_ancestor_establishes_containing_box() {
        let (nodes, boxes) = layout(
            "<div id='anchor'><div id='flow'></div><div id='abs'></div></div>",
            "#anchor{width:200px;height:100px;box-sizing:border-box;padding:10px;transform:translateX(5px)}#flow{width:50%;height:10px}#abs{position:absolute;left:50%;top:0;width:50%;height:10px}",
        );
        let flow = rect(&nodes, &boxes, "flow");
        assert_eq!((flow.x, flow.y, flow.w, flow.h), (10.0, 10.0, 90.0, 10.0));
        let abs = rect(&nodes, &boxes, "abs");
        assert_eq!((abs.x, abs.y, abs.w, abs.h), (100.0, 0.0, 100.0, 10.0));
    }
}
