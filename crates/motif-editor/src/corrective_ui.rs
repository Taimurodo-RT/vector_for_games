use super::*;
use crate::workspace::Workspace;

impl App {
    pub(super) fn prune_deformer_reference(&mut self) {
        let doc = &self.scene.doc;
        let target = self.deformer.target.as_deref();
        let invalid_corrective =
            self.deformer.corrective.as_ref().is_some_and(|(id, _, _)| {
                !doc.correctives.iter().any(|c| c.id == *id && target.is_some_and(|target| c.tracks.iter().any(|t| t.node == target)))
            });
        let invalid_form = self.deformer.form.as_ref().is_some_and(|(p, _)| {
            doc.parameter(p).is_none()
                || !doc.drivers.iter().any(|d| d.parameter == *p && target.is_some_and(|target| d.tracks.iter().any(|t| t.node == target)))
        });
        if invalid_corrective || invalid_form {
            self.deformer.corrective = None;
            self.deformer.form = None;
            self.pose_preview.clear();
        }
        self.pose_preview.retain(|id, _| doc.parameter(id).is_some());
        if self.deformer.target.as_deref().is_some_and(|id| doc.node(id).is_none_or(|n| n.lattice.is_none())) {
            self.deformer.target = None;
            self.deformer.grab = None;
        }
    }

    pub(super) fn corrective_panel(&mut self, ui: &mut egui::Ui, node: &str) {
        if self.workspace() != Workspace::Setup {
            if self.scene.doc.parameters.len() >= 2 && ui.button("Настроить сочетания параметров…").clicked() {
                self.set_workspace(Workspace::Setup);
            }
            return;
        }
        ui.collapsing("Сочетания параметров", |ui| {
            let parameters = self.scene.doc.parameters.clone();
            if parameters.len() < 2 {
                ui.small("Для корректирующей формы нужны два параметра движения.");
                return;
            }
            for axis in 0..2 {
                if !parameters.iter().any(|p| p.id == self.deformer.axes[axis]) {
                    self.deformer.axes[axis] = parameters[axis].id.clone();
                }
                egui::ComboBox::from_id_salt(("corrective-axis", node, axis))
                    .selected_text(parameters.iter().find(|p| p.id == self.deformer.axes[axis]).map_or("Параметр", |p| p.name.as_str()))
                    .show_ui(ui, |ui| {
                        for p in &parameters {
                            ui.selectable_value(&mut self.deformer.axes[axis], p.id.clone(), &p.name);
                        }
                    });
            }
            if ui.add_enabled(self.deformer.axes[0] != self.deformer.axes[1], egui::Button::new("Создать корректирующую форму")).clicked()
            {
                let axes = self.deformer.axes.clone();
                self.create_corrective(node, [&axes[0], &axes[1]]);
            }
            ui.small("Поправка к сочетанию двух параметров. Опоры общие для клипов; тяните сетку в выбранной ячейке.");
            let correctives: Vec<_> = self.scene.doc.correctives.iter().filter(|c| c.tracks.iter().any(|t| t.node == node)).cloned().collect();
            for c in correctives {
                let names: Vec<_> = c.parameters.iter().map(|id| self.scene.doc.parameter(id).map_or(id.as_str(), |p| p.name.as_str())).collect();
                ui.label(format!("{} · {} × {}", c.id, names[0], names[1]));
                let Some(track) = c.tracks.iter().find(|t| t.node == node) else { continue };
                let mut xs: Vec<_> = track.keys.iter().map(|k| k.x).collect();
                let mut ys: Vec<_> = track.keys.iter().map(|k| k.y).collect();
                xs.sort_by(f64::total_cmp);
                xs.dedup();
                ys.sort_by(f64::total_cmp);
                ys.dedup();
                egui::Grid::new(("corrective-grid", node, &c.id)).show(ui, |ui| {
                    ui.label("Y / X");
                    for x in &xs {
                        ui.label(fmt_num(*x));
                    }
                    ui.end_row();
                    for y in &ys {
                        ui.label(fmt_num(*y));
                        for x in &xs {
                            let selected = self.deformer.corrective.as_ref() == Some(&(c.id.clone(), *x, *y));
                            if ui.selectable_label(selected, "●").on_hover_text(format!("{}: {} / {}", c.id, fmt_num(*x), fmt_num(*y))).clicked() {
                                self.begin_corrective(&c.id, *x, *y);
                            }
                        }
                        ui.end_row();
                    }
                });
            }
        });
    }

    pub(super) fn create_corrective(&mut self, node: &str, parameters: [&str; 2]) {
        let base = format!("correction_{node}");
        let mut id = base.clone();
        let mut n = 2;
        while self.scene.doc.correctives.iter().any(|c| c.id == id) {
            id = format!("{base}_{n}");
            n += 1;
        }
        let out = motif_script::correctives::add_lattice(&self.src, &self.scene, &id, node, parameters);
        if self.apply_once("add-corrective", out) {
            let p = self.scene.doc.parameter(parameters[0]).unwrap();
            let q = self.scene.doc.parameter(parameters[1]).unwrap();
            self.begin_corrective(&id, p.max, q.max);
            self.deformer.target = Some(node.into());
        }
    }

    pub(super) fn begin_corrective(&mut self, id: &str, x: f64, y: f64) {
        let Some(c) = self.scene.doc.correctives.iter().find(|c| c.id == id).cloned() else { return };
        self.set_workspace(Workspace::Setup);
        self.pose_preview.insert(c.parameters[0].clone(), x);
        self.pose_preview.insert(c.parameters[1].clone(), y);
        self.deformer.form = None;
        self.deformer.corrective = Some((id.into(), x, y));
        if let Some(target) = self.selected.as_ref().filter(|id| c.tracks.iter().any(|t| t.node == **id)) {
            self.deformer.target = Some(target.clone());
        }
    }
}
