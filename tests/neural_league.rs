use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "solverpk-neural-league-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn checkpoint(&self, name: &str) -> String {
        self.0.join(name).to_str().unwrap().to_owned()
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solverpk"))
        .args(args)
        .output()
        .unwrap()
}

fn succeeds(args: &[&str]) -> String {
    let output = run(args);
    assert!(
        output.status.success(),
        "args={args:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn train(path: &str, seed: &str) {
    succeeds(&[
        "--sixmax-neural-train", "--hands", "12", "--seed", seed,
        "--learning-rate", "0.001", "--checkpoint", path, "--rake-bps", "0",
    ]);
}

fn field<'a>(line: &'a str, name: &str) -> &'a str {
    let prefix = format!("{name}=");
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("missing field {name}: {line}"))
}

#[test]
fn league_cli_requires_an_explicit_rake_configuration() {
    let output = run(&[
        "--sixmax-neural-league", "--hands", "12", "--checkpoint", "unused",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("explicit --rake-bps"));
}

#[test]
fn league_cli_rejects_a_single_cluster_before_opening_checkpoints() {
    let output = run(&[
        "--sixmax-neural-league", "--hands", "6", "--checkpoint", "unused",
        "--rake-bps", "0",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("two independent clusters"));
}

#[test]
fn league_runs_diverse_frozen_rivals_reproducibly_and_preserves_checkpoints() {
    let directory = TestDirectory::new();
    let candidate = directory.checkpoint("candidate.bin");
    let rival = directory.checkpoint("rival.bin");
    train(&candidate, "803");
    train(&rival, "919");
    let before_candidate = std::fs::read(&candidate).unwrap();
    let before_rival = std::fs::read(&rival).unwrap();
    let args = [
        "--sixmax-neural-league", "--hands", "24", "--seed", "4073",
        "--checkpoint", &candidate, "--rival-checkpoint", &rival,
        "--rake-bps", "500", "--rake-cap-bb", "3",
    ];
    let output = succeeds(&args);
    let repeated = succeeds(&args);
    let rows: Vec<_> = output.lines()
        .filter(|line| line.starts_with("SIXMAX_LEAGUE_ROW ")).collect();
    let repeated_rows: Vec<_> = repeated.lines()
        .filter(|line| line.starts_with("SIXMAX_LEAGUE_ROW ")).collect();
    assert_eq!(rows, repeated_rows, "fixed seeds must reproduce league results");
    assert_eq!(rows.len(), 6, "five behavioral references plus the frozen rival");
    for name in ["uniform_legal", "calling_station", "tight_aggressive", "loose_aggressive", "nit"] {
        assert!(rows.iter().any(|row| field(row, "opponent") == name), "missing rival {name}");
    }
    for row in &rows {
        assert_eq!(field(row, "hands"), "24");
        assert_eq!(field(row, "clusters"), "4");
        for metric in ["bb100", "ci95_cluster_half", "table_rake_bb100"] {
            assert!(field(row, metric).parse::<f64>().unwrap().is_finite());
        }
        assert!(field(row, "ci95_cluster_half").parse::<f64>().unwrap() >= 0.0);
        assert!(field(row, "table_rake_bb100").parse::<f64>().unwrap() >= 0.0);
    }
    let summary = output.lines().find(|line| line.starts_with("SIXMAX_NEURAL_LEAGUE ")).unwrap();
    assert_eq!(field(summary, "comparison"), "descriptive");
    let worst = field(summary, "worst_observed_rival");
    let worst_row = rows.iter().find(|row| field(row, "opponent") == worst).unwrap();
    let worst_ev = field(worst_row, "bb100").parse::<f64>().unwrap();
    assert!(rows.iter().all(|row| field(row, "bb100").parse::<f64>().unwrap() >= worst_ev));
    assert_eq!(std::fs::read(&candidate).unwrap(), before_candidate);
    assert_eq!(std::fs::read(&rival).unwrap(), before_rival);
}

#[test]
fn league_rejects_actual_training_stream_reuse_even_with_a_different_master_seed() {
    let directory = TestDirectory::new();
    let candidate = directory.checkpoint("candidate.bin");
    train(&candidate, "803");
    // With the existing XOR schedule, this different master seed makes the
    // first evaluation deal exactly equal to training deal number one.
    let overlapping_seed = (803u64 ^ 0x9e3779b97f4a7c15).to_string();
    for seed in ["803", overlapping_seed.as_str()] {
        let output = run(&[
            "--sixmax-neural-league", "--hands", "12", "--seed", seed,
            "--checkpoint", &candidate, "--rake-bps", "0",
        ]);
        assert!(!output.status.success(), "accepted overlapping stream seed {seed}");
        assert!(String::from_utf8_lossy(&output.stderr).contains("overlap"));
    }
}

#[test]
fn league_rejects_the_rng_zero_state_fallback_alias() {
    let directory = TestDirectory::new();
    let candidate = directory.checkpoint("candidate.bin");
    // SplitMix64(seed + stride) is zero for the first literal and equals
    // the RNG's fallback state for the second. The decks are identical.
    train(&candidate, &0x61c8864680b583ebu64.to_string());
    let alias = 0xcd36da00563c5ef6u64.to_string();
    let output = run(&[
        "--sixmax-neural-league", "--hands", "12", "--seed", &alias,
        "--checkpoint", &candidate, "--rake-bps", "0",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overlap"));
}

#[test]
fn league_checks_the_frozen_rivals_training_stream_too() {
    let directory = TestDirectory::new();
    let candidate = directory.checkpoint("candidate.bin");
    let rival = directory.checkpoint("rival.bin");
    train(&candidate, "803");
    train(&rival, "919");
    let output = run(&[
        "--sixmax-neural-league", "--hands", "12", "--seed", "919",
        "--checkpoint", &candidate, "--rival-checkpoint", &rival,
        "--rake-bps", "0",
    ]);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("overlap"));
}

#[test]
fn league_does_not_label_untracked_legacy_weights_as_verified_holdout() {
    let directory = TestDirectory::new();
    let candidate = directory.checkpoint("legacy.bin");
    // Exact public v1 format: magic, steps, baseline, 64x32 + 32 +
    // 32x6 + 6 parameters. Zero weights represent uniform legal play.
    let mut checkpoint = Vec::from(&b"SPKNN001"[..]);
    checkpoint.extend_from_slice(&0u64.to_le_bytes());
    checkpoint.extend_from_slice(&0f32.to_le_bytes());
    checkpoint.resize(checkpoint.len() + (64 * 32 + 32 + 32 * 6 + 6) * 4, 0);
    std::fs::write(&candidate, &checkpoint).unwrap();
    let output = succeeds(&[
        "--sixmax-neural-league", "--hands", "12", "--seed", "4073",
        "--checkpoint", &candidate, "--rake-bps", "0",
    ]);
    let rows: Vec<_> = output.lines()
        .filter(|line| line.starts_with("SIXMAX_LEAGUE_ROW ")).collect();
    assert_eq!(rows.len(), 5);
    assert!(rows.iter().all(|row| field(row, "split") == "unverified_legacy"));
    assert_eq!(std::fs::read(&candidate).unwrap(), checkpoint);
}
