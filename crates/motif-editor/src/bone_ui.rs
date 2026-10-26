//! Bones in the editor: the bone tool, the skeleton drawn over the canvas, dragging bones, and
//! the panels for length, IK, skin and making a shape follow a bone.

use crate::app::{App, DragOp, MoveItem, Tool, fold};
use crate::canvas::View;
use crate::theme::{self, ButtonKind, FAINT, FG, MUTED, SEL};
use egui::{Color32, Pos2, Rect, RichText, Stroke, vec2};
use motif_core::kurbo::{Affine, Point};
use motif_core::{Document, Ik, Node, Prop, ShapeKind, Skin, Value};

/// Bones on the canvas.
const BONE: Color32 = Color32::from_rgb(0xe8, 0xb0, 0x4a);
/// IK targets and the line from a bone's tip to its target.
const IK: Color32 = Color32::from_rgb(0x5a, 0xd2, 0xff);
/// How close to a joint or a tip the pointer has to be to grab it, in screen points.
const GRAB: f32 = 7.0;

/// Direction of the x axis of `a`, in radians.
fn angle(a: Affine) -> f64 {
    let v = a * Point::new(1.0, 0.0) - a * Point::ZERO;
    v.y.atan2(v.x)
}

fn round(v: f64, k: f64) -> f64 {
    (v * k).round() / k
}

fn degrees(rad: f64) -> f64 {
    round((rad.to_degrees() + 180.0).rem_euclid(360.0) - 180.0, 10.0)
}

/// Joint and tip of bone `i` on the artboard.
fn ends(doc: &Document, i: usize) -> (Point, Point) {
    let w = doc.world_transform(i);
    (w * Point::ZERO, w * doc.nodes[i].bone_tip())
}

fn is_bone(doc: &Document, i: usize) -> bool {
    doc.nodes[i].kind == ShapeKind::Bone
}

/// The bones that `i` belongs to through its parents, nearest first.
fn bone_parent(doc: &Document, i: usize) -> Option<usize> {
    doc.nodes[i].parent.filter(|&p| is_bone(doc, p))
}

/// The bone whose line passes within a few screen points of `p`, nearest first: bones are thin,
/// so they are easier to pick on screen than by their outline.
pub(crate) fn pick_bone(doc: &Document, rect: Rect, view: View, p: Pos2) -> Option<usize> {
    (0..doc.nodes.len())
        .filter(|&i| is_bone(doc, i) && doc.is_visible(i) && !doc.is_locked(i))
        .map(|i| {
            let (a, b) = ends(doc, i);
            let (a, b) = (view.to_screen(rect, a), view.to_screen(rect, b));
            let ab = b - a;
            let t = if ab.length_sq() > 0.0 { ((p - a).dot(ab) / ab.length_sq()).clamp(0.0, 1.0) } else { 0.0 };
            (i, (a + ab * t).distance(p))
        })
        .filter(|(_, d)| *d < GRAB)
        .min_by(|x, y| x.1.total_cmp(&y.1))
        .map(|(i, _)| i)
}

