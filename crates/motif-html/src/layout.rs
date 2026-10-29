//! Small deterministic flow layout: inline runs, explicit breaks, block children and flex.
//! It deliberately does not implement viewport sizing or browser word wrapping.
use crate::{BoxNode, Element, style::Style, text_size};

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
        .filter(|c| !styles[*c].hidden && (nodes[*c].tag != "#text" || !nodes[*c].text.is_empty() || nodes[*c].text_part))
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
    let s = &styles[i];
    if s.hidden {
        return;
    }
    for &c in &nodes[i].children {
        measure(c, nodes, styles, boxes);
    }
    if nodes[i].tag == "#text" {
        let (w, h) = text_size(&nodes[i].text, s);
        let shape = motif_core::TextShape::new("", if s.bold { motif_core::text::BOLD_FONT } else { motif_core::text::DEFAULT_FONT }).unwrap();
        boxes[i].w = w;
        boxes[i].h = h;
        boxes[i].baseline = s.font_size * (s.line_height + shape.ascender - shape.descender) / 2.0;
        return;
    }
    let c = content(i, nodes, styles, boxes);
    let px = s.padding[1] + s.padding[3] + 2.0 * s.border_width;
    let py = s.padding[0] + s.padding[2] + 2.0 * s.border_width;
    boxes[i].w = s.width.map(|v| if s.border_box { v } else { v + px }).unwrap_or(c.width + px).clamp(0.0, 8192.0);
    boxes[i].h = s.height.map(|v| if s.border_box { v } else { v + py }).unwrap_or(c.height + py).clamp(0.0, 8192.0);
    boxes[i].baseline = if c.height > 0.0 { c.baseline + s.padding[0] + s.border_width } else { boxes[i].h };
}

pub(crate) fn position(i: usize, x: f64, y: f64, nodes: &[Element], styles: &[Style], boxes: &mut [BoxNode]) {
    boxes[i].x = x;
    boxes[i].y = y;
    let s = &styles[i];
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
        position(child, x + s.padding[3] + s.border_width + cx, y + s.padding[0] + s.border_width + cy, nodes, styles, boxes);
    }
}
