#![allow(unsafe_code)]

use clack_host::entry::PluginEntry;
use clack_host::plugin::features;
use std::path::{Path, PathBuf};

/// Metadata for a discovered CLAP plugin.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DiscoveredPlugin {
    pub id: String,
    pub name: String,
    pub vendor: Option<String>,
    pub version: Option<String>,
    pub description: Option<String>,
    pub path: PathBuf,
    pub is_instrument: bool,
    pub is_effect: bool,
}

/// Standard directories where CLAP plugins are installed on the system.
#[must_use]
pub fn standard_clap_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();
    if let Some(home) = std::env::var_os("HOME") {
        paths.push(PathBuf::from(home).join(".clap"));
    }
    paths.push(PathBuf::from("/usr/lib/clap"));
    paths.push(PathBuf::from("/usr/local/lib/clap"));
    paths
}

/// Scans the given directory paths for `.clap` dynamic libraries and returns discovered plugins.
pub fn scan_clap_plugins(paths: &[PathBuf]) -> Vec<DiscoveredPlugin> {
    let mut plugins = Vec::new();
    for base in paths {
        if !base.is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|ext| ext == "clap") || path.is_dir() {
                scan_clap_file(&path, &mut plugins);
            }
        }
    }
    plugins
}

fn scan_clap_file(path: &Path, out: &mut Vec<DiscoveredPlugin>) {
    // Safety: we load the dynamic library entry to query descriptors.
    let Ok(entry) = (unsafe { PluginEntry::load(path) }) else {
        return;
    };
    let Some(factory) = entry.get_plugin_factory() else {
        return;
    };
    for desc in factory.plugin_descriptors() {
        let Some(id) = desc.id().and_then(|s| s.to_str().ok()) else {
            continue;
        };
        let name = desc
            .name()
            .and_then(|s| s.to_str().ok())
            .unwrap_or(id)
            .to_owned();
        let vendor = desc
            .vendor()
            .and_then(|s| s.to_str().ok())
            .map(str::to_owned);
        let version = desc
            .version()
            .and_then(|s| s.to_str().ok())
            .map(str::to_owned);
        let description = desc
            .description()
            .and_then(|s| s.to_str().ok())
            .map(str::to_owned);

        let mut is_instrument = false;
        let mut is_effect = false;
        for feat in desc.features() {
            if let Ok(feat_str) = feat.to_str() {
                if feat_str == features::INSTRUMENT.to_str().unwrap_or("")
                    || feat_str == "instrument"
                {
                    is_instrument = true;
                }
                if feat_str == features::AUDIO_EFFECT.to_str().unwrap_or("")
                    || feat_str == "audio-effect"
                {
                    is_effect = true;
                }
            }
        }

        out.push(DiscoveredPlugin {
            id: id.to_owned(),
            name,
            vendor,
            version,
            description,
            path: path.to_path_buf(),
            is_instrument,
            is_effect,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_paths_contain_default_clap_locations() {
        let paths = standard_clap_paths();
        assert!(!paths.is_empty());
        assert!(
            paths
                .iter()
                .any(|p| p.ends_with(".clap") || p.ends_with("clap"))
        );
    }

    #[test]
    fn scan_empty_or_missing_directory_returns_empty() {
        let plugins = scan_clap_plugins(&[PathBuf::from("/nonexistent/path/for/clap")]);
        assert!(plugins.is_empty());
    }
}