impl App {
    /// Bone tool: a drag from the joint to the tip. With a bone selected, the new bone becomes its
    /// child, and starts exactly at its tip when the drag starts near it. The new bone is selected,
    /// so the next drag continues the chain.
    pub(crate) fn create_bone(&mut self, from: Point, to: Point, grab: f64) {
        if !self.can_edit() {
            return;
        }
        let doc = self.display_doc();
        let parent = self.selected.as_ref().and_then(|s| doc.index_of(s)).filter(|&i| is_bone(&doc, i));
        let parent_world = parent.map_or(Affine::IDENTITY, |p| doc.world_transform(p));
        let mut from = from;
        if let Some(p) = parent {
            let tip = ends(&doc, p).1;
            if (from - tip).hypot() <= grab {
                from = tip;
            }
        }
        let (len, dir) = if (to - from).hypot() < 4.0 { (60.0, angle(parent_world)) } else { ((to - from).hypot(), (to - from).atan2()) };
        let at = parent_world.inverse() * from;
        let rot = degrees(dir - angle(parent_world));
        let parent_id = parent.map(|p| doc.nodes[p].id.clone());
        let Some((out, id)) =
            motif_script::add_bone(&self.src, &self.scene, parent_id.as_deref(), (round(at.x, 10.0), round(at.y, 10.0)), round(len, 10.0), rot)
        else {
            self.status = "Кость из цикла: новые кости в неё добавляются в тексте".into();
            return;
        };
        if self.apply("create", Some(out)) {
            self.select_only(Some(id.clone()));
            self.status = match parent_id {
                Some(p) => format!("Кость «{id}» внутри «{p}». Тяните дальше, чтобы продолжить цепочку; V — выход"),
                None => format!("Кость «{id}». Тяните от её кончика, чтобы добавить следующую; V — выход"),
            };
        }
    }

    /// A drag that starts on a bone: the joint moves it, the tip of the selected bone turns and
    /// stretches it, the body turns it. A bone with IK moves its target instead, so the limb follows.
    pub(crate) fn bone_drag(&mut self, doc: &Document, rect: Rect, view: View, p: Pos2, dp: Point) -> Option<DragOp> {
        let screen = |q: Point| view.to_screen(rect, q);
        // The tip of the selected bone first: it may lie over another bone.
        let selected = self.selected.as_ref().and_then(|s| doc.index_of(s)).filter(|&i| is_bone(doc, i));
        let i = match selected {
            Some(s) if screen(ends(doc, s).1).distance(p) < GRAB => s,
            _ => pick_bone(doc, rect, view, p).or_else(|| doc.hit_test(dp).filter(|&i| is_bone(doc, i)))?,
        };
        let node = &doc.nodes[i];
        let (joint, tip) = ends(doc, i);
        let id = node.id.clone();
        if screen(joint).distance(p) < GRAB {
            return None;
        }
        self.select_only(Some(id.clone()));
        if let Some(t) = node.ik.as_ref().and_then(|ik| doc.index_of(&ik.target)) {
            let item = MoveItem {
                node: doc.nodes[t].id.clone(),
                origin: doc.world_transform(t) * Point::ZERO,
                to_parent: doc.nodes[t].parent.map_or(Affine::IDENTITY, |pi| doc.world_transform(pi).inverse()),
            };
            let targets = crate::guides::Targets::collect(doc, &[t]);
            return Some(DragOp::Move { from: dp, items: vec![item], start_box: None, targets });
        }
        let parent_angle = node.parent.map_or(0.0, |pi| angle(doc.world_transform(pi)));
        if screen(tip).distance(p) < GRAB {
            return Some(DragOp::BoneTip { node: id, joint, parent_angle });
        }
        let start_angle = (dp - joint).atan2().to_degrees();
        Some(DragOp::Rotate { node: id, center: joint, start_angle, start_rot: node.rotation })
    }

    /// Dragging the tip: the bone turns to the pointer, and in Design also takes its length.
    pub(crate) fn drag_bone_tip(&mut self, doc: &Document, node: &str, joint: Point, parent_angle: f64, dp: Point, shift: bool) {
        let v = dp - joint;
        if v.hypot() < 1.0 {
            return;
        }
        let mut rot = (v.atan2() - parent_angle).to_degrees();
        rot = if shift { (rot / 15.0).round() * 15.0 } else { self.snap.rot(rot) };
        let rot = degrees(rot.to_radians());
        let Some(n) = doc.node(node) else { return };
        if n.rotation != rot {
            self.preview(node, Prop::Rotation, Value::Num(rot));
        }
        let len = self.snap.size(v.hypot()).max(1.0);
        if self.anim.is_none() && n.width != len {
            self.preview(node, Prop::Width, Value::Num(len));
        }
    }

