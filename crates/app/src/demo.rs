use octava_core::{PatternId, Preset, Project, Ticks};

pub fn demo_project() -> Project {
    let mut project = Project::new("Octava Demo");
    let ppq = project.tempo_map.ppq();

    let piano = project
        .add_track("FM Bells")
        .expect("demo project has free channels");
    if let Some(track) = project.track_mut(piano) {
        track.preset = Preset::new(0, 0);
    }
    let piano_pattern = project
        .add_pattern("Bell Arpeggio", Ticks::from_quarters(16.0, ppq))
        .expect("demo pattern is valid");
    if let Some(pattern) = project.pattern_mut(piano_pattern) {
        pattern.preset = Preset::new(0, 0);
    }
    add_notes(
        &mut project,
        piano_pattern,
        &[
            (0.0, 60, 1.5, 104),
            (1.5, 64, 1.5, 90),
            (3.0, 67, 1.0, 105),
            (4.0, 72, 2.0, 112),
            (6.0, 67, 1.0, 95),
            (7.0, 64, 1.0, 90),
            (8.0, 57, 1.5, 98),
            (9.5, 60, 1.5, 100),
            (11.0, 64, 1.0, 92),
            (12.0, 65, 2.0, 106),
            (14.0, 62, 1.0, 94),
            (15.0, 67, 1.0, 100),
        ],
    );
    // Two placements of the same pattern show off the pattern library.
    project
        .add_clip(piano, piano_pattern, Ticks::ZERO)
        .expect("demo clip is valid");

    let bass = project
        .add_track("Lately Bass")
        .expect("demo project has free channels");
    if let Some(track) = project.track_mut(bass) {
        track.preset = Preset::new(0, 20);
        track.gain = 0.85;
    }
    let bass_pattern = project
        .add_pattern("Bass Line", Ticks::from_quarters(16.0, ppq))
        .expect("demo pattern is valid");
    if let Some(pattern) = project.pattern_mut(bass_pattern) {
        pattern.preset = Preset::new(0, 20);
    }
    add_notes(
        &mut project,
        bass_pattern,
        &[
            (0.0, 36, 4.0, 92),
            (4.0, 33, 4.0, 88),
            (8.0, 29, 4.0, 94),
            (12.0, 31, 2.0, 90),
            (14.0, 31, 2.0, 86),
        ],
    );
    project
        .add_clip(bass, bass_pattern, Ticks::ZERO)
        .expect("demo clip is valid");

    project
}

fn add_notes(project: &mut Project, pattern: PatternId, notes: &[(f64, u8, f64, u8)]) {
    let ppq = project.tempo_map.ppq();
    for &(start, key, length, velocity) in notes {
        project
            .add_note(
                pattern,
                key,
                velocity,
                Ticks::from_quarters(start, ppq),
                Ticks::from_quarters(length, ppq),
            )
            .expect("demo note is valid");
    }
}
