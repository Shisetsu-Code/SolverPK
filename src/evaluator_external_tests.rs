use super::evaluate;

const STRENGTHS: usize = 7_462;
const MIN_RANDOM_PER_SIZE: usize = 10_000;

struct Case {
    kind: String,
    name: String,
    strength: usize,
    cards: Vec<u8>,
}

/// An external strength is an ordinal, not SolverPK's packed score. First
/// establish a strictly increasing bijection using all 7,462 five-card
/// strengths, then check every sampled/edge hand against its representative.
/// This catches category/tiebreak order, ties, wheels, flush selection and
/// incorrect six/seven-card reduction without duplicating the evaluator.
#[test]
#[ignore = "requires scripts/external_evaluator.py and SOLVERPK_EXTERNAL_EVALUATOR_TSV"]
fn external_evaluator_matches_treys_and_pokerkit() {
    let path = std::env::var("SOLVERPK_EXTERNAL_EVALUATOR_TSV")
        .expect("set SOLVERPK_EXTERNAL_EVALUATOR_TSV to the generated external fixture");
    let text = std::fs::read_to_string(&path).expect("read external evaluator fixture");
    let mut lines = text.lines();
    assert_eq!(lines.next(), Some("# solverpk-external-evaluator-v1"));

    let mut cases = Vec::new();
    let mut pokerkit = false;
    let mut treys = false;
    let mut declared_random = None;
    let mut declared_rows = None;
    for (offset, line) in lines.enumerate() {
        let line_number = offset + 2;
        if line.starts_with('#') {
            let fields: Vec<_> = line.split('\t').collect();
            match fields.as_slice() {
                ["# engine", "PokerKit", "abf795299eaac41183495a0956ce7136ce38ed64"] => {
                    assert!(!pokerkit, "duplicate PokerKit provenance");
                    pokerkit = true;
                }
                ["# engine", "Treys", "70fbaade2f9b63ee3ea41b54aed7be2c921add7e"] => {
                    assert!(!treys, "duplicate Treys provenance");
                    treys = true;
                }
                ["# random_per_size", count] => {
                    assert!(declared_random.is_none(), "duplicate random sample count");
                    declared_random = Some(count.parse::<usize>().expect("random sample count"));
                }
                ["# rows", count] => {
                    assert!(declared_rows.is_none(), "duplicate row count");
                    declared_rows = Some(count.parse::<usize>().expect("row count"));
                }
                _ => {}
            }
            continue;
        }
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 5, "malformed fixture line {line_number}");
        let treys_rank = fields[2].parse::<usize>().expect("Treys rank");
        let pokerkit_index = fields[3].parse::<usize>().expect("PokerKit rank");
        assert!((1..=STRENGTHS).contains(&treys_rank), "line {line_number}");
        assert!(pokerkit_index < STRENGTHS, "line {line_number}");
        let strength = STRENGTHS + 1 - treys_rank;
        assert_eq!(strength, pokerkit_index + 1, "external oracles disagree at line {line_number}");
        let cards: Vec<u8> = fields[4].split(',')
            .map(|value| value.parse::<u8>().expect("numeric card ID"))
            .collect();
        assert!((5..=7).contains(&cards.len()), "line {line_number}");
        let mut seen = 0u64;
        for &card in &cards {
            assert!(card < 52, "out-of-deck card at line {line_number}");
            assert_eq!(seen & (1u64 << card), 0, "duplicate card at line {line_number}");
            seen |= 1u64 << card;
        }
        cases.push(Case {
            kind: fields[0].to_owned(),
            name: fields[1].to_owned(),
            strength,
            cards,
        });
    }
    assert!(pokerkit && treys, "fixture must identify both pinned external engines");
    assert_eq!(Some(cases.len()), declared_rows, "truncated or extended fixture");
    let random_per_size = declared_random.expect("fixture declares its sample size");
    assert!(random_per_size >= MIN_RANDOM_PER_SIZE, "at least 30,000 random hands are required");

    let mut canonical_scores = vec![None; STRENGTHS + 1];
    let mut canonical_count = 0;
    for case in cases.iter().filter(|case| case.kind == "canonical") {
        assert_eq!(case.cards.len(), 5, "canonical representative must have five cards");
        let score = evaluate(&case.cards);
        assert!(canonical_scores[case.strength].replace(score).is_none(),
            "duplicate strength {}", case.strength);
        canonical_count += 1;
    }
    assert_eq!(canonical_count, STRENGTHS, "all five-card strengths must be represented");
    for strength in 1..=STRENGTHS {
        let score = canonical_scores[strength].expect("missing canonical strength");
        if strength > 1 {
            assert!(score > canonical_scores[strength - 1].unwrap(),
                "SolverPK ordering/ties disagree with both engines at external strength {strength}");
        }
    }

    let mut random_counts = [0usize; 3];
    let mut edge_count = 0;
    for case in &cases {
        match case.kind.as_str() {
            "canonical" => continue,
            "random" => random_counts[case.cards.len() - 5] += 1,
            "edge" => edge_count += 1,
            other => panic!("unknown fixture kind {other}"),
        }
        assert_eq!(evaluate(&case.cards), canonical_scores[case.strength].unwrap(),
            "both external engines disagree with SolverPK: case={} strength={} cards={:?}",
            case.name, case.strength, case.cards);
    }
    assert_eq!(random_counts, [random_per_size; 3]);
    assert!(edge_count >= 1_000, "missing suit/card-order edge cases");
    println!(
        "EXTERNAL_EVALUATOR canonical={} random5={} random6={} random7={} edge={} total={} engines=PokerKit,Treys",
        canonical_count, random_counts[0], random_counts[1], random_counts[2], edge_count, cases.len(),
    );
}
