//! Project file serialization.
//!
//! Projects are stored as a small, versioned JSON envelope so that future
//! format changes can be detected instead of silently mis-parsed.

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::model::Project;

pub const PROJECT_FORMAT: &str = "openoctava-project";
pub const PROJECT_VERSION: u32 = 1;
pub const PROJECT_EXTENSION: &str = "octava";

#[derive(Serialize, Deserialize)]
struct ProjectDocument {
    format: String,
    version: u32,
    project: Project,
}

#[derive(Debug, Error)]
pub enum PersistError {
    #[error("project data is not valid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("not an OpenOctava project file")]
    WrongFormat,
    #[error("project version {found} is newer than supported version {supported}")]
    UnsupportedVersion { found: u32, supported: u32 },
}

/// Serializes a project into the on-disk JSON envelope.
pub fn encode_project(project: &Project) -> Result<String, PersistError> {
    let document = ProjectDocument {
        format: PROJECT_FORMAT.to_owned(),
        version: PROJECT_VERSION,
        project: project.clone(),
    };
    Ok(serde_json::to_string_pretty(&document)?)
}

/// Parses a project written by [`encode_project`].
pub fn decode_project(text: &str) -> Result<Project, PersistError> {
    let document: ProjectDocument = serde_json::from_str(text)?;
    if document.format != PROJECT_FORMAT {
        return Err(PersistError::WrongFormat);
    }
    if document.version > PROJECT_VERSION {
        return Err(PersistError::UnsupportedVersion {
            found: document.version,
            supported: PROJECT_VERSION,
        });
    }
    Ok(document.project)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::Ticks;

    fn sample_project() -> Project {
        let mut project = Project::new("Song");
        let track = project.add_track("Piano").unwrap();
        let pattern = project.add_pattern("Verse", Ticks(1920)).unwrap();
        project.add_clip(track, pattern, Ticks::ZERO).unwrap();
        project
            .add_note(pattern, 60, 100, Ticks::ZERO, Ticks(480))
            .unwrap();
        project
    }

    #[test]
    fn round_trips_a_project() {
        let project = sample_project();
        let text = encode_project(&project).unwrap();
        assert!(text.contains(PROJECT_FORMAT));
        let restored = decode_project(&text).unwrap();
        assert_eq!(project, restored);
    }

    #[test]
    fn rejects_other_json_documents() {
        let text = encode_project(&sample_project())
            .unwrap()
            .replace(PROJECT_FORMAT, "something-else");
        assert!(matches!(
            decode_project(&text),
            Err(PersistError::WrongFormat)
        ));
        assert!(matches!(
            decode_project("not json"),
            Err(PersistError::Json(_))
        ));
    }

    #[test]
    fn rejects_newer_versions() {
        let project = sample_project();
        let text = encode_project(&project).unwrap().replace(
            &format!("\"version\": {PROJECT_VERSION}"),
            "\"version\": 999",
        );
        assert!(matches!(
            decode_project(&text),
            Err(PersistError::UnsupportedVersion {
                found: 999,
                supported: PROJECT_VERSION
            })
        ));
    }
}
