use std::path::PathBuf;
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

const BB_UNITS: i64 = 20_000;
const USD_UNITS: i64 = 200_000;
const DEAL_STRIDE: u64 = 0x9e3779b97f4a7c15;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TestDirectory(PathBuf);

impl TestDirectory {
    fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "solverpk-risk-benchmark-{}-{}",
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

fn words(args: &[&str]) -> Vec<String> {
    args.iter().map(|arg| (*arg).to_owned()).collect()
}

fn run(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_solverpk"))
        .args(args)
        .output()
        .unwrap()
}

fn succeeds(args: &[String]) -> String {
    let output = run(args);
    assert!(
        output.status.success(),
        "args={args:?}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

fn train(kind: &str, checkpoint: &str, seed: &str) {
    let mode = match kind {
        "tabular" => "--sixmax-train",
        "neural" => "--sixmax-neural-train",
        _ => panic!("unsupported fixture kind {kind}"),
    };
    succeeds(&words(&[
        mode, "--hands", "12", "--seed", seed,
        "--learning-rate", "0.001", "--checkpoint", checkpoint,
        "--rake-bps", "0",
    ]));
}

fn benchmark_args(kind: &str, checkpoint: &str) -> Vec<String> {
    words(&[
        "--sixmax-risk-benchmark", "--candidate-kind", kind,
        "--checkpoint", checkpoint, "--opponent", "equity_multiway",
        "--sessions", "3", "--hands", "4", "--seed", "4073",
        "--stack-bb", "100", "--equity-samples", "1",
        "--bankroll-usd", "50", "--stop-loss-usd", "50",
        "--max-bet-usd", "10", "--rake-bps", "500", "--rake-cap-bb", "3",
    ])
}

fn set(args: &mut [String], flag: &str, value: &str) {
    let index = args.iter().position(|arg| arg == flag).unwrap();
    args[index + 1] = value.to_owned();
}

fn field<'a>(line: &'a str, name: &str) -> &'a str {
    let prefix = format!("{name}=");
    line.split_whitespace()
        .find_map(|part| part.strip_prefix(&prefix))
        .unwrap_or_else(|| panic!("missing field {name}: {line}"))
}

fn number(line: &str, name: &str) -> i64 {
    field(line, name).parse().unwrap_or_else(|_| {
        panic!("{name} must preserve exact integer units/counts: {line}")
    })
}

fn session_rows(output: &str) -> Vec<&str> {
    output.lines().filter(|line| line.starts_with("SIXMAX_RISK_SESSION ")).collect()
}

fn summary(output: &str) -> &str {
    let rows: Vec<_> = output.lines()
        .filter(|line| line.starts_with("SIXMAX_RISK_SUMMARY ")).collect();
    assert_eq!(rows.len(), 1, "expected one aggregate row:\n{output}");
    rows[0]
}

fn assert_session_accounting(output: &str, sessions: usize, hands: i64) {
    let rows = session_rows(output);
    assert_eq!(rows.len(), sessions, "every requested session must be retained:\n{output}");
    let mut total_hands = 0;
    let mut total_profit = 0;
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(number(row, "session"), index as i64, "{row}");
        assert_eq!(number(row, "seed"), 4073, "{row}");
        assert_eq!(number(row, "hand_offset"), index as i64 * hands, "{row}");
        assert_eq!(number(row, "start_button"), (index % 6) as i64, "{row}");
        assert_eq!(number(row, "hands_requested"), hands, "{row}");
        let played = number(row, "hands_played");
        assert!((0..=hands).contains(&played), "{row}");
        let initial = number(row, "initial_units");
        let final_balance = number(row, "final_units");
        let profit = number(row, "profit_units");
        let buyin = number(row, "buyin_units");
        let floor = initial - number(row, "stop_loss_units");
        assert_eq!(profit, final_balance - initial, "{row}");
        assert!(final_balance >= floor && final_balance >= 0, "{row}");
        assert_eq!(number(row, "max_wager_units"), 10 * USD_UNITS, "{row}");
        assert!(buyin > 0 && buyin <= 10 * USD_UNITS, "{row}");
        assert!(number(row, "max_drawdown_units") >= (-profit).max(0), "{row}");
        assert!((0..=buyin).contains(&number(row, "hero_max_single_action_units")), "{row}");
        assert!((0..=buyin).contains(&number(row, "hero_max_hand_exposure_units")), "{row}");
        assert!((0..=played).contains(&number(row, "hero_allin_actions")), "{row}");
        assert!((0..=3 * BB_UNITS * played).contains(&number(row, "rake_table_units")), "{row}");
        assert_eq!(field(row, "reached_horizon"), if played == hands { "true" } else { "false" }, "{row}");
        let reason = if final_balance <= floor {
            "stop_loss"
        } else if final_balance < buyin {
            "insufficient_buyin"
        } else if final_balance - buyin < floor {
            "stop_loss_budget"
        } else {
            "hands_limit"
        };
        assert_eq!(field(row, "stop_reason"), reason, "{row}");
        assert_eq!(field(row, "can_continue"), if reason == "hands_limit" { "true" } else { "false" }, "{row}");
        total_hands += played;
        total_profit += profit;
    }
    let aggregate = summary(output);
    assert_eq!(number(aggregate, "sessions"), sessions as i64, "{aggregate}");
    assert_eq!(number(aggregate, "hands_played"), total_hands, "{aggregate}");
    assert_eq!(number(aggregate, "profit_units"), total_profit, "{aggregate}");
    for (name, scale) in [("bb100", BB_UNITS), ("usd100", USD_UNITS)] {
        if total_hands == 0 {
            assert_eq!(field(aggregate, name), "NA", "undefined rate must not look like zero profit: {aggregate}");
        } else {
            let rate: f64 = field(aggregate, name).parse().unwrap();
            let expected = 100.0 * total_profit as f64 / (scale as f64 * total_hands as f64);
            assert!(rate.is_finite() && (rate - expected).abs() <= 0.0051,
                "rate must divide summed net by actual hands, including stopped sessions: {aggregate}");
        }
    }
}

