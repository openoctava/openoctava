use std::collections::hash_map::DefaultHasher;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

pub(crate) const DEFAULT_SOUNDFONT: &str = "assets/soundfonts/VintageDreamsWaves-v2.sf2";

/// SF2.01 section 8.1.3 #17: pan position generator.
const GEN_PAN: u16 = 17;
/// Valid range of the pan generator, in tenths of a percent.
const PAN_LIMIT: i16 = 500;

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

/// Path of the SoundFont bundled with the application.
#[must_use]
pub fn default_soundfont_path() -> PathBuf {
    default_path()
}

/// A preset (instrument) exposed by a SoundFont file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoundFontPreset {
    pub name: String,
    pub bank: u8,
    pub program: u8,
}

/// Why a SoundFont file could not be inspected.
#[derive(Debug, thiserror::Error)]
pub enum SoundFontError {
    #[error("soundfont '{path}' could not be read: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("'{0}' is not a SoundFont file")]
    NotASoundFont(String),
}

/// Lists the presets (instruments) contained in a SoundFont file.
pub fn list_presets(path: &Path) -> Result<Vec<SoundFontPreset>, SoundFontError> {
    let bytes = fs::read(path).map_err(|source| SoundFontError::Io {
        path: path.display().to_string(),
        source,
    })?;
    parse_presets(&bytes).ok_or_else(|| SoundFontError::NotASoundFont(path.display().to_string()))
}

/// Lists the presets of an in-memory SoundFont, or `None` when the bytes are
/// not a SoundFont.
fn parse_presets(bytes: &[u8]) -> Option<Vec<SoundFontPreset>> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"sfbk" {
        return None;
    }

    let Some((body, end)) = find_chunk(bytes, 12, b"phdr") else {
        return Some(Vec::new());
    };
    let records = &bytes[body..end];
    let (records, _) = records.as_chunks::<38>();
    let mut presets = Vec::with_capacity(records.len().saturating_sub(1));
    for (index, record) in records.iter().enumerate() {
        // The list is terminated by a sentinel record that must be skipped.
        if index + 1 == records.len() {
            break;
        }
        let name = preset_name(&record[..20]);
        if name.is_empty() {
            continue;
        }
        presets.push(SoundFontPreset {
            name,
            bank: u16::from_le_bytes([record[22], record[23]]) as u8,
            program: u16::from_le_bytes([record[20], record[21]]) as u8,
        });
    }
    Some(presets)
}

fn preset_name(bytes: &[u8]) -> String {
    let end = bytes
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).trim().to_owned()
}

fn find_chunk(bytes: &[u8], start: usize, wanted: &[u8; 4]) -> Option<(usize, usize)> {
    let mut pos = start;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body = pos + 8;
        let end = body.saturating_add(size).min(bytes.len());

        if id == wanted {
            return Some((body, end));
        }
        if id == b"LIST"
            && end >= body + 4
            && let Some(found) = find_chunk(bytes, body + 4, wanted)
        {
            return Some(found);
        }

        pos = end + (size & 1);
    }
    None
}

/// Makes a SoundFont safe to play through FluidLite's per-voice panning.
///
/// Some banks hard-pan layers by writing pan generator values outside the
/// range the spec allows (typically ±1000 instead of ±500). FluidLite clamps
/// the result of every voice, so a channel pan can no longer move those
/// layers and the mixer appears to do nothing. Clamping the generators back
/// into the spec range restores the expected balance behaviour.
///
/// Returns the original path when no rewrite is needed (or possible).
pub(crate) fn prepare(path: &Path) -> PathBuf {
    let Ok(mut bytes) = fs::read(path) else {
        return path.to_path_buf();
    };
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"sfbk" {
        return path.to_path_buf();
    }

    let mut changed = 0;
    clamp_chunks(&mut bytes, 12, &mut changed);
    if changed == 0 {
        return path.to_path_buf();
    }

    let cache = cache_path(&bytes);
    if cache.is_file() {
        return cache;
    }

    // Write through a private file and rename it into place so that parallel
    // loads never observe a half-written soundfont.
    let unique = STAGING_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let staging = cache.with_extension(format!("{}-{unique}.tmp", std::process::id()));
    if fs::write(&staging, &bytes).is_ok() && fs::rename(&staging, &cache).is_ok() {
        cache
    } else {
        let _ = fs::remove_file(&staging);
        path.to_path_buf()
    }
}

static STAGING_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn cache_path(bytes: &[u8]) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    bytes.hash(&mut hasher);
    std::env::temp_dir().join(format!("octava-sf2-{:016x}.sf2", hasher.finish()))
}

fn clamp_chunks(bytes: &mut [u8], start: usize, changed: &mut usize) {
    let mut pos = start;
    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes([
            bytes[pos + 4],
            bytes[pos + 5],
            bytes[pos + 6],
            bytes[pos + 7],
        ]) as usize;
        let body = pos + 8;
        let end = body.saturating_add(size).min(bytes.len());

        match id {
            b"LIST" if end >= body + 4 => {
                if &bytes[body..body + 4] == b"pdta" {
                    clamp_chunks(bytes, body + 4, changed);
                }
            }
            b"pgen" | b"igen" => clamp_generators(&mut bytes[body..end], changed),
            _ => {}
        }

        pos = end + (size & 1);
    }
}

