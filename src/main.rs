mod selfplay;
mod evaluator;
mod phh;
mod imitation;
mod phh_eval;
mod phh_compare;
mod sixmax;
mod neural;
mod accounting;
use std::env;
use std::time::Instant;

const START_STACK: i64 = 200;
const SMALL_BLIND: i64 = 1;
const BIG_BLIND: i64 = 2;

#[derive(Clone)]
struct Rng(u64);
impl Rng {
    fn new(seed: u64) -> Self {
        // SplitMix64 finalizer prevents correlated initial states from nearby seeds.
        let mut z = seed.wrapping_add(0x9e3779b97f4a7c15);
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        let mixed = z ^ (z >> 31);
        Self(if mixed == 0 { 0x9e3779b97f4a7c15 } else { mixed })
    }
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x >> 12; x ^= x << 25; x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545f4914f6cdd1d)
    }
    fn range(&mut self, n: usize) -> usize {
        assert!(n > 0, "random range must be nonempty");
        let bound = n as u64;
        let threshold = bound.wrapping_neg() % bound;
        loop {
            let r = self.next();
            if r >= threshold { return (r % bound) as usize; }
        }
    }
    fn chance(&mut self, p: f64) -> bool { (self.next() as f64 / u64::MAX as f64) < p }
}
fn deck(rng: &mut Rng) -> [u8; 52] {
    let mut cards = [0; 52];
    for (i, card) in cards.iter_mut().enumerate() { *card = i as u8; }
    for i in (1..52).rev() { let j = rng.range(i + 1); cards.swap(i, j); }
    cards
}
fn rank(card: u8) -> usize { (card % 13) as usize + 2 }
fn suit(card: u8) -> usize { (card / 13) as usize }
use evaluator::{score5,score7};
#[derive(Default, Clone, Copy)]
struct Stats { won: u64, folds: u64, showdowns: u64, net: i64, actions: u64 }
struct Table {
    hole: [[u8; 2]; 2],
    board: [u8; 5],
    deck: [u8; 52],
    pos: usize,
    visible: usize,
    stack: [i64; 2],
    street: [i64; 2],
    invested: [i64; 2],
    folded: Option<usize>,
    actions: [u64; 2],
}
impl Table {
    fn new(rng: &mut Rng, sb: usize) -> Self {
        let d = deck(rng);
        let mut t = Self {
            hole: [[d[0],d[2]], [d[1],d[3]]],
            board: [d[4],d[5],d[6],d[7],d[8]],
            deck: d, pos: sb, visible: 0,
            stack: [START_STACK; 2], street: [0; 2], invested: [0; 2],
            folded: None, actions: [0; 2],
        };
        t.put(sb, SMALL_BLIND);
        t.put(1-sb, BIG_BLIND);
        t
    }
    fn put(&mut self, player: usize, amount: i64) {
        let n = amount.max(0).min(self.stack[player]);
        self.stack[player] -= n;
        self.street[player] += n;
        self.invested[player] += n;
    }
    fn pot(&self) -> i64 { self.invested[0] + self.invested[1] }
    fn call_amount(&self, p: usize) -> i64 { (self.street[1-p] - self.street[p]).max(0) }
    fn equity(&self, p: usize, samples: usize, rng: &mut Rng) -> f64 {
        // Monte Carlo versus a uniformly sampled opponent range, not CFR/GTO.
        // Zero allocations per sample. Known cards removed once, partial
        // Fisher-Yates sampling draws opponent + unknown community cards.
        let mut excluded = 0u64;
        for &c in &self.hole[p] { excluded |= 1u64 << c; }
        for &c in &self.board[..self.visible] { excluded |= 1u64 << c; }
        let mut pool = [0u8; 52];
        let mut remaining=0usize;
        for card in 0..52u8 {
            if excluded & (1u64 << card) == 0 {
                pool[remaining]=card;
                remaining+=1;
            }
        }
        let missing=5-self.visible;
        let mut won=0u64;
        for _ in 0..samples {
            // Reusing the previously permuted pool is unbiased: every prefix
            // is a uniform sample without replacement.
            for k in 0..missing+2 {
                let j=k+rng.range(remaining-k);
                pool.swap(k,j);
            }
            let mut board=self.board;
            board[self.visible..].copy_from_slice(&pool[2..missing+2]);
            let ours=score7(&[self.hole[p][0],self.hole[p][1],board[0],board[1],board[2],board[3],board[4]]);
            let theirs=score7(&[pool[0],pool[1],board[0],board[1],board[2],board[3],board[4]]);
            won+=if ours>theirs {2} else if ours==theirs {1} else {0};
        }
        won as f64 / (2*samples) as f64
    }
    fn betting(&mut self, first: usize, rng: &mut Rng, simulations: usize, verbose: bool) {
        // Heads-up: each player acts once unless a bet/raise reopens action.
        let mut actor = first;
        let mut pending = 2usize;
        let mut min_raise = BIG_BLIND;
        let mut steps = 0;
        while pending > 0 {
            if self.stack[actor] == 0 { break; }
            // When an opponent is all-in, the remaining player must still
            // respond to an outstanding bet, but cannot bet into an empty side pot.
            if self.stack[1-actor] == 0 && self.call_amount(actor) == 0 { break; }
            steps += 1;
            assert!(steps < 1000, "betting round did not terminate");
            let call = self.call_amount(actor);
            let max_extra = self.stack[actor].min(call + self.stack[1-actor]);
            let equity = self.equity(actor, simulations, rng);
            let pot = self.pot();
            let odds = call as f64 / (pot+call).max(1) as f64;
            let tight = actor == 0;
            let fold = call > 0 && equity + if tight { 0.025 } else { 0.06 } < odds;
            let want_raise = if tight {
                equity > if call > 0 { 0.76 } else { 0.65 }
            } else {
                equity > if call > 0 { 0.69 } else { 0.56 } ||
                    (equity < 0.43 && rng.chance(0.12))
            };
            let raise_size = (pot / 2).max(min_raise);
            let can_raise = max_extra > call && max_extra - call >= min_raise;
            let action;
            if fold {
                self.folded = Some(actor);
                action = "fold".to_string();
            } else if want_raise && can_raise && steps < 20 {
                let extra = (call + raise_size).min(max_extra);
                let previous_high = self.street[1-actor];
                self.put(actor, extra);
                let increment = self.street[actor] - previous_high;
                min_raise = increment;
                pending = 1;
                action = format!("raise-to {}", self.street[actor]);
            } else {
                self.put(actor, call.min(max_extra));
                pending -= 1;
                action = if call == 0 { "check".to_string() } else { "call".to_string() };
            }
            self.actions[actor] += 1;
            if verbose { println!("  Bot{} equity={:.3} pot={} -> {}", actor, equity, pot, action); }
            if self.folded.is_some() { break; }
            actor = 1-actor;
        }
    }
    fn play(&mut self, rng: &mut Rng, simulations: usize, verbose: bool) -> ([i64;2], Option<usize>, bool) {
        if verbose {
            println!("Hole: bot0 {} {}, bot1 {} {}", fmt(self.hole[0][0]),fmt(self.hole[0][1]),fmt(self.hole[1][0]),fmt(self.hole[1][1]));
        }
        for street in 0..4 {
            if street > 0 {
                self.street = [0; 2];
                self.visible = match street { 1=>3, 2=>4, _=>5 };
                if verbose {
                    println!("Board: {}",self.board[..self.visible].iter().map(|&c|fmt(c)).collect::<Vec<_>>().join(" "));
                }
            }
            self.betting(if street==0 { self.pos } else { 1-self.pos }, rng, simulations, verbose);
            if self.folded.is_some() { break; }
        }
        let mut winnings = [0i64; 2];
        if let Some(loser) = self.folded {
            winnings[1-loser] = self.pot();
            return (winnings, Some(1-loser), false);
        }
        // Refund unmatched contributions; keep invested unchanged for net accounting.
        for p in 0..2 {
            if self.invested[p] > self.invested[1-p] {
                let refund = self.invested[p] - self.invested[1-p];
                winnings[p] += refund;
            }
        }
        let pot = 2 * self.invested[0].min(self.invested[1]);
        let scores = [0,1].map(|p|score7(&[
            self.hole[p][0],self.hole[p][1],self.board[0],self.board[1],
            self.board[2],self.board[3],self.board[4]
        ]));
        let winner = if scores[0] > scores[1] { Some(0) }
            else if scores[1] > scores[0] { Some(1) } else { None };
        if let Some(p) = winner { winnings[p] += pot; }
        else {
            winnings[0] += pot / 2;
            winnings[1] += pot - pot/2;
        }
        (winnings,winner,true)
    }
}
fn fmt(card: u8) -> String {
    let r = b"23456789TJQKA"[(card%13) as usize] as char;
    let s = b"cdhs"[(card/13) as usize] as char;
    format!("{r}{s}")
}
fn arg<T: std::str::FromStr>(args: &[String], flag: &str, default: T) -> T {
    args.windows(2).find(|w|w[0]==flag).and_then(|w|w[1].parse().ok()).unwrap_or(default)
}
fn main() {
    let args: Vec<String> = env::args().collect();
    if args.iter().any(|a|a=="--help" || a=="-h") {
        println!("solverpk --hands 10000 --seed 42 --samples 64 --verbose");
        println!("solverpk --sixmax-train --hands 100000 --checkpoint sixmax.bin --rake-bps 500 --rake-cap-bb 3");
        println!("solverpk --sixmax-neural-train --hands 20000 --checkpoint sixmax-neural.bin --rake-bps 500 --rake-cap-bb 3");
        println!("solverpk --sixmax-neural-league --checkpoint neural-v2.bin --hands 120000 --seed 2026111901 --rake-bps 500 --rake-cap-bb 3 [--rival-checkpoint historical.bin]");
        println!("Neural training creates SPKNN002; legacy SPKNN001 is evaluation-only. Resume continues recorded deal indices.");
        println!("solverpk --sixmax-neural-distill --teacher sixmax.bin --hands 100000 --checkpoint sixmax-neural.bin --rake-bps 0");
        println!("solverpk --sixmax-neural-eval --hands 6000 --checkpoint sixmax-neural.bin --tabular sixmax.bin --rake-bps 500 --rake-cap-bb 3");
        println!("solverpk --sixmax-eval --hands 60000 --checkpoint sixmax.bin --rake-bps 0");
        println!("solverpk --sixmax-bankroll-eval --checkpoint sixmax.bin --hands 120000 --opponent phh --phh-model imitation.bin --bankroll-usd 50 --max-bet-usd 10 --stop-loss-usd 50 --rake-bps 500 --rake-cap-bb 3");
        println!("solverpk --sixmax-duel --left scratch.bin --right warmstart.bin --hands 120000 --rake-bps 0");
        println!("Sixmax requires explicit --rake-bps (500 = 5%, 0 = none); positive rake requires --rake-cap-bb. Default no-flop-no-drop; --rake-preflop overrides it.");
        println!("Sixmax bb/100 is net after rake. The 5%/3BB example is a configurable scenario, not a specific poker room tariff.");
        println!("solverpk --self-play --hands 10000 --rooms 64 --agents 8 --samples 8 --checkpoint selfplay.bin");
        println!("solverpk --self-play --resume --hands 10000 --rooms 64 --agents 8 --checkpoint selfplay.bin");
        println!("solverpk --duel --left 1m.bin --right 11m.bin --hands 100000 --samples 8");
        println!("solverpk --import-phh --source data/pluribus --output observations.bin --max-files 1000");
        println!("solverpk --train-phh --source observations.bin --output imitation.bin");
        println!("solverpk --evaluate-phh --model imitation.bin --candidate phh|uniform|mc --hands 100000 --samples 32");
        println!("solverpk --compare-phh-historical --source observations.bin --model-10m 10m.bin --model-20m 20m.bin --samples 32");
        println!("solverpk --baseline-test --checkpoint selfplay.bin --baseline random|mc|clone --hands 100000 --samples 16");
        println!("Bot0: tight/aggressive. Bot1: loose/aggressive with occasional bluffs.");
        println!("Heads-up cash simulation, 100 BB stacks, no rake. Every hand resets stacks.");
        return;
    }
    if args.iter().any(|a| a=="--sixmax-neural-distill") {
        if let Err(e)=sixmax::neural_distill(&args) {eprintln!("Distillation error: {e}");std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--sixmax-neural-train") {
        if let Err(e)=sixmax::neural_train(&args) {eprintln!("Neural training error: {e}");std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--sixmax-neural-league") {
        if let Err(e)=sixmax::neural_league(&args) {eprintln!("Neural league error: {e}");std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--sixmax-neural-eval") {
        if let Err(e)=sixmax::neural_eval(&args) {eprintln!("Neural evaluation error: {e}");std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--sixmax-duel") {
        if let Err(e)=sixmax::duel(&args) {eprintln!("6max duel error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--sixmax-bankroll-eval") {
        if let Err(e)=sixmax::bankroll_eval(&args) {eprintln!("Bankroll evaluation error: {e}");std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--sixmax-eval") {
        if let Err(e)=sixmax::evaluate(&args) {eprintln!("6max eval error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--sixmax-train") {
        if let Err(e)=sixmax::run(&args) {eprintln!("6max error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--compare-phh-historical") {
        if let Err(e)=phh_compare::run(&args) {eprintln!("Historical comparison error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--evaluate-phh") {
        if let Err(e)=phh_eval::run(&args) {eprintln!("PHH evaluation error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--train-phh") {
        if let Err(e)=imitation::run(&args) {eprintln!("Imitation error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--import-phh") {
        if let Err(e)=phh::run(&args) {eprintln!("PHH import error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--baseline-test") {
        if let Err(e)=selfplay::baseline_test(&args) {eprintln!("Baseline error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--duel") {
        if let Err(e)=selfplay::duel(&args) {eprintln!("Duel error: {e}"); std::process::exit(2);}
        return;
    }
    if args.iter().any(|a| a=="--self-play") {
        if let Err(e)=selfplay::run(&args) {eprintln!("Self-play error: {e}"); std::process::exit(2);}
        return;
    }
    let hands: usize = arg(&args,"--hands",1000);
    let seed: u64 = arg(&args,"--seed",42);
    let samples: usize = arg(&args,"--samples",64);
    let verbose = args.iter().any(|a|a=="--verbose");
    if hands==0 || samples==0 { eprintln!("--hands and --samples must be positive"); std::process::exit(2); }
    let mut shuffle_rng=Rng::new(seed);
    let mut bot_rng=Rng::new(seed ^ 0xd1b54a32d192ed03);
    let mut stats=[Stats::default(); 2];
    let start=Instant::now();
    for hand in 0..hands {
        let mut t=Table::new(&mut shuffle_rng,hand%2);
        if verbose { println!("Hand {} (SB=bot{})",hand+1,hand%2); }
        let (win,winner,showdown)=t.play(&mut bot_rng,samples,verbose);
        let net=[0,1].map(|p|win[p]-t.invested[p]);
        // Winnings include unmatched refunds; invested retains all debited chips.
        for p in 0..2 {
            stats[p].net += net[p];
            stats[p].actions += t.actions[p];
            if winner==Some(p) {stats[p].won+=1;}
            if showdown {stats[p].showdowns+=1;}
            if t.folded==Some(p) {stats[p].folds+=1;}
        }
        assert_eq!(net[0]+net[1],0,"chip conservation");
        if verbose { println!("Net: bot0={} bot1={}\n",net[0],net[1]); }
    }
    let secs=start.elapsed().as_secs_f64();
    println!("Hands: {hands} | Seed: {seed} | MC samples/action: {samples}");
    println!("Elapsed: {:.3}s | Throughput: {:.1} hands/s",secs,hands as f64/secs.max(1e-9));
    for p in 0..2 {
        let s=stats[p];
        let bb100=s.net as f64 / BIG_BLIND as f64 * 100.0 / hands as f64;
        println!("Bot{p}: net={} chips | {:+.3} bb/100 | wins={} | folds={} | showdowns={} | actions={}",
            s.net,bb100,s.won,s.folds,s.showdowns,s.actions);
    }
    println!("Note: Monte Carlo equity versus random hands is not a GTO solver.");
}
#[cfg(test)]
mod tests {
    use super::*;
    fn c(r:usize,s:usize)->u8 { (s*13+r-2) as u8 }
    #[test] fn rng_seed_mixing_and_shuffle_invariants() {
        let mut a=Rng::new(77101);
        let mut b=Rng::new(77102);
        assert_ne!(a.next(),b.next());
        let mut unique=std::collections::HashSet::new();
        for seed in 0..200 {
            let mut rng=Rng::new(seed);
            let d=deck(&mut rng);
            assert_eq!(d.iter().copied().collect::<std::collections::HashSet<_>>().len(),52);
            unique.insert(d);
        }
        assert_eq!(unique.len(),200);
    }
    #[test] fn all_hand_ranking_categories() {
        assert!(score5(&[c(10,0),c(11,0),c(12,0),c(13,0),c(14,0)])
            > score5(&[c(9,0),c(9,1),c(9,2),c(9,3),c(14,1)]));
        let flush = score5(&[c(2,0),c(5,0),c(8,0),c(11,0),c(13,0)]);
        let straight=score5(&[c(5,0),c(6,1),c(7,2),c(8,0),c(9,1)]);
        assert!(flush > straight);
        let full=score5(&[c(10,0),c(10,1),c(10,2),c(8,0),c(8,1)]);
        assert!(full > flush);
        let trips=score5(&[c(10,0),c(10,1),c(10,2),c(8,0),c(7,1)]);
        assert!(straight > trips);
    }
    #[test] fn straight_flush_beats_quads() {
        let sf=score5(&[c(10,0),c(11,0),c(12,0),c(13,0),c(14,0)]);
        let quad=score5(&[c(9,0),c(9,1),c(9,2),c(9,3),c(14,0)]);
        assert!(sf>quad);
    }
    #[test] fn wheel_is_straight() {
        let wheel=score5(&[c(14,0),c(2,1),c(3,2),c(4,0),c(5,3)]);
        let pair=score5(&[c(14,0),c(14,1),c(9,2),c(4,0),c(5,3)]);
        assert!(wheel>pair);
    }
    #[test] fn seven_card_best_five() {
        let cards=[c(14,0),c(13,0),c(12,0),c(11,0),c(10,0),c(2,1),c(3,2)];
        assert_eq!(score7(&cards),score5(&[cards[0],cards[1],cards[2],cards[3],cards[4]]));
    }
    #[test] fn all_in_raise_requires_opponent_response() {
        let mut rng=Rng::new(123);
        let mut table=Table::new(&mut rng,0);
        // Player 0 shoved 200 chips; player 1 must call/fold before showdown.
        table.stack=[0,198];
        table.street=[200,2];
        table.invested=[200,2];
        table.betting(1,&mut rng,8,false);
        assert!(table.actions[1] >= 1,"all-in bet was not answered");
        assert!(table.folded==Some(1) || table.street[1]>2);
    }
    #[test] fn deterministic_game_and_chip_conservation() {
        let mut a=Rng::new(123);
        let mut b=Rng::new(456);
        for i in 0..50 {
            let mut table=Table::new(&mut a,i%2);
            let (win,_,_)=table.play(&mut b,8,false);
            assert_eq!(win[0]+win[1]-table.pot(),0);
        }
    }
}
