//! Auto-layout for interfaces: groups that stack their children in a row or a column, panels
//! that grow with what is stacked on them, and objects measured from an edge of the artboard.

use crate::{Document, ShapeKind};
use kurbo::{Rect, Shape, Vec2};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackDir {
    Row,
    Column,
}

/// Where the stacked children sit across the stack: a row's children at the top, middle or
/// bottom; a column's at the left, center or right.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StackAlign {
    Start,
    Center,
    End,
}

/// `stack column center gap 8 pad 12`: the group places its children one after another.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stack {
    pub dir: StackDir,
    pub align: StackAlign,
    /// Space between two children.
    pub gap: f64,
    /// Space between the children and the edge of the stack's box.
    pub pad: f64,
}

impl Stack {
    pub fn new(dir: StackDir) -> Self {
        Stack { dir, align: StackAlign::Center, gap: 0.0, pad: 0.0 }
    }
}

impl StackDir {
    pub fn keyword(self) -> &'static str {
        match self {
            StackDir::Row => "row",
            StackDir::Column => "column",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        [StackDir::Row, StackDir::Column].into_iter().find(|d| d.keyword() == s)
    }
}

impl StackAlign {
    pub const ALL: [StackAlign; 3] = [StackAlign::Start, StackAlign::Center, StackAlign::End];

    pub fn keyword(self) -> &'static str {
        match self {
            StackAlign::Start => "start",
            StackAlign::Center => "center",
            StackAlign::End => "end",
        }
    }

    pub fn from_keyword(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|a| a.keyword() == s)
    }

    fn at(self, room: f64) -> f64 {
        match self {
            StackAlign::Start => 0.0,
            StackAlign::Center => room / 2.0,
            StackAlign::End => room,
        }
    }
}

/// The words of `anchor`: which point of the box a position is measured from, as a part of its
/// width or height.
pub fn anchor_word(s: &str) -> Option<(Option<f64>, Option<f64>)> {
    Some(match s {
        "left" => (Some(0.0), None),
        "center" => (Some(0.5), None),
        "right" => (Some(1.0), None),
        "top" => (None, Some(0.0)),
        "middle" => (None, Some(0.5)),
        "bottom" => (None, Some(1.0)),
        _ => return None,
    })
}

/// How an anchor is written: `anchor right bottom`, `anchor center`.
pub fn anchor_text((h, v): (f64, f64)) -> String {
    let h = match h {
        x if x < 0.25 => "left",
        x if x > 0.75 => "right",
        _ => "center",
    };
    let v = match v {
        y if y < 0.25 => "top",
        y if y > 0.75 => "bottom",
        _ => "middle",
    };
    match (h, v) {
        (h, "top") => format!("anchor {h}"),
        ("left", v) => format!("anchor {v}"),
        (h, v) => format!("anchor {h} {v}"),
    }
}

impl Document {
    /// Places stacked children, sizes `fit` panels and moves anchored objects. Runs before the
    /// other rigging, every time the document is posed: stacks follow animated sizes.
    pub fn layout(&mut self) {
        let any = self.nodes.iter().any(|n| n.stack.is_some() || n.anchor.is_some());
        for n in &mut self.nodes {
            n.offset = Vec2::ZERO;
            n.frame = None;
        }
        if !any {
            return;
        }
        // Inner stacks first: an outer stack measures them by the box they end up with.
        for g in (0..self.nodes.len()).rev() {
            if let Some(stack) = self.nodes[g].stack {
                self.stack(g, stack);
            }
        }
        // Anchors, parents before children (a parent's offset moves its children anyway).
        for i in 0..self.nodes.len() {
            let Some((h, v)) = self.nodes[i].anchor else { continue };
            let stacked = self.nodes[i].parent.is_some_and(|p| self.nodes[p].stack.is_some());
            if stacked || self.nodes[i].fit {
                continue;
            }
            if let Some(r) = self.anchor_room(i) {
                self.nodes[i].offset = Vec2::new(r.x0 + h * r.width(), r.y0 + v * r.height());
            }
        }
    }

