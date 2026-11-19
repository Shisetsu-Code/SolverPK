use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "solverpk-bankroll-controls-{}-{}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&path).unwrap();
        Self(path)
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

fn field<'a>(line: &'a str, name: &str) -> &'a str {
    let prefix = format!("{name}=");
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("missing field {name}: {line}"))
}

fn allin_checkpoint(directory: &TestDirectory) -> String {
    // SPK6MAX3: six seats, 19,440 rows each, then six weights, baseline,
    // and visits per row. Shove when legal; otherwise call, never fold.
    let mut row = Vec::new();
    for weight in [-1000.0_f32, 0.0, -1000.0, -1000.0, -1000.0, 1000.0] {
        row.extend_from_slice(&weight.to_le_bytes());
    }
    row.extend_from_slice(&0.0_f32.to_le_bytes());
    row.extend_from_slice(&0_u32.to_le_bytes());
    let mut bytes = b"SPK6MAX3".to_vec();
    for _ in 0..6 * 19_440 {
        bytes.extend_from_slice(&row);
    }
    let path = directory.0.join("allin.bin");
    std::fs::write(&path, bytes).unwrap();
    path.to_str().unwrap().to_owned()
}

#[test]
fn bankroll_cli_rejects_malformed_hands_before_reading_a_checkpoint() {
    for value in ["not-a-number", "-1", "18446744073709551616"] {
        let output = run(&[
            "--sixmax-bankroll-eval", "--hands", value, "--seed", "42",
            "--checkpoint", "unused", "--opponent", "clone", "--rake-bps", "0",
        ]);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--hands"),
            "invalid --hands={value} must be reported, not silently defaulted: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn bankroll_cli_rejects_malformed_seed_before_reading_a_checkpoint() {
    for value in ["not-a-number", "-1", "18446744073709551616"] {
        let output = run(&[
            "--sixmax-bankroll-eval", "--hands", "1", "--seed", value,
            "--checkpoint", "unused", "--opponent", "clone", "--rake-bps", "0",
        ]);
        assert_eq!(output.status.code(), Some(2));
        assert!(
            String::from_utf8_lossy(&output.stderr).contains("--seed"),
            "invalid --seed={value} must be reported, not silently defaulted: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[test]
fn bankroll_cli_rejects_a_trailing_duplicate_money_flag_before_reading_a_checkpoint() {
    for flag in ["--bankroll-usd", "--stop-loss-usd", "--max-bet-usd"] {
        let output = run(&[
            "--sixmax-bankroll-eval", "--hands", "1", "--seed", "42",
            "--checkpoint", "unused", "--opponent", "clone", "--rake-bps", "0",
            "--bankroll-usd", "50", "--stop-loss-usd", "20", "--max-bet-usd", "10",
            flag,
        ]);
        assert_eq!(output.status.code(), Some(2));
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(flag) && stderr.contains("duplicate"),
            "trailing duplicate {flag} must be reported before opening the checkpoint: {stderr}"
        );
    }
}

#[test]
fn bankroll_cli_reports_a_stop_triggered_by_the_final_requested_hand() {
    let directory = TestDirectory::new();
    let checkpoint = allin_checkpoint(&directory);
    // Seed 42 gives hero Ad6s on 8s Kc 2s Ks Qd. Seat 3 has KhQs,
    // so the all-in/call fixture loses exactly the hero's USD 10 buy-in.
    for (bankroll, final_balance, reason) in [
        ("10", "0.00", "stop_loss"),
        ("11", "1.00", "insufficient_buyin"),
    ] {
        let output = run(&[
            "--sixmax-bankroll-eval", "--hands", "1", "--seed", "42",
            "--checkpoint", &checkpoint, "--opponent", "clone",
            "--bankroll-usd", bankroll, "--stop-loss-usd", bankroll,
            "--max-bet-usd", "10", "--rake-bps", "0",
        ]);
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        let stdout = String::from_utf8(output.stdout).unwrap();
        let line = stdout.lines().find(|line| line.starts_with("SIXMAX_BANKROLL ")).unwrap();
        assert_eq!(field(line, "hands_played"), "1");
        assert_eq!(field(line, "bankroll_end_usd"), final_balance);
        assert_eq!(field(line, "stop_reason"), reason, "{line}");
    }
}