#[test]
fn tabular_benchmark_supports_board_aware_rivals_and_repeats_frozen_rows() {
    let directory = TestDirectory::new();
    let checkpoint = directory.checkpoint("candidate.bin");
    train("tabular", &checkpoint, "711");
    let before = std::fs::read(&checkpoint).unwrap();
    for opponent in ["equity_multiway", "tight_aggressive", "loose_aggressive", "clone"] {
        let mut args = benchmark_args("tabular", &checkpoint);
        set(&mut args, "--opponent", opponent);
        let output = succeeds(&args);
        let repeated = succeeds(&args);
        assert_session_accounting(&output, 3, 4);
        assert_eq!(session_rows(&output), session_rows(&repeated),
            "fixed seeds must reproduce every session against {opponent}");
        assert_eq!(summary(&output), summary(&repeated));
        for row in session_rows(&output) {
            assert_eq!(number(row, "initial_units"), 50 * USD_UNITS);
            assert_eq!(number(row, "buyin_units"), 100 * BB_UNITS);
            // At most USD 30 can have been lost before the fourth hand.
            assert_eq!(number(row, "hands_played"), 4, "{row}");
        }
        assert_eq!(std::fs::read(&checkpoint).unwrap(), before,
            "evaluation against {opponent} changed the candidate checkpoint");
    }
}

#[test]
fn neural_benchmark_is_frozen_and_deterministic() {
    let directory = TestDirectory::new();
    let checkpoint = directory.checkpoint("candidate-neural.bin");
    train("neural", &checkpoint, "803");
    let before = std::fs::read(&checkpoint).unwrap();
    let args = benchmark_args("neural", &checkpoint);
    let output = succeeds(&args);
    let repeated = succeeds(&args);
    assert_session_accounting(&output, 3, 4);
    assert_eq!(session_rows(&output), session_rows(&repeated));
    assert_eq!(summary(&output), summary(&repeated));
    assert_eq!(std::fs::read(&checkpoint).unwrap(), before);
}