    /// The box an anchor of node `i` is measured in: the artboard for a top-level object, else
    /// its group's stack box, or the size of its parent. In the parent's space.
    pub fn anchor_room(&self, i: usize) -> Option<Rect> {
        match self.nodes[i].parent {
            None => Some(Rect::new(0.0, 0.0, self.artboard.width, self.artboard.height)),
            Some(p) => self.nodes[p].frame.or_else(|| {
                let n = &self.nodes[p];
                (n.width > 0.0 && n.height > 0.0).then(|| Rect::from_center_size((0.0, 0.0), (n.width, n.height)))
            }),
        }
    }

    fn stack(&mut self, g: usize, stack: Stack) {
        let kids: Vec<usize> = (0..self.nodes.len()).filter(|&k| self.nodes[k].parent == Some(g) && self.nodes[k].visible).collect();
        let (fit, flow): (Vec<usize>, Vec<usize>) = kids.into_iter().partition(|&k| self.nodes[k].fit);
        // Each child's box around its own origin.
        let boxes: Vec<Rect> = flow.iter().map(|&k| self.own_box(k).unwrap_or(Rect::ZERO)).collect();
        let along = |r: &Rect| if stack.dir == StackDir::Row { r.width() } else { r.height() };
        let across = |r: &Rect| if stack.dir == StackDir::Row { r.height() } else { r.width() };
        let length = boxes.iter().map(along).sum::<f64>() + stack.gap * boxes.len().saturating_sub(1) as f64;
        let thick = boxes.iter().map(across).fold(0.0, f64::max);
        let size = if stack.dir == StackDir::Row { (length, thick) } else { (thick, length) };
        let frame = Rect::from_center_size((0.0, 0.0), (size.0 + 2.0 * stack.pad, size.1 + 2.0 * stack.pad));
        let mut cursor = 0.0;
        for (&k, b) in flow.iter().zip(&boxes) {
            let cross = stack.align.at(thick - across(b));
            let corner = match stack.dir {
                StackDir::Row => Vec2::new(cursor, cross),
                StackDir::Column => Vec2::new(cross, cursor),
            };
            let target = Vec2::new(frame.x0 + stack.pad, frame.y0 + stack.pad) + corner - Vec2::new(b.x0, b.y0);
            let n = &mut self.nodes[k];
            n.offset = target - Vec2::new(n.x, n.y);
            cursor += along(b) + stack.gap;
        }
        for k in fit {
            let n = &mut self.nodes[k];
            if !matches!(n.kind, ShapeKind::Group | ShapeKind::Bone) {
                n.width = frame.width();
                n.height = frame.height();
            }
            n.offset = frame.center().to_vec2() - Vec2::new(n.x, n.y);
        }
        self.nodes[g].frame = Some(frame);
    }

    /// The box of node `i` with everything inside it, in its parent's space, as if it stood at
    /// the parent's origin: what a stack lines up.
    fn own_box(&self, i: usize) -> Option<Rect> {
        let n = &self.nodes[i];
        let at = n.local_transform();
        let shift = kurbo::Affine::translate(-(Vec2::new(n.x, n.y) + n.offset));
        let own = match (n.frame, n.path()) {
            (Some(f), _) => Some(f),
            // A text takes the room of its lines, not of the ink of its letters.
            (None, _) if n.text.is_some() => n.text.as_ref().map(|t| t.line_box(&n.text_style)),
            (None, Some(p)) => Some(p.bounding_box()),
            (None, None) => None,
        }
        .map(|r| (shift * at).transform_rect_bbox(r));
        let kids = (0..self.nodes.len()).filter(|&k| self.nodes[k].parent == Some(i) && self.nodes[k].visible).filter_map(|k| {
            let b = self.own_box(k)?;
            let c = &self.nodes[k];
            Some((shift * at).transform_rect_bbox(b + Vec2::new(c.x, c.y) + c.offset))
        });
        own.into_iter().chain(kids).reduce(|a, b| a.union(b))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Artboard, Node};

    fn rect(id: &str, w: f64, h: f64, parent: Option<usize>) -> Node {
        let mut n = Node::new(id, ShapeKind::Rect);
        (n.width, n.height, n.parent) = (w, h, parent);
        n
    }

    fn center(d: &Document, id: &str) -> (f64, f64) {
        let p = d.world_transform(d.index_of(id).unwrap()) * kurbo::Point::ZERO;
        ((p.x * 100.0).round() / 100.0, (p.y * 100.0).round() / 100.0)
    }