    /// The skeleton over the canvas: every bone as a thin diamond with a joint circle, and every
    /// IK target as a crosshair tied to its bone's tip.
    pub(crate) fn paint_bones(&self, painter: &egui::Painter, rect: Rect, view: View, doc: &Document) {
        let screen = |q: Point| view.to_screen(rect, q);
        let selected: Vec<&str> = self.selection_ids();
        for i in 0..doc.nodes.len() {
            let n = &doc.nodes[i];
            if n.kind != ShapeKind::Bone || !doc.is_visible(i) {
                continue;
            }
            let (a, b) = ends(doc, i);
            let (sa, sb) = (screen(a), screen(b));
            let on = selected.contains(&n.id.as_str());
            let hot = self.hovered.as_deref() == Some(n.id.as_str());
            let color = if on { SEL } else { BONE };
            let d = sb - sa;
            let len = d.length();
            if len > 1.0 {
                let dir = d / len;
                let side = vec2(-dir.y, dir.x) * (len * 0.1).clamp(3.0, 8.0);
                let mid = sa + dir * len * 0.15;
                let pts = vec![sa, mid + side, sb, mid - side];
                let fill = color.gamma_multiply(if hot || on { 0.55 } else { 0.32 });
                painter.add(egui::Shape::convex_polygon(pts, fill, Stroke::new(if on { 1.6 } else { 1.0 }, color)));
            }
            painter.circle(sa, 4.0, theme::BG, Stroke::new(1.5, color));
            if on {
                // The tip handle: drag it to turn and stretch the bone.
                painter.circle(sb, 4.0, Color32::WHITE, Stroke::new(1.0, SEL));
            }
            if let Some(t) = n.ik.as_ref().and_then(|ik| doc.index_of(&ik.target)) {
                let st = screen(doc.world_transform(t) * Point::ZERO);
                let dashes = egui::Shape::dashed_line(&[sb, st], Stroke::new(1.0, IK.gamma_multiply(0.8)), 4.0, 3.0);
                painter.extend(dashes);
                let t_on = selected.contains(&doc.nodes[t].id.as_str());
                let c = if t_on { SEL } else { IK };
                painter.circle_stroke(st, 7.0, Stroke::new(1.5, c));
                for v in [vec2(1.0, 0.0), vec2(0.0, 1.0)] {
                    painter.line_segment([st - v * 11.0, st - v * 4.0], Stroke::new(1.5, c));
                    painter.line_segment([st + v * 4.0, st + v * 11.0], Stroke::new(1.5, c));
                }
            }
        }
        if let Some(DragOp::Create { kind: ShapeKind::Bone, from, to }) = &self.drag {
            painter.line_segment([screen(*from), screen(*to)], Stroke::new(2.0, BONE));
            painter.circle(screen(*from), 4.0, theme::BG, Stroke::new(1.5, BONE));
        }
    }