fn clamp_generators(records: &mut [u8], changed: &mut usize) {
    let (records, _) = records.as_chunks_mut::<4>();
    for record in records {
        if u16::from_le_bytes([record[0], record[1]]) != GEN_PAN {
            continue;
        }
        let amount = i16::from_le_bytes([record[2], record[3]]);
        let clamped = amount.clamp(-PAN_LIMIT, PAN_LIMIT);
        if clamped != amount {
            record[2..4].copy_from_slice(&clamped.to_le_bytes());
            *changed += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chunks(data: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let mut body = Vec::new();
        for (id, chunk) in data {
            body.extend_from_slice(*id);
            body.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
            body.extend_from_slice(chunk);
            if chunk.len() % 2 == 1 {
                body.push(0);
            }
        }
        body
    }

    fn soundfont(data: &[(&[u8; 4], Vec<u8>)]) -> Vec<u8> {
        let body = chunks(data);
        let mut file = Vec::new();
        file.extend_from_slice(b"RIFF");
        file.extend_from_slice(&((body.len() + 4) as u32).to_le_bytes());
        file.extend_from_slice(b"sfbk");
        file.extend_from_slice(&body);
        file
    }

    fn pdta(pgen: Vec<u8>) -> Vec<u8> {
        let mut data = b"pdta".to_vec();
        data.extend_from_slice(&chunks(&[(b"pgen", pgen)]));
        data
    }

    fn generator(op: u16, amount: i16) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(4);
        bytes.extend_from_slice(&op.to_le_bytes());
        bytes.extend_from_slice(&amount.to_le_bytes());
        bytes
    }

    #[test]
    fn clamps_out_of_range_pan_generators() {
        let mut pgen = Vec::new();
        pgen.extend_from_slice(&generator(GEN_PAN, 1000));
        pgen.extend_from_slice(&generator(8, 13500));
        pgen.extend_from_slice(&generator(GEN_PAN, -1000));
        pgen.extend_from_slice(&generator(GEN_PAN, 200));

        let mut bytes = soundfont(&[(b"LIST", pdta(pgen))]);
        let mut changed = 0;
        clamp_chunks(&mut bytes, 12, &mut changed);

        assert_eq!(changed, 2);
        // 12 byte file header + 8 byte LIST header + 4 byte list type +
        // 8 byte pgen header.
        let start = 12 + 8 + 4 + 8;
        let amounts: Vec<i16> = (0..4)
            .map(|index| {
                let offset = start + index * 4 + 2;
                i16::from_le_bytes([bytes[offset], bytes[offset + 1]])
            })
            .collect();
        assert_eq!(amounts, [500, 13500, -500, 200]);
    }

    #[test]
    fn leaves_valid_soundfonts_unchanged() {
        let mut pgen = Vec::new();
        pgen.extend_from_slice(&generator(GEN_PAN, -500));
        pgen.extend_from_slice(&generator(GEN_PAN, 500));

        let mut bytes = soundfont(&[(b"LIST", pdta(pgen))]);
        let before = bytes.clone();
        let mut changed = 0;
        clamp_chunks(&mut bytes, 12, &mut changed);

        assert_eq!(changed, 0);
        assert_eq!(bytes, before);
    }

    fn phdr_record(name: &str, bank: u16, program: u16) -> Vec<u8> {
        let mut record = vec![0_u8; 38];
        record[..name.len()].copy_from_slice(name.as_bytes());
        record[20..22].copy_from_slice(&program.to_le_bytes());
        record[22..24].copy_from_slice(&bank.to_le_bytes());
        record
    }

    #[test]
    fn bundled_soundfont_lists_presets() {
        let presets = list_presets(&super::default_path()).unwrap();
        assert!(presets.len() > 16);
    }

    #[test]
    fn lists_presets_and_skips_the_sentinel() {
        let mut phdr = Vec::new();
        phdr.extend_from_slice(&phdr_record("Piano", 0, 0));
        phdr.extend_from_slice(&phdr_record("Choir", 1, 52));
        // The final record is the end-of-list sentinel.
        phdr.extend_from_slice(&phdr_record("EOP", 0, 0));

        let mut data = b"pdta".to_vec();
        data.extend_from_slice(&chunks(&[(b"phdr", phdr)]));
        let bytes = soundfont(&[(b"LIST", data)]);

        let presets = parse_presets(&bytes).unwrap();
        assert_eq!(
            presets,
            vec![
                SoundFontPreset {
                    name: "Piano".to_owned(),
                    bank: 0,
                    program: 0,
                },
                SoundFontPreset {
                    name: "Choir".to_owned(),
                    bank: 1,
                    program: 52,
                },
            ]
        );
        assert!(parse_presets(b"not a soundfont").is_none());
    }
}