    #[test]
    fn column_row_fit_and_anchor() {
        let mut d = Document::new(Artboard { width: 400.0, height: 300.0, ..Artboard::default() });
        let mut g = Node::new("menu", ShapeKind::Group);
        (g.x, g.y) = (200.0, 150.0);
        g.stack = Some(Stack { dir: StackDir::Column, align: StackAlign::Center, gap: 10.0, pad: 20.0 });
        d.nodes.push(g);
        let mut bg = rect("bg", 1.0, 1.0, Some(0));
        bg.fit = true;
        d.nodes.push(bg);
        // Positions written in the script do not matter inside a stack.
        let mut a = rect("a", 100.0, 40.0, Some(0));
        a.x = 999.0;
        d.nodes.push(a);
        d.nodes.push(rect("b", 60.0, 20.0, Some(0)));
        d.layout();
        // Column: 40 + 10 + 20 = 70 tall, 100 wide; with padding 140×110 around (200, 150).
        assert_eq!(d.nodes[0].frame, Some(Rect::new(-70.0, -55.0, 70.0, 55.0)));
        assert_eq!(center(&d, "a"), (200.0, 150.0 - 35.0 + 20.0));
        assert_eq!(center(&d, "b"), (200.0, 150.0 - 35.0 + 50.0 + 10.0));
        assert_eq!((d.nodes[1].width, d.nodes[1].height), (140.0, 110.0));
        assert_eq!(center(&d, "bg"), (200.0, 150.0));
        // A growing child pushes the next one.
        d.nodes[2].height = 60.0;
        d.layout();
        assert_eq!(center(&d, "b"), (200.0, 150.0 - 45.0 + 70.0 + 10.0));
        // Row, aligned to the bottom.
        d.nodes[0].stack = Some(Stack { dir: StackDir::Row, align: StackAlign::End, gap: 0.0, pad: 0.0 });
        d.layout();
        assert_eq!(center(&d, "a"), (200.0 - 80.0 + 50.0, 150.0));
        assert_eq!(center(&d, "b"), (200.0 - 80.0 + 130.0, 150.0 + 30.0 - 10.0));
        // Anchored to the bottom-right corner of the artboard.
        let mut c = rect("close", 20.0, 20.0, None);
        (c.x, c.y, c.anchor) = (-20.0, -15.0, Some((1.0, 1.0)));
        d.nodes.push(c);
        d.layout();
        assert_eq!(center(&d, "close"), (380.0, 285.0));
        d.artboard.width = 800.0;
        d.layout();
        assert_eq!(center(&d, "close"), (780.0, 285.0));
        assert_eq!(anchor_text((1.0, 1.0)), "anchor right bottom");
        assert_eq!(anchor_text((0.5, 0.0)), "anchor center");
    }

    #[test]
    fn nested_stacks_measure_inner_boxes() {
        let mut d = Document::new(Artboard::default());
        let mut outer = Node::new("outer", ShapeKind::Group);
        outer.stack = Some(Stack { gap: 5.0, ..Stack::new(StackDir::Row) });
        d.nodes.push(outer);
        let mut inner = Node::new("inner", ShapeKind::Group);
        inner.parent = Some(0);
        inner.stack = Some(Stack { gap: 0.0, ..Stack::new(StackDir::Column) });
        d.nodes.push(inner);
        d.nodes.push(rect("a", 10.0, 10.0, Some(1)));
        d.nodes.push(rect("b", 10.0, 30.0, Some(1)));
        d.nodes.push(rect("c", 20.0, 20.0, Some(0)));
        d.layout();
        // Inner column: 10 wide, 40 tall. Outer row: 10 + 5 + 20 = 35 wide.
        assert_eq!(d.nodes[1].frame, Some(Rect::new(-5.0, -20.0, 5.0, 20.0)));
        assert_eq!(d.nodes[0].frame, Some(Rect::new(-17.5, -20.0, 17.5, 20.0)));
        assert_eq!(center(&d, "c"), (17.5 - 10.0, 0.0));
        assert_eq!(center(&d, "a"), (-12.5, -15.0));
    }
}
