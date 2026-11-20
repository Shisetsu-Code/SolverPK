//! Session-level bankroll controls for frozen six-max evaluations.
//! Reserve the actual table buy-in, subject to the USD 10 maximum wager.
//! Money accounting remains integer-valued in accounting::MONEY_SCALE units.
use super::*;
pub(super) const USD_UNITS:i64=BB_UNITS*10; // BB is USD 0.10: USD 1 = 10 BB.
const BUYIN_UNITS:i64=STACK*accounting::MONEY_SCALE; // 100 BB = USD 10.
const DEFAULT_BANKROLL_USD:i64=50;
const DEFAULT_MAX_WAGER_USD:i64=10;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub(super) struct Limits {
    pub(super) bankroll:i64,
    pub(super) stop_loss:i64,
    pub(super) max_bet:i64,
}
impl Limits {
    fn from_dollars(bankroll:i64,stop_loss:i64,max_bet:i64)->io::Result<Self>{
        if bankroll<=0 || stop_loss<=0 || stop_loss>bankroll || max_bet!=DEFAULT_MAX_WAGER_USD {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,
                "bankroll must be positive, stop-loss in (0,bankroll], and maximum wager cap exactly USD 10"));
        }
        let to_units=|usd:i64| usd.checked_mul(USD_UNITS).ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"USD value overflow"));
        let limits=Self {bankroll:to_units(bankroll)?,stop_loss:to_units(stop_loss)?,max_bet:to_units(max_bet)?};
        assert_eq!(limits.max_bet,BUYIN_UNITS);
        Ok(limits)
    }
    pub(super) fn from_args(args:&[String])->io::Result<Self> {
        let bankroll=neural_arg(args,"--bankroll-usd",DEFAULT_BANKROLL_USD)?;
        let stop_loss=neural_arg(args,"--stop-loss-usd",bankroll)?;
        let max_bet=neural_arg(args,"--max-bet-usd",DEFAULT_MAX_WAGER_USD)?;
        Self::from_dollars(bankroll,stop_loss,max_bet)
    }
}
#[derive(Default)]
pub(super) struct Session {
    pub(super) balance:i64,
    pub(super) initial:i64,
    pub(super) peak:i64,
    pub(super) max_drawdown:i64,
    pub(super) hands:u64,
    pub(super) hero_allins:u64,
    // Keep telemetry in engine chips for the existing bankroll CLI. Exact
    // monetary output multiplies these two fields by MONEY_SCALE.
    pub(super) max_single_action:i64,
    pub(super) max_hand_exposure:i64,
    pub(super) total_rake:i64,
    pub(super) stopped:Option<&'static str>,
}
impl Session {
    pub(super) fn new(limits:Limits)->Self{ Self{initial:limits.bankroll,balance:limits.bankroll,peak:limits.bankroll,..Self::default()} }
    fn eligible(&mut self,limits:Limits)->bool {
        self.eligible_for_buyin(limits,BUYIN_UNITS)
    }
    pub(super) fn eligible_for_buyin(&mut self,limits:Limits,buyin_units:i64)->bool {
        // CLI callers validate their stack before starting a session. Fail
        // closed as well if an internal caller supplies an invalid buy-in.
        if buyin_units<=0 || buyin_units>limits.max_bet || limits.max_bet>BUYIN_UNITS {
            self.stopped=Some("insufficient_buyin");
            return false;
        }
        if self.balance <= self.initial - limits.stop_loss {
            self.stopped=Some("stop_loss");
            return false;
        }
        if self.balance < buyin_units {
            self.stopped=Some("insufficient_buyin");
            return false;
        }
        // Reserve the actual whole buy-in before dealing: this hand must not
        // be able to take the bankroll below the configured loss floor.
        if self.balance-buyin_units < self.initial-limits.stop_loss {
            self.stopped=Some("stop_loss_budget");
            return false;
        }
        true
    }
    fn record(&mut self,net:i64,rake:i64,risk:&RiskTelemetry)->io::Result<()> {
        self.record_for_buyin(net,rake,risk,BUYIN_UNITS)
    }
    pub(super) fn record_for_buyin(&mut self,net:i64,rake:i64,risk:&RiskTelemetry,buyin_units:i64)->io::Result<()> {
        if buyin_units<=0 || buyin_units>BUYIN_UNITS {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"invalid table buy-in or wager cap"));
        }
        // All six participants start with the same fixed stack each hand.
        // Bound both a complete loss and the largest possible five-seat win.
        if net < -buyin_units || net > buyin_units*5 {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"invalid per-hand exposure or profit"));
        }
        for chips in [risk.hero_max_single_action,risk.hero_max_exposure] {
            let units=chips.checked_mul(accounting::MONEY_SCALE).ok_or_else(||
                io::Error::new(io::ErrorKind::InvalidData,"risk telemetry arithmetic overflow"))?;
            if units<0 || units>buyin_units {
                return Err(io::Error::new(io::ErrorKind::InvalidData,"hero exceeded actual table buy-in or USD 10 wager cap"));
            }
        }
        let balance=self.balance.checked_add(net).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"bankroll arithmetic overflow"))?;
        if balance<0 {return Err(io::Error::new(io::ErrorKind::InvalidData,"negative bankroll"));}
        if rake<0 {return Err(io::Error::new(io::ErrorKind::InvalidData,"negative table rake"));}
        let total_rake=self.total_rake.checked_add(rake).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"rake arithmetic overflow"))?;
        let hands=self.hands.checked_add(1).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"hand count overflow"))?;
        let hero_allins=self.hero_allins.checked_add(risk.hero_allins).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"all-in count overflow"))?;
        // Validate the whole hand before changing the session so that a
        // rejected result cannot leave a partially updated bankroll record.
        self.balance=balance;
        self.peak=self.peak.max(balance);
        self.max_drawdown=self.max_drawdown.max(self.peak-balance);
        self.hands=hands;
        self.total_rake=total_rake;
        self.hero_allins=hero_allins;
        self.max_single_action=self.max_single_action.max(risk.hero_max_single_action);
        self.max_hand_exposure=self.max_hand_exposure.max(risk.hero_max_exposure);
        Ok(())
    }
}
fn dollars(units:i64)->f64{units as f64/USD_UNITS as f64}
pub fn bankroll_eval(args:&[String])->io::Result<()> {
    let limits=Limits::from_args(args)?;
    let rake=RakeConfig::from_args(args)?;
    print_rules(rake);
    let hands:usize=neural_arg(args,"--hands",100000)?;
    let seed:u64=neural_arg(args,"--seed",42)?;
    let path=args.windows(2).find(|w|w[0]=="--checkpoint").map(|w|w[1].as_str())
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --checkpoint"))?;
    let opponent=args.windows(2).find(|w|w[0]=="--opponent").map(|w|w[1].as_str()).unwrap_or("phh");
    if hands==0 || !matches!(opponent,"random"|"phh"|"clone") {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"invalid hands or opponent"));
    }
    let frozen=load(path)?[0].clone();
    let model=if opponent=="phh" {
        let model_path=args.windows(2).find(|w|w[0]=="--phh-model").map(|w|w[1].as_str())
            .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --phh-model"))?;
        Some(FrozenPolicy::load(model_path)?)
    }else{None};
    let mut bots=std::array::from_fn(|_|Bot::new());
    bots[0]=frozen.clone();
    if opponent=="clone" {for b in bots[1..].iter_mut(){*b=frozen.clone();}}
    let mut session=Session::new(limits);
    let mut actions=[0u64;ACTIONS];
    for i in 0..hands{
        if !session.eligible(limits){break;}
        let mut risk=RiskTelemetry::default();
        let deal_seed=seed^(i as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let result=play_audited(&mut bots,deal_seed,i%N,0.0,opponent=="random",model.as_ref(),false,&mut actions,rake,Some(&mut risk));
        session.record(result.net[0],result.rake,&risk)?;
    }
    if session.stopped.is_none() && session.eligible(limits){session.stopped=Some("hands_limit");}
    let profit=session.balance-session.initial;
    let gain_per_100=if session.hands>0 {dollars(profit)*100.0/session.hands as f64}else{0.0};
    println!("SIXMAX_BANKROLL bankroll_start_usd={:.2} bankroll_end_usd={:.2} profit_usd={:+.4} stop_loss_usd={:.2} maximum_wager_usd={:.2} bb_usd=0.10 buyin_usd=10.00 hands_requested={} hands_played={} stop_reason={} max_drawdown_usd={:.4} hero_allin_actions={} hero_max_single_action_usd={:.2} hero_max_hand_exposure_usd={:.2} rake_table_usd={:.4} profit_per_100_hands_usd={:+.4} gain_bb_per_100_hands={:+.3} opponent={} seed={} checkpoint={}",
        dollars(session.initial),dollars(session.balance),dollars(profit),dollars(limits.stop_loss),dollars(limits.max_bet),
        hands,session.hands,session.stopped.unwrap(),dollars(session.max_drawdown),session.hero_allins,
        session.max_single_action as f64/BB as f64*0.10,session.max_hand_exposure as f64/BB as f64*0.10,
        dollars(session.total_rake),gain_per_100,gain_per_100/0.10,opponent,seed,path);
    println!("SIXMAX_BANKROLL_NOTE max_wager_usd=10 is the maximum TOTAL amount at risk per player per hand, not a $10 wager on each street; fixed 100BB buy-in; stop-loss is a hard cumulative loss limit relative to initial bankroll, not a peak-based trailing loss; stop_loss_budget stops before a hand whose full USD 10 buy-in could exceed the remaining loss budget; no doubling of stakes");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fixed_stack_is_a_ten_dollar_hard_cap(){
        assert_eq!(BUYIN_UNITS,USD_UNITS*10);
        let mut t=Table6::new(7,0);
        assert_eq!(t.stack[3]+t.total[3],STACK);
        for p in 0..N { assert_eq!(t.stack[p]+t.total[p],STACK); }
        let winner=accounting::settle([200,200,200,200,200,200],[false;N],[1,2,3,4,5,6],0,true,RakeConfig::NONE);
        assert_eq!(winner.net.iter().sum::<i64>(),0);
    }
    #[test]
    fn losses_are_booked_and_trigger_stop_at_limit(){
        let l=Limits::from_dollars(50,20,10).unwrap();
        let mut s=Session::new(l);
        for _ in 0..2 {assert!(s.eligible(l));s.record(-BUYIN_UNITS,0,&RiskTelemetry::default()).unwrap();}
        assert_eq!(dollars(s.balance),30.0);
        assert!(!s.eligible(l));
        assert_eq!(s.stopped,Some("stop_loss"));
    }
    #[test]
    fn stop_loss_blocks_a_buyin_that_could_exceed_the_remaining_loss_budget(){
        let l=Limits::from_dollars(50,20,10).unwrap();
        let mut s=Session::new(l);
        for _ in 0..2 {
            assert!(s.eligible(l));
            s.record(-19*USD_UNITS/2,0,&RiskTelemetry::default()).unwrap();
        }
        assert_eq!(dollars(s.balance),31.0);
        assert!(!s.eligible(l),"another USD 10 buy-in could cross the USD 30 bankroll floor");
        assert_eq!(s.stopped,Some("stop_loss_budget"));
    }
    #[test]
    fn stop_loss_smaller_than_the_buyin_does_not_start_a_hand(){
        let l=Limits::from_dollars(50,1,10).unwrap();
        let mut s=Session::new(l);
        assert!(!s.eligible(l),"a USD 1 loss budget cannot fund a USD 10 hand");
        assert_eq!(s.balance,50*USD_UNITS);
        assert_eq!(s.hands,0);
        assert_eq!(s.stopped,Some("stop_loss_budget"));
    }
    #[test]
    fn cannot_play_if_full_buyin_unfunded(){
        let l=Limits::from_dollars(50,50,10).unwrap();
        let mut s=Session::new(l);
        for _ in 0..5 {assert!(s.eligible(l));s.record(-BUYIN_UNITS,0,&RiskTelemetry::default()).unwrap();}
        assert!(!s.eligible(l));assert_eq!(s.stopped,Some("stop_loss"));
        let mut z=Session::new(l);z.record(-BUYIN_UNITS*4-1,0,&RiskTelemetry::default()).unwrap_err();
    }
    #[test]
    fn early_stop_when_balance_below_buyin_even_before_stop_loss(){
        let l=Limits::from_dollars(50,50,10).unwrap();
        let mut s=Session::new(l);
        for _ in 0..4{s.record(-BUYIN_UNITS,0,&RiskTelemetry::default()).unwrap();}
        s.record(-BUYIN_UNITS/2,0,&RiskTelemetry::default()).unwrap();
        assert_eq!(dollars(s.balance),5.0);
        assert!(!s.eligible(l));assert_eq!(s.stopped,Some("insufficient_buyin"));
    }
    #[test]
    fn limits_reject_higher_wager_or_stop_above_bankroll(){
        assert!(Limits::from_dollars(50,50,11).is_err());
        assert!(Limits::from_dollars(50,51,10).is_err());
        assert!(Limits::from_dollars(50,20,10).is_ok());
    }
    #[test]
    fn session_gain_per_100_is_in_dollars_not_blinds(){
        let l=Limits::from_dollars(50,50,10).unwrap();
        let mut s=Session::new(l);
        for _ in 0..100{s.record(BB_UNITS,0,&RiskTelemetry::default()).unwrap();}
        assert_eq!(dollars(s.balance-s.initial),10.0);
    }
    #[test]
    fn actual_fifty_bb_buyin_reserves_five_dollars_instead_of_the_ten_dollar_cap(){
        let buyin=BUYIN_UNITS/2;
        let risk=RiskTelemetry {hero_allins:1,hero_max_single_action:STACK/2,hero_max_exposure:STACK/2};
        for bankroll in [5,50] {
            let limits=Limits::from_dollars(bankroll,5,10).unwrap();
            let mut session=Session::new(limits);
            assert!(session.eligible_for_buyin(limits,buyin),"a funded USD 5 buy-in fits a USD 5 remaining loss budget");
            session.record_for_buyin(-buyin,0,&risk,buyin).unwrap();
            assert_eq!(session.balance,(bankroll-5)*USD_UNITS);
            assert_eq!(session.max_drawdown,buyin);
            assert_eq!(session.max_hand_exposure,STACK/2);
            assert_eq!(session.hands,1);
            assert!(!session.eligible_for_buyin(limits,buyin));
            assert_eq!(session.stopped,Some("stop_loss"));
        }
    }
    #[test]
    fn actual_buyin_checks_remaining_balance_and_loss_budget_separately(){
        let buyin=BUYIN_UNITS/2;
        let risk=RiskTelemetry {hero_allins:0,hero_max_single_action:20,hero_max_exposure:20};
        let limits=Limits::from_dollars(6,6,10).unwrap();
        let mut session=Session::new(limits);
        assert!(session.eligible_for_buyin(limits,buyin));
        session.record_for_buyin(-USD_UNITS,0,&risk,buyin).unwrap();
        assert_eq!(session.balance,buyin);
        assert!(session.eligible_for_buyin(limits,buyin),"exactly one actual buy-in remains funded");
        session.record_for_buyin(-USD_UNITS,0,&risk,buyin).unwrap();
        assert!(!session.eligible_for_buyin(limits,buyin));
        assert_eq!(session.stopped,Some("insufficient_buyin"));

        let limits=Limits::from_dollars(50,5,10).unwrap();
        let mut session=Session::new(limits);
        assert!(session.eligible_for_buyin(limits,buyin));
        session.record_for_buyin(-USD_UNITS,0,&risk,buyin).unwrap();
        assert_eq!(session.balance,49*USD_UNITS);
        assert!(!session.eligible_for_buyin(limits,buyin),"cash alone must not override the remaining USD 4 loss budget");
        assert_eq!(session.stopped,Some("stop_loss_budget"));
    }
    #[test]
    fn actual_buyin_rejects_oversize_results_and_telemetry_without_booking_them(){
        let limits=Limits::from_dollars(50,50,10).unwrap();
        let buyin=BUYIN_UNITS/2;
        for (net,risk) in [
            (-buyin-1,RiskTelemetry::default()),
            (buyin*5+1,RiskTelemetry::default()),
            (0,RiskTelemetry {hero_max_single_action:STACK/2+1,..RiskTelemetry::default()}),
            (0,RiskTelemetry {hero_max_exposure:STACK/2+1,..RiskTelemetry::default()}),
            (0,RiskTelemetry {hero_max_exposure:i64::MAX,..RiskTelemetry::default()}),
        ] {
            let mut session=Session::new(limits);
            assert!(session.record_for_buyin(net,0,&risk,buyin).is_err());
            assert_eq!(session.balance,limits.bankroll);
            assert_eq!(session.hands,0);
            assert_eq!(session.total_rake,0);
            assert_eq!(session.max_drawdown,0);
        }
    }
    #[test]
    fn invalid_buyins_never_start_or_book_a_hand(){
        let limits=Limits::from_dollars(50,50,10).unwrap();
        for buyin in [-1,0,BUYIN_UNITS+1,i64::MAX] {
            let mut session=Session::new(limits);
            assert!(!session.eligible_for_buyin(limits,buyin));
            assert!(session.record_for_buyin(0,0,&RiskTelemetry::default(),buyin).is_err());
            assert_eq!(session.balance,limits.bankroll);
            assert_eq!(session.hands,0);
        }
        for maximum in [4*USD_UNITS,11*USD_UNITS] {
            let limits=Limits {max_bet:maximum,..limits};
            let mut session=Session::new(limits);
            assert!(!session.eligible_for_buyin(limits,BUYIN_UNITS/2));
        }
    }
}