#[test]
fn fifty_bb_reserves_a_five_dollar_buyin_with_the_ten_dollar_wager_cap() {
    let directory = TestDirectory::new();
    let checkpoint = directory.checkpoint("candidate.bin");
    train("tabular", &checkpoint, "711");
    for bankroll in ["5", "50"] {
        let mut args = benchmark_args("tabular", &checkpoint);
        set(&mut args, "--opponent", "clone");
        set(&mut args, "--sessions", "1");
        set(&mut args, "--hands", "1");
        set(&mut args, "--stack-bb", "50");
        set(&mut args, "--bankroll-usd", bankroll);
        set(&mut args, "--stop-loss-usd", "5");
        let output = succeeds(&args);
        assert_session_accounting(&output, 1, 1);
        let row = session_rows(&output)[0];
        assert_eq!(number(row, "buyin_units"), 5 * USD_UNITS, "{row}");
        assert_eq!(number(row, "max_wager_units"), 10 * USD_UNITS, "{row}");
        assert_eq!(number(row, "hands_played"), 1,
            "the actual USD 5 buy-in is funded and within the loss budget: {row}");

        set(&mut args, "--stack-bb", "100");
        let output = succeeds(&args);
        assert_session_accounting(&output, 1, 1);
        let row = session_rows(&output)[0];
        assert_eq!(number(row, "hands_played"), 0,
            "the same budget cannot reserve a USD 10 buy-in: {row}");
        assert_eq!(number(row, "buyin_units"), 10 * USD_UNITS, "{row}");
    }
}

#[test]
fn zero_hand_sessions_are_retained_and_their_rates_are_unavailable() {
    let directory = TestDirectory::new();
    let checkpoint = directory.checkpoint("candidate.bin");
    train("tabular", &checkpoint, "711");
    let mut args = benchmark_args("tabular", &checkpoint);
    set(&mut args, "--stop-loss-usd", "1");
    let output = succeeds(&args);
    assert_session_accounting(&output, 3, 4);
    for row in session_rows(&output) {
        for name in ["hands_played", "profit_units", "max_drawdown_units", "rake_table_units",
            "hero_allin_actions", "hero_max_single_action_units", "hero_max_hand_exposure_units"] {
            assert_eq!(number(row, name), 0, "{row}");
        }
        assert_eq!(number(row, "initial_units"), 50 * USD_UNITS, "{row}");
        assert_eq!(number(row, "final_units"), 50 * USD_UNITS, "{row}");
        assert_eq!(number(row, "stop_loss_units"), USD_UNITS, "{row}");
        assert_eq!(field(row, "stop_reason"), "stop_loss_budget", "{row}");
        assert_eq!(field(row, "reached_horizon"), "false", "{row}");
        assert_eq!(field(row, "can_continue"), "false", "{row}");
    }
}

#[test]
fn malformed_duplicate_and_unknown_arguments_fail_before_opening_the_model() {
    let directory = TestDirectory::new();
    let absent = directory.checkpoint("must-not-be-opened.bin");
    // Keep the first regression's --hands valid and small: before the mode
    // exists, main falls through to its unrelated heads-up demonstration.
    for (flag, value) in [
        ("--sessions", "not-a-number"), ("--sessions", "0"),
        ("--hands", "not-a-number"), ("--hands", "0"), ("--hands", "-1"),
        ("--hands", "18446744073709551616"),
        ("--seed", "not-a-number"), ("--seed", "-1"),
        ("--seed", "18446744073709551616"),
        ("--stack-bb", "75"), ("--equity-samples", "0"),
        ("--equity-samples", "4097"), ("--candidate-kind", "unknown"),
        ("--opponent", "unknown"), ("--bankroll-usd", "0"),
        ("--stop-loss-usd", "51"), ("--max-bet-usd", "11"),
        ("--rake-bps", "not-a-number"),
    ] {
        let mut args = benchmark_args("tabular", &absent);
        set(&mut args, flag, value);
        let output = run(&args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "accepted {flag}={value}: {stderr}");
        assert!(!stderr.contains("No such file"),
            "invalid {flag}={value} reached checkpoint loading: {stderr}");
        assert!(!stderr.trim().is_empty(), "invalid {flag}={value} lacked an explanation");
    }
    for flag in ["--hands", "--seed", "--sessions", "--bankroll-usd", "--stop-loss-usd",
        "--max-bet-usd", "--candidate-kind", "--checkpoint", "--stack-bb"] {
        let mut args = benchmark_args("tabular", &absent);
        args.push(flag.to_owned()); // A duplicate must be detected even without a following value.
        let output = run(&args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "accepted duplicate {flag}: {stderr}");
        assert!(stderr.contains("duplicate") && stderr.contains(flag),
            "duplicate {flag} must be diagnosed before checkpoint loading: {stderr}");
    }
    for extra in [
        vec!["--unknown-risk-option", "1"],
        vec!["--exclude-stream", "4073:0"],
        vec!["--exclude-stream", "4073:invalid:1"],
        vec!["--exclude-stream", "4073:18446744073709551615:2"],
    ] {
        let mut args = benchmark_args("tabular", &absent);
        args.extend(words(&extra));
        let output = run(&args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "accepted {extra:?}: {stderr}");
        assert!(!stderr.contains("No such file"), "invalid {extra:?} reached model loading: {stderr}");
        assert!(!stderr.trim().is_empty(), "invalid {extra:?} lacked an explanation");
    }
}

