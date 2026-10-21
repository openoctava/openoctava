use crate::command::{EditCommand, EditOutcome};
use crate::model::{ModelError, Project};

const DEFAULT_UNDO_LIMIT: usize = 128;

/// Owns the authoritative project and its edit history.
#[derive(Clone, Debug)]
pub struct ProjectStore {
    project: Project,
    undo_stack: Vec<Project>,
    redo_stack: Vec<Project>,
    undo_limit: usize,
    revision: u64,
    saved_project: Option<Project>,
    dirty: bool,
}

impl ProjectStore {
    #[must_use]
    pub fn new(project: Project) -> Self {
        let saved_project = Some(project.clone());
        Self {
            project,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            undo_limit: DEFAULT_UNDO_LIMIT,
            revision: 1,
            saved_project,
            dirty: false,
        }
    }

    #[must_use]
    pub fn project(&self) -> &Project {
        &self.project
    }

    pub fn set_undo_limit(&mut self, limit: usize) {
        self.undo_limit = limit;
        self.trim_undo();
    }

    #[must_use]
    pub fn revision(&self) -> u64 {
        self.revision
    }

    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn mark_saved(&mut self) {
        self.saved_project = Some(self.project.clone());
        self.dirty = false;
    }

    pub fn apply(&mut self, command: EditCommand) -> Result<EditOutcome, ModelError> {
        let previous = self.project.clone();
        let outcome = self.project.apply(&command)?;

        if self.undo_limit > 0 {
            self.undo_stack.push(previous);
            self.trim_undo();
        }
        self.redo_stack.clear();
        self.revision = self.revision.wrapping_add(1);
        self.dirty = true;
        Ok(outcome)
    }

    pub fn undo(&mut self) -> bool {
        let Some(previous) = self.undo_stack.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.project, previous);
        self.redo_stack.push(current);
        self.revision = self.revision.wrapping_add(1);
        self.dirty = self
            .saved_project
            .as_ref()
            .is_none_or(|saved| saved != &self.project);
        true
    }

    pub fn redo(&mut self) -> bool {
        let Some(next) = self.redo_stack.pop() else {
            return false;
        };
        let current = std::mem::replace(&mut self.project, next);
        self.undo_stack.push(current);
        self.trim_undo();
        self.revision = self.revision.wrapping_add(1);
        self.dirty = self
            .saved_project
            .as_ref()
            .is_none_or(|saved| saved != &self.project);
        true
    }

    #[must_use]
    pub fn can_undo(&self) -> bool {
        !self.undo_stack.is_empty()
    }

    #[must_use]
    pub fn can_redo(&self) -> bool {
        !self.redo_stack.is_empty()
    }

    fn trim_undo(&mut self) {
        while self.undo_stack.len() > self.undo_limit {
            self.undo_stack.remove(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::{Project, TrackId};
    use crate::time::Ticks;

    fn fixture() -> (ProjectStore, TrackId, crate::model::ClipId) {
        let mut project = Project::new("Test");
        let track = project.add_track("Piano").unwrap();
        let clip = project.add_clip(track, Ticks::ZERO, Ticks(3840)).unwrap();
        (ProjectStore::new(project), track, clip)
    }

    fn add_note_command(track: TrackId, clip: crate::model::ClipId) -> EditCommand {
        EditCommand::AddNote {
            track,
            clip,
            key: 60,
            velocity: 100,
            start: Ticks::ZERO,
            length: Ticks(480),
        }
    }

    #[test]
    fn undo_and_redo_restore_project_state() {
        let (mut store, track, clip) = fixture();
        assert!(!store.is_dirty());

        store.apply(add_note_command(track, clip)).unwrap();
        assert_eq!(store.project().note_count(), 1);
        assert!(store.is_dirty());
        assert!(store.can_undo());

        assert!(store.undo());
        assert_eq!(store.project().note_count(), 0);
        assert!(store.can_redo());

        assert!(store.redo());
        assert_eq!(store.project().note_count(), 1);
    }

    #[test]
    fn undo_back_to_saved_revision_clears_dirty_flag() {
        let (mut store, track, clip) = fixture();
        store.apply(add_note_command(track, clip)).unwrap();
        store.mark_saved();
        assert!(!store.is_dirty());

        store.apply(add_note_command(track, clip)).unwrap();
        assert!(store.is_dirty());
        store.undo();
        assert!(!store.is_dirty());
    }

    #[test]
    fn failed_command_does_not_touch_history_or_revision() {
        let (mut store, _track, _clip) = fixture();
        let revision = store.revision();

        let result = store.apply(EditCommand::RemoveTrack {
            track: TrackId(999),
        });

        assert!(result.is_err());
        assert_eq!(store.revision(), revision);
        assert!(!store.can_undo());
    }

    #[test]
    fn undo_limit_discards_old_history() {
        let (mut store, track, clip) = fixture();
        store.set_undo_limit(2);
        for _ in 0..5 {
            store.apply(add_note_command(track, clip)).unwrap();
        }

        assert!(store.undo());
        assert!(store.undo());
        assert!(!store.undo());
    }
}
