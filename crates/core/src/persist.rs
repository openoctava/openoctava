//! Project file serialization.
//!
//! Projects are stored as a small, versioned JSON envelope so that future
//! format changes can be detected instead of silently mis-parsed.

use std::path::Path;

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

/// Serializes a project into the on-disk JSON envelope, making sample and soundfont
/// paths relative to the project file's directory where applicable.
pub fn encode_project_at(project: &Project, project_file: &Path) -> Result<String, PersistError> {
    let mut project = project.clone();
    let project_dir = project_file.parent().unwrap_or(Path::new("."));

    for track in &mut project.tracks {
        for sample in &mut track.samples {
            if let Ok(rel) = sample.path.strip_prefix(project_dir) {
                sample.path = rel.to_path_buf();
            }
        }
    }

    if let Some(soundfont) = &project.soundfont
        && let Ok(rel) = soundfont.strip_prefix(project_dir)
    {
        project.soundfont = Some(rel.to_path_buf());
    }

    encode_project(&project)
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

/// Parses a project written by [`encode_project`], resolving relative sample and
/// soundfont paths against the project file's directory. If an absolute file path
/// does not exist on disk, attempts to locate the file in the project's directory.
pub fn decode_project_at(text: &str, project_file: &Path) -> Result<Project, PersistError> {
    let mut project = decode_project(text)?;
    let project_dir = project_file.parent().unwrap_or(Path::new("."));

    for track in &mut project.tracks {
        for sample in &mut track.samples {
            if sample.path.is_relative() {
                sample.path = project_dir.join(&sample.path);
            } else if !sample.path.exists()
                && let Some(file_name) = sample.path.file_name()
            {
                let local = project_dir.join(file_name);
                let subfolder = project_dir.join("samples").join(file_name);
                if local.exists() {
                    sample.path = local;
                } else if subfolder.exists() {
                    sample.path = subfolder;
                }
            }
        }
    }

    if let Some(soundfont) = &project.soundfont {
        if soundfont.is_relative() {
            project.soundfont = Some(project_dir.join(soundfont));
        } else if !soundfont.exists()
            && let Some(file_name) = soundfont.file_name()
        {
            let local = project_dir.join(file_name);
            if local.exists() {
                project.soundfont = Some(local);
            }
        }
    }

    Ok(project)
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
    fn round_trips_relative_paths() {
        let mut project = sample_project();
        let track_id = project.tracks[0].id;
        let proj_dir = PathBuf::from("/music/my_song");
        let proj_file = proj_dir.join("song.octava");
        let sample_path = proj_dir.join("samples").join("kick.wav");

        project
            .add_sample(track_id, "Kick", sample_path.clone(), Ticks::ZERO, 1.0, 1.0)
            .unwrap();

        let encoded = encode_project_at(&project, &proj_file).unwrap();
        // On-disk path should be relative
        assert!(encoded.contains("\"path\": \"samples/kick.wav\""));

        let restored = decode_project_at(&encoded, &proj_file).unwrap();
        // In-memory path should be resolved back to absolute
        assert_eq!(restored.tracks[0].samples[0].path, sample_path);
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
