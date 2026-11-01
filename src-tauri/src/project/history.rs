//! История правок открытого проекта: снимки «до» и «после» каждого шага (файлы проекта
//! небольшие, снимок целиком проще и надежнее разностей). Отмена и повтор; новая правка после
//! отмены стирает ветку повтора. Не больше `LIMIT` шагов.

use super::model::Snapshot;

const LIMIT: usize = 300;

struct Step {
    label: String,
    before: Snapshot,
    after: Snapshot,
}

#[derive(Default)]
pub struct History {
    undo: Vec<Step>,
    redo: Vec<Step>,
}

impl History {
    pub fn push(&mut self, label: &str, before: Snapshot, after: Snapshot) {
        self.redo.clear();
        self.undo.push(Step { label: label.to_owned(), before, after });
        if self.undo.len() > LIMIT {
            self.undo.remove(0);
        }
    }

    /// Состояние «до» последнего шага (шаг уходит в повтор).
    pub fn undo(&mut self) -> Option<Snapshot> {
        let step = self.undo.pop()?;
        let target = step.before.clone();
        self.redo.push(step);
        Some(target)
    }

    pub fn redo(&mut self) -> Option<Snapshot> {
        let step = self.redo.pop()?;
        let target = step.after.clone();
        self.undo.push(step);
        Some(target)
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }

    pub fn undo_label(&self) -> Option<&str> {
        self.undo.last().map(|s| s.label.as_str())
    }

    pub fn redo_label(&self) -> Option<&str> {
        self.redo.last().map(|s| s.label.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::super::model::{ProjectFile, Sequence, TimelineFile};
    use super::*;

    fn snap(fps: u32) -> Snapshot {
        Snapshot {
            project: ProjectFile { version: 1, name: "A".into(), series: None, sequence: Sequence { width: 2, height: 2, fps, duration_frames: 0, background: "#000000".into() }, library_rules: vec![] },
            timeline: TimelineFile::default(),
        }
    }

    #[test]
    fn new_edit_drops_redo_branch_and_limit_holds() {
        let mut h = History::default();
        h.push("a", snap(1), snap(2));
        h.push("b", snap(2), snap(3));
        assert_eq!(h.undo().unwrap().project.sequence.fps, 2);
        assert_eq!(h.redo_label(), Some("b"));
        h.push("c", snap(2), snap(4));
        assert!(!h.can_redo());
        for i in 0..400 {
            h.push("x", snap(i), snap(i + 1));
        }
        assert_eq!(h.undo.len(), LIMIT);
    }
}