#[test]
fn excluded_stream_collisions_are_rejected_before_model_loading_even_after_a_stop() {
    let directory = TestDirectory::new();
    let absent = directory.checkpoint("must-not-be-opened.bin");
    let other_master = (4073_u64 ^ DEAL_STRIDE).to_string();
    // These two raw seeds produce the same initialized RNG because zero is
    // replaced by its fixed fallback. Equal raw seeds are not the only alias.
    let alias_a = 0x61c8864680b583eb_u64.to_string();
    let alias_b = 0xcd36da00563c5ef6_u64.to_string();
    for (seed, exclusion) in [
        ("4073".to_owned(), "4073:8:1".to_owned()),
        (other_master, "4073:0:12".to_owned()),
        (alias_b.clone(), format!("{alias_a}:0:1")),
        (alias_a, format!("{alias_b}:0:1")),
    ] {
        let mut args = benchmark_args("tabular", &absent);
        set(&mut args, "--seed", &seed);
        set(&mut args, "--stop-loss-usd", "1");
        args.extend(words(&["--exclude-stream", "811:0:12", "--exclude-stream", &exclusion]));
        let output = run(&args);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(2), "accepted overlapping seed {seed}");
        assert!(stderr.contains("overlap"),
            "all planned sessions must be checked before opening a model, even with zero affordable hands: {stderr}");
        assert!(!stderr.contains("No such file"), "overlap validation ran too late: {stderr}");
        assert!(session_rows(&String::from_utf8_lossy(&output.stdout)).is_empty());
    }
}

#[test]
fn disjoint_repeated_exclusions_preserve_results_and_session_blocks() {
    let directory = TestDirectory::new();
    let checkpoint = directory.checkpoint("candidate.bin");
    train("tabular", &checkpoint, "711");
    let before = std::fs::read(&checkpoint).unwrap();
    let mut args = benchmark_args("tabular", &checkpoint);
    set(&mut args, "--opponent", "tight_aggressive");
    set(&mut args, "--sessions", "7");
    set(&mut args, "--hands", "1");
    let without_exclusions = succeeds(&args);
    // A half-open training interval beginning at seven is disjoint from all
    // seven planned evaluation deals. Repeating this option is intentional.
    args.extend(words(&["--exclude-stream", "4073:7:12", "--exclude-stream", "811:0:12"]));
    let with_exclusions = succeeds(&args);
    assert_session_accounting(&with_exclusions, 7, 1);
    assert_eq!(session_rows(&with_exclusions), session_rows(&without_exclusions));
    assert_eq!(summary(&with_exclusions), summary(&without_exclusions));
    assert_eq!(std::fs::read(&checkpoint).unwrap(), before);
}

#[test]
fn neural_recorded_streams_reject_reuse_including_other_master_seeds_and_rng_aliases() {
    let directory = TestDirectory::new();
    for (name, training_seed, evaluation_seeds) in [
        ("ordinary.bin", "803".to_owned(), vec!["803".to_owned(), (803_u64 ^ DEAL_STRIDE).to_string()]),
        ("rng-alias.bin", 0x61c8864680b583eb_u64.to_string(), vec![0xcd36da00563c5ef6_u64.to_string()]),
    ] {
        let checkpoint = directory.checkpoint(name);
        train("neural", &checkpoint, &training_seed);
        let before = std::fs::read(&checkpoint).unwrap();
        for evaluation_seed in evaluation_seeds {
            let mut args = benchmark_args("neural", &checkpoint);
            set(&mut args, "--seed", &evaluation_seed);
            let output = run(&args);
            let stderr = String::from_utf8_lossy(&output.stderr);
            assert_eq!(output.status.code(), Some(2), "accepted recorded training stream {evaluation_seed}");
            assert!(stderr.contains("overlap"), "{stderr}");
            assert!(session_rows(&String::from_utf8_lossy(&output.stdout)).is_empty());
            assert_eq!(std::fs::read(&checkpoint).unwrap(), before,
                "rejected evaluation changed recorded training provenance");
        }
    }
}
