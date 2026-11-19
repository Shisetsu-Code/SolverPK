//! Session-level bankroll controls for frozen six-max tabular evaluations.
//! A 100 BB table buy-in is exactly the configured max wager of $10.
//! Money accounting remains integer-valued in accounting::MONEY_SCALE units.
use super::*;
const USD_UNITS:i64=BB_UNITS*10; // BB is USD 0.10: USD 1 = 10 BB.
const BUYIN_UNITS:i64=STACK*accounting::MONEY_SCALE; // 100 BB = USD 10.
const DEFAULT_BANKROLL_USD:i64=50;
const DEFAULT_MAX_WAGER_USD:i64=10;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
struct Limits { bankroll:i64,stop_loss:i64,max_bet:i64 }
impl Limits {
    fn from_dollars(bankroll:i64,stop_loss:i64,max_bet:i64)->io::Result<Self>{
        if bankroll<=0 || stop_loss<=0 || stop_loss>bankroll || max_bet!=DEFAULT_MAX_WAGER_USD {
            return Err(io::Error::new(io::ErrorKind::InvalidInput,
                "bankroll must be positive, stop-loss in (0,bankroll], and maximum bet exactly USD 10 for this fixed-100BB arena"));
        }
        let to_units=|usd:i64| usd.checked_mul(USD_UNITS).ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"USD value overflow"));
        let limits=Self {bankroll:to_units(bankroll)?,stop_loss:to_units(stop_loss)?,max_bet:to_units(max_bet)?};
        assert_eq!(limits.max_bet,BUYIN_UNITS);
        Ok(limits)
    }
    fn from_args(args:&[String])->io::Result<Self> {
        fn number(args:&[String],key:&str,default:i64)->io::Result<i64>{
            let matches:Vec<_>=args.windows(2).filter(|w|w[0]==key).collect();
            if matches.len()>1 {return Err(io::Error::new(io::ErrorKind::InvalidInput,format!("duplicate {key}")));}
            if let Some(w)=matches.first(){return w[1].parse::<i64>().map_err(|_|io::Error::new(io::ErrorKind::InvalidInput,format!("invalid {key}")));}
            if args.last().is_some_and(|s|s==key){return Err(io::Error::new(io::ErrorKind::InvalidInput,format!("missing {key}")));}
            Ok(default)
        }
        let bankroll=number(args,"--bankroll-usd",DEFAULT_BANKROLL_USD)?;
        let stop_loss=number(args,"--stop-loss-usd",bankroll)?;
        let max_bet=number(args,"--max-bet-usd",DEFAULT_MAX_WAGER_USD)?;
        Self::from_dollars(bankroll,stop_loss,max_bet)
    }
}
#[derive(Default)]
struct Session {
    balance:i64,
    initial:i64,
    peak:i64,
    max_drawdown:i64,
    hands:u64,
    hero_allins:u64,
    max_single_action:i64,
    max_hand_exposure:i64,
    total_rake:i64,
    stopped:Option<&'static str>,
}
impl Session {
    fn new(limits:Limits)->Self{ Self{initial:limits.bankroll,balance:limits.bankroll,peak:limits.bankroll,..Self::default()} }
    fn eligible(&mut self,limits:Limits)->bool {
        if self.balance <= self.initial - limits.stop_loss {
            self.stopped=Some("stop_loss");
            return false;
        }
        if self.balance < limits.max_bet {
            self.stopped=Some("insufficient_buyin");
            return false;
        }
        true
    }
    fn record(&mut self,net:i64,rake:i64,risk:&RiskTelemetry)->io::Result<()> {
        // Because all participants are 100BB fixed, the absolute loss of the
        // candidate in one deal must not exceed the full USD 10 table buy-in.
        if net < -BUYIN_UNITS || net > BUYIN_UNITS*5 {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"invalid per-hand exposure or profit"));
        }
        self.balance=self.balance.checked_add(net).ok_or_else(||io::Error::new(io::ErrorKind::InvalidData,"bankroll arithmetic overflow"))?;
        if self.balance < 0 {return Err(io::Error::new(io::ErrorKind::InvalidData,"negative bankroll"));}
        self.peak=self.peak.max(self.balance);
        self.max_drawdown=self.max_drawdown.max(self.peak-self.balance);
        self.hands+=1;
        self.total_rake+=rake;
        self.hero_allins+=risk.hero_allins;
        self.max_single_action=self.max_single_action.max(risk.hero_max_single_action);
        self.max_hand_exposure=self.max_hand_exposure.max(risk.hero_max_exposure);
        if self.max_single_action>STACK || self.max_hand_exposure>STACK {
            return Err(io::Error::new(io::ErrorKind::InvalidData,"hero exceeded USD 10 wager cap"));
        }
        Ok(())
    }
}
fn dollars(units:i64)->f64{units as f64/USD_UNITS as f64}
pub fn bankroll_eval(args:&[String])->io::Result<()> {
    let limits=Limits::from_args(args)?;
    let rake=RakeConfig::from_args(args)?;
    print_rules(rake);
    let hands:usize=super::super::arg(args,"--hands",100000);
    let seed:u64=super::super::arg(args,"--seed",42);
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
    if session.stopped.is_none(){session.stopped=Some("hands_limit");}
    let profit=session.balance-session.initial;
    let gain_per_100=if session.hands>0 {dollars(profit)*100.0/session.hands as f64}else{0.0};
    println!("SIXMAX_BANKROLL bankroll_start_usd={:.2} bankroll_end_usd={:.2} profit_usd={:+.4} stop_loss_usd={:.2} maximum_wager_usd={:.2} bb_usd=0.10 buyin_usd=10.00 hands_requested={} hands_played={} stop_reason={} max_drawdown_usd={:.4} hero_allin_actions={} hero_max_single_action_usd={:.2} hero_max_hand_exposure_usd={:.2} rake_table_usd={:.4} profit_per_100_hands_usd={:+.4} gain_bb_per_100_hands={:+.3} opponent={} seed={} checkpoint={}",
        dollars(session.initial),dollars(session.balance),dollars(profit),dollars(limits.stop_loss),dollars(limits.max_bet),
        hands,session.hands,session.stopped.unwrap(),dollars(session.max_drawdown),session.hero_allins,
        session.max_single_action as f64/BB as f64*0.10,session.max_hand_exposure as f64/BB as f64*0.10,
        dollars(session.total_rake),gain_per_100,gain_per_100/0.10,opponent,seed,path);
    println!("SIXMAX_BANKROLL_NOTE max_wager_usd=10 is the maximum TOTAL amount at risk per player per hand, not a $10 wager on each street; fixed 100BB buy-in; stop-loss is a cumulative loss limit relative to initial bankroll, not a peak-based trailing loss; no doubling of stakes");
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
}
