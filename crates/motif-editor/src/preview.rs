//! Read-only player for the active board, independent of authoring and undo.
use motif_core::{Document, MachinePlayer, SimCache, UiCheck};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mode {
    #[default]
    Rest,
    Clip,
    Ui,
    Machine,
}

#[derive(Default)]
pub(crate) struct Preview {
    pub doc: Option<Document>,
    pub stale: bool,
    pub mode: Mode,
    pub clip: Option<String>,
    pub machine: Option<String>,
    pub player: Option<MachinePlayer>,
    pub check: UiCheck,
    pub time: f64,
    pub playing: bool,
    pub follow: bool,
    pub parameters: BTreeMap<String, f64>,
    pub size: Option<(f64, f64)>,
    pub message: String,
    cache: SimCache,
}

impl Preview {
    pub fn sync(&mut self, doc: &Document, valid: bool) {
        self.stale = !valid;
        if !valid {
            self.playing = false;
            return;
        }
        if self.doc.as_ref() == Some(doc) {
            return;
        }
        let first = self.doc.is_none();
        self.doc = Some(doc.clone());
        if !doc.animations.iter().any(|a| Some(&a.name) == self.clip.as_ref()) {
            self.clip = doc.animations.first().map(|a| a.name.clone());
        }
        if !doc.machines.iter().any(|m| Some(&m.name) == self.machine.as_ref()) {
            self.machine = doc.machines.first().map(|m| m.name.clone());
        }
        self.parameters.retain(|id, v| {
            if let Some(p) = doc.parameter(id) {
                *v = v.clamp(p.min, p.max);
                true
            } else {
                false
            }
        });
        self.reset_player();
        if !first {
            self.message = "Сцена обновлена; проигрывание остановлено".into();
        }
    }

    pub fn reset_player(&mut self) {
        self.playing = false;
        self.time = 0.0;
        self.cache.clear();
        self.check = self.doc.as_ref().map(UiCheck::new).unwrap_or_default();
        self.player = self.doc.as_ref().and_then(|d| d.machines.iter().find(|m| Some(&m.name) == self.machine.as_ref())).map(MachinePlayer::new);
    }

    pub fn set_mode(&mut self, mode: Mode) {
        if self.mode != mode {
            self.mode = mode;
            self.reset_player();
        }
    }

    pub fn set_clip(&mut self, clip: Option<String>) {
        if self.clip != clip {
            self.clip = clip;
            self.reset_player();
        }
    }

    pub fn hidden(&mut self) {
        self.playing = false;
        // Release captures when a panel closes in the middle of an interaction.
        if let Some(doc) = &self.doc {
            self.check.pointer(doc, None, false);
            if let (Some(p), Some(m)) = (&mut self.player, doc.machines.iter().find(|m| Some(&m.name) == self.machine.as_ref())) {
                p.pointer(m, doc, None, false);
            }
        }
    }

    pub fn advance(&mut self, dt: f64, linked: Option<(&str, f64)>) -> bool {
        if self.stale {
            return false;
        }
        if self.follow && self.mode == Mode::Clip {
            if let Some((name, time)) = linked {
                self.set_clip(Some(name.into()));
                self.time = time;
            }
            self.playing = false;
        }
        let Some(mut doc) = self.doc.clone() else {
            return false;
        };
        if let Some((w, h)) = self.size {
            doc.artboard.width = w;
            doc.artboard.height = h;
        }
        let dt = dt.clamp(0.0, 0.1);
        match self.mode {
            Mode::Clip if self.playing => {
                if let Some(a) = doc.animations.iter().find(|a| Some(&a.name) == self.clip.as_ref()) {
                    self.time += dt;
                    if !a.looping && self.time >= a.duration {
                        self.time = a.duration;
                        self.playing = false;
                    }
                    if a.looping {
                        self.time = a.local_time(self.time);
                    }
                }
            }
            Mode::Machine if self.playing => {
                if let (Some(p), Some(m)) = (&mut self.player, doc.machines.iter().find(|m| Some(&m.name) == self.machine.as_ref())) {
                    p.advance(m, &doc, dt);
                }
            }
            Mode::Ui => return self.check.advance(&doc, dt),
            _ => {}
        }
        self.playing
    }

    pub fn pose(&mut self) -> Option<Document> {
        let mut source = self.doc.clone()?;
        if let Some((w, h)) = self.size {
            source.artboard.width = w;
            source.artboard.height = h;
        }
        // Overrides feed the same parameter evaluation and simulation as the editor.
        for p in &mut source.parameters {
            if let Some(value) = self.parameters.get(&p.id) {
                p.value = *value;
            }
        }
        let mut pose = match self.mode {
            Mode::Clip => source
                .animations
                .iter()
                .find(|a| Some(&a.name) == self.clip.as_ref())
                .map(|a| a.pose_with(&source, self.time, &mut self.cache))
                .unwrap_or_else(|| source.clone()),
            Mode::Machine => self
                .player
                .as_ref()
                .zip(source.machines.iter().find(|m| Some(&m.name) == self.machine.as_ref()))
                .map(|(p, m)| p.pose(m, &source))
                .unwrap_or_else(|| source.clone()),
            _ => {
                source.apply_parameters();
                source.clone()
            }
        };
        if self.mode == Mode::Ui {
            pose = self.check.pose(&source, &pose);
        }
        pose.rig();
        Some(pose)
    }

    pub fn invalidate_simulation(&mut self) {
        self.cache.clear();
    }

    pub fn pointer(&mut self, shown: &Document, at: Option<motif_core::kurbo::Point>, down: bool) -> bool {
        if self.stale {
            return false;
        }
        let before = self.check.clone();
        if self.mode == Mode::Ui {
            if let Some((node, said)) = self.check.pointer(shown, at, down) {
                self.message = format!("{node}: {said}");
            }
        } else if self.mode == Mode::Machine
            && self.playing
            && let Some(doc) = &self.doc
            && let (Some(p), Some(m)) = (&mut self.player, doc.machines.iter().find(|m| Some(&m.name) == self.machine.as_ref()))
        {
            p.pointer(m, shown, at, down);
        }
        before != self.check
    }
}
