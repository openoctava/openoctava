use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_SOUNDFONT: &str = "assets/soundfonts/VintageDreamsWaves-v2.sf2";

pub(crate) fn default_path() -> PathBuf {
    if let Some(path) = std::env::var_os("OCTAVA_SF2") {
        return PathBuf::from(path);
    }

    if let Ok(current_dir) = std::env::current_dir() {
        let candidate = current_dir.join(DEFAULT_SOUNDFONT);
        if candidate.is_file() {
            return candidate;
        }
    }

    let manifest_candidate = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(DEFAULT_SOUNDFONT);
    if manifest_candidate.is_file() {
        return manifest_candidate;
    }

    if let Ok(executable) = std::env::current_exe()
        && let Some(directory) = executable.parent()
    {
        for prefix in ["", "..", "../.."] {
            let candidate = directory.join(prefix).join(DEFAULT_SOUNDFONT);
            if candidate.is_file() {
                return candidate;
            }
        }
    }

    PathBuf::from(DEFAULT_SOUNDFONT)
}