    /// Length and IK of a bone; bones, skin and «Следует за костью» of a shape or group.
    pub(crate) fn bone_section(&mut self, ui: &mut egui::Ui, node: &Node) {
        let doc = self.display_doc();
        let Some(i) = doc.index_of(&node.id) else { return };
        if node.kind == ShapeKind::Bone {
            self.ik_section(ui, &doc, i);
            return;
        }
        let bones: Vec<String> = doc.nodes.iter().filter(|n| n.kind == ShapeKind::Bone).map(|n| n.id.clone()).collect();
        let label = |id: &str| doc.node(id).map_or_else(|| id.to_string(), |n| n.display_name().to_string());
        if bones.is_empty() {
            return;
        }
        let mut new_skin: Option<Option<Skin>> = None;
        let mut attach: Option<Option<String>> = None;
        fold(ui, "Кости", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            let parent = bone_parent(&doc, i).map(|p| doc.nodes[p].id.clone());
            ui.horizontal(|ui| {
                ui.label(RichText::new("Следует за").size(12.0).color(MUTED));
                egui::ComboBox::from_id_salt(("follow", &node.id))
                    .width(ui.available_width() - 8.0)
                    .selected_text(parent.as_deref().map_or_else(|| "ничем".into(), label))
                    .show_ui(ui, |ui| {
                        if ui.selectable_label(parent.is_none(), "ничем").clicked() {
                            attach = Some(None);
                        }
                        for b in &bones {
                            if ui.selectable_label(parent.as_deref() == Some(b), label(b)).clicked() {
                                attach = Some(Some(b.clone()));
                            }
                        }
                    });
            });
            ui.label(RichText::new("Объект переносится в блок кости и движется с ней целиком, не изгибаясь.").size(11.5).color(FAINT));
            if node.kind.is_container() {
                return;
            }
            ui.add_space(4.0);
            ui.label(RichText::new("Гнётся костями").size(12.5).color(FG));
            let skin = node.skin.clone();
            let mut list = skin.as_ref().map(|s| s.bones.clone()).unwrap_or_default();
            let mut changed = false;
            for b in &bones {
                let mut on = list.contains(b);
                if ui.checkbox(&mut on, RichText::new(label(b)).size(12.5).color(FG)).changed() {
                    if on {
                        list.push(b.clone());
                    } else {
                        list.retain(|x| x != b);
                    }
                    changed = true;
                }
            }
            let mut sharp = skin.as_ref().map_or(Skin::SHARP, |s| s.sharp);
            if !list.is_empty() {
                let w = ui.available_width();
                let out = theme::num_field(ui, w, "Жёсткость сгиба", sharp, 0.05, 0.5..=16.0, 1, theme::KeyState::Hidden);
                if let Some(v) = out.changed {
                    sharp = round(v, 10.0);
                    changed = true;
                }
            }
            if changed {
                new_skin = Some((!list.is_empty()).then(|| Skin { sharp, ..Skin::new(list) }));
            }
            ui.label(
                RichText::new(
                    "Контур гнётся вслед за отмеченными костями: каждая точка тянется к ближним костям, на сгибе — к обеим сразу. Жёсткость больше — сгиб острее. Фигуру рисуют в позе, где кости стоят по скрипту, без анимации.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        if let Some(target) = attach
            && self.can_edit()
        {
            match motif_script::attach_to_bone(&self.src, &self.scene, &node.id, target.as_deref()) {
                Some(out) => {
                    self.apply("attach", Some(out));
                }
                None => self.status = "Не получилось перенести: объект из цикла, координаты заданы выражением или кость лежит внутри объекта".into(),
            }
        }
        if let Some(skin) = new_skin
            && self.can_edit()
        {
            // Strokes of the bones taken off the skin go with them.
            let kept = skin.as_ref().map(|s| s.bones.clone()).unwrap_or_default();
            let mut src = self.src.clone();
            for b in node.skin.iter().flat_map(|s| &s.bones).filter(|b| !kept.contains(b)) {
                if let Ok(scene) = motif_script::parse(&src)
                    && let Some(out) = motif_script::clear_weights(&src, &scene, &node.id, Some(b))
                {
                    src = out;
                }
            }
            let out = motif_script::parse(&src).ok().and_then(|scene| motif_script::set_skin(&src, &scene, &node.id, skin.as_ref()));
            if !self.apply(&format!("{}.skin", node.id), out) {
                self.status = "Объект из цикла: кости ему задаются в тексте цикла".into();
            }
        }
    }

    fn ik_section(&mut self, ui: &mut egui::Ui, doc: &Document, i: usize) {
        let node = &doc.nodes[i];
        let id = node.id.clone();
        let ik = node.ik.clone();
        let mut new_ik: Option<Option<Ik>> = None;
        let mut make_target = false;
        // Targets must not move with the chain: leave out the chain's root and everything in it.
        let mut root = i;
        for _ in 1..ik.as_ref().map_or(2, |k| k.chain.max(1)) {
            match bone_parent(doc, root) {
                Some(p) => root = p,
                None => break,
            }
        }
        let inside = |k: usize| {
            let mut up = Some(k);
            while let Some(u) = up {
                if u == root {
                    return true;
                }
                up = doc.nodes[u].parent;
            }
            false
        };
        let candidates: Vec<String> = (0..doc.nodes.len()).filter(|&k| !is_bone(doc, k) && !inside(k)).map(|k| doc.nodes[k].id.clone()).collect();
        fold(ui, "Инверсная кинематика (IK)", |ui| {
            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
            ui.horizontal(|ui| {
                ui.label(RichText::new("Цель").size(12.0).color(MUTED));
                let shown = ik.as_ref().map_or_else(|| "нет".to_string(), |k| k.target.clone());
                egui::ComboBox::from_id_salt(("ik-target", &id)).width(ui.available_width() - 8.0).selected_text(shown).show_ui(ui, |ui| {
                    if ui.selectable_label(ik.is_none(), "нет").clicked() {
                        new_ik = Some(None);
                    }
                    for c in &candidates {
                        let on = ik.as_ref().is_some_and(|k| &k.target == c);
                        if ui.selectable_label(on, c).clicked() {
                            let base = ik.clone().unwrap_or(Ik { target: String::new(), chain: 2, flip: false });
                            new_ik = Some(Some(Ik { target: c.clone(), ..base }));
                        }
                    }
                });
            });
            match &ik {
                None => {
                    if theme::button(ui, "Создать цель на кончике", ButtonKind::Normal)
                        .on_hover_text("Пустая группа на кончике кости становится целью: двигайте её, и рука или нога тянется за ней")
                        .clicked()
                    {
                        make_target = true;
                    }
                }
                Some(k) => {
                    let w = ui.available_width();
                    let out = theme::num_field(ui, w, "Костей в цепочке", k.chain as f64, 0.05, 1.0..=8.0, 0, theme::KeyState::Hidden);
                    if let Some(v) = out.changed {
                        new_ik = Some(Some(Ik { chain: v.round().max(1.0) as usize, ..k.clone() }));
                    }
                    let mut flip = k.flip;
                    if ui.checkbox(&mut flip, RichText::new("Сгиб в другую сторону").size(12.5).color(FG)).changed() {
                        new_ik = Some(Some(Ik { flip, ..k.clone() }));
                    }
                }
            }
            ui.label(
                RichText::new(
                    "Кончик кости тянется к цели, кости выше по цепочке поворачиваются сами: 2 кости — рука или нога, 1 — кость просто смотрит на цель. Анимируйте цель, а не углы костей.",
                )
                .size(11.5)
                .color(FAINT),
            );
        });
        if !self.can_edit() {
            return;
        }
        if make_target {
            let Some((out, target)) = motif_script::add_ik_target(&self.src, &self.scene, &id) else { return };
            if self.apply("ik", Some(out)) {
                let ik = Ik { target, chain: 2, flip: false };
                let out = motif_script::set_ik(&self.src, &self.scene, &id, Some(&ik));
                self.apply("ik", out);
                self.status = format!("Цель «{}» на кончике кости: двигайте её или саму кость", ik.target);
            }
        } else if let Some(ik) = new_ik {
            let out = motif_script::set_ik(&self.src, &self.scene, &id, ik.as_ref());
            if !self.apply("ik", out) {
                self.status = "Кость из цикла: IK задаётся в тексте цикла".into();
            }
        }
    }

    pub(crate) fn selection_ids(&self) -> Vec<&str> {
        self.selected.iter().chain(&self.also).map(|s| s.as_str()).collect()
    }

    /// The bone tool is a create tool that stays on after each bone.
    pub(crate) fn bone_tool(&self) -> bool {
        self.tool == Tool::Create(ShapeKind::Bone)
    }
}
