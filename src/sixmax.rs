//! Playable six-handed No-Limit Hold'em cash table and lightweight self-play.
//! No external dependencies. Separate from historical PHH replay and HU arena.
use crate::{deck, score7, Rng};
use crate::accounting::{self, RakeConfig, Settlement, MONEY_SCALE, BB_UNITS};
use crate::imitation::FrozenPolicy;
use crate::neural::{Net, ObservationVersion, PolicySample, TrainingKind, TrainingSegment, INPUT as NN_INPUT};
use std::fs::File;
use std::io::{self,Read,Write};
use std::time::Instant;

#[path = "sixmax_bankroll.rs"]
mod bankroll;
pub use bankroll::bankroll_eval;

const N:usize=6;
const STACK:i64=200;
const BB:i64=2;
const STATES:usize=4*5*3*3*3*4*3*3;
const ACTIONS:usize=6;
const MAGIC:&[u8;8]=b"SPK6MAX3";

fn print_rules(rake:RakeConfig){
    println!("SIXMAX_RULES engine=accounting-v2 bb_units={} rake_bps={} rake_cap_bb={:.5} no_flop_no_drop={} settlement_precision_bb=0.00005 metric=net_after_rake",
        BB_UNITS,rake.basis_points,rake.cap_units as f64/BB_UNITS as f64,rake.no_flop_no_drop);
}

// Inputs are sums of per-cluster rewards in big blinds. Each cluster must
// contain the same number of player-hands (6 for a seat, 36 for a 3v3 duel).
fn cluster_result(sum:f64,sumsq:f64,groups:usize,player_hands_per_group:f64)->(f64,f64){
    assert!(groups>=2,"at least two independent clusters are required");
    let mean=sum/groups as f64;
    let variance=((sumsq-sum*mean)/(groups-1) as f64).max(0.0);
    (mean*100.0/player_hands_per_group,
     1.96*(variance/groups as f64).sqrt()*100.0/player_hands_per_group)
}

#[cfg(test)]
#[path = "sixmax_audit_tests.rs"]
mod audit_tests;

#[cfg(test)]
#[path = "sixmax_stress_tests.rs"]
mod stress_tests;

#[cfg(test)]
#[path = "sixmax_external_tests.rs"]
mod external_tests;

#[cfg(test)]
#[path = "sixmax_observation_tests.rs"]
mod observation_tests;

#[cfg(test)]
#[path = "sixmax_neural_tests.rs"]
mod neural_tests;

#[path = "sixmax_observation.rs"]
mod observation;

#[path = "sixmax_league.rs"]
mod league;
pub use league::neural_league;

#[path = "sixmax_equity.rs"]
mod equity;

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
enum Action {Fold,Call,Raise(i64)} // Raise to street-total
#[derive(Clone)]
struct Table6 {
    hole:[[u8;2];N],board:[u8;5],visible:usize,
    stack:[i64;N],street:[i64;N],total:[i64;N],
    folded:[bool;N],pending:[bool;N],acted:[bool;N],
    button:usize,sb:usize,bb:usize,actor:usize,
    high:i64,min_raise:i64,pot:i64,round:u8,
    raise_count:u8,raise_per_seat:[u8;N],
    estimated_ranges:[[f32;3];N],
}
impl Table6 {
    fn new(seed:u64,button:usize)->Self{
        let mut rng=Rng::new(seed);
        let d=deck(&mut rng);
        let sb=(button+1)%N;let bb=(button+2)%N;
        let mut t=Self{
            hole:std::array::from_fn(|p|[d[2*p],d[2*p+1]]),
            board:[d[12],d[13],d[14],d[15],d[16]],visible:0,
            stack:[STACK;N],street:[0;N],total:[0;N],folded:[false;N],
            pending:[true;N],acted:[false;N],button,sb,bb,actor:(bb+1)%N,
            high:0,min_raise:BB,pot:0,round:0,
            raise_count:0,raise_per_seat:[0;N],
            estimated_ranges:[[0.15,0.35,0.50];N],
        };
        t.pay(sb,1);t.pay(bb,BB);t.high=BB;
        t
    }
    fn update_public_range(&mut self,p:usize,kind:usize){
        // Three hand-strength classes: premium, medium, weak.
        // Fixed public-action likelihoods are a heuristic Bayesian prior,
        // not an oracle or a learned 169-combo posterior.
        let likelihood=match kind{
            0=>[0.08f32,0.20,0.60], // fold
            1=>[0.25f32,0.52,0.32], // call/check
            _=>[0.69f32,0.30,0.09], // bet/raise
        };
        let mut posterior=[0f32;3];
        for i in 0..3{
            posterior[i]=self.estimated_ranges[p][i]*likelihood[i];
        }
        let sum=posterior.iter().sum::<f32>();
        if sum>0.0 {
            for i in 0..3{self.estimated_ranges[p][i]=posterior[i]/sum;}
        }
    }
    fn opponent_range_bucket(&self,p:usize)->usize{
        let mut strongest=0f32;
        for i in 0..N {
            if i!=p && !self.folded[i] {
                strongest=strongest.max(self.estimated_ranges[i][0]);
            }
        }
        if strongest<0.25{0}else if strongest<0.55{1}else{2}
    }
    fn pay(&mut self,p:usize,amount:i64){
        assert!((0..=self.stack[p]).contains(&amount));
        self.stack[p]-=amount;self.street[p]+=amount;
        self.total[p]+=amount;self.pot+=amount;
    }
    fn alive(&self)->usize{(0..N).filter(|&p|!self.folded[p]).count()}
    fn live_bettors(&self)->usize{(0..N).filter(|&p|!self.folded[p] && self.stack[p]>0).count()}
    fn call(&self,p:usize)->i64{(self.high-self.street[p]).max(0).min(self.stack[p])}
    fn raise_to(&self,p:usize,action:usize)->i64{
        let max_to=self.street[p]+self.stack[p];
        let pot_after_call=self.pot+self.call(p);
        let desired=match action{
            2=>self.high+(pot_after_call/3).max(self.min_raise),
            3=>self.high+((pot_after_call*3)/4).max(self.min_raise),
            4=>self.high+pot_after_call.max(self.min_raise),
            5=>max_to,
            _=>panic!("invalid bet-size action"),
        };
        desired.min(max_to)
    }
    fn legal(&self,p:usize)->[bool;ACTIONS]{
        let due=self.high-self.street[p];
        // A live stack must be able to contest money above the current bet.
        // A short caller entirely covered by that bet cannot fund a side pot.
        let opponent_live=(0..N).any(|q|q!=p && !self.folded[q]
            && self.stack[q]+self.street[q]>self.high);
        // Reopening is individual: short all-ins may cumulatively make a
        // player face a full raise since their most recent call/bet.
        let reopened=!self.acted[p] || due>=self.min_raise;
        let possible=reopened && opponent_live && self.stack[p]>due;
        let mut result=[false;ACTIONS];
        result[0]=due>0;
        result[1]=true;
        for action in 2..ACTIONS {
            if possible {
                let to=self.raise_to(p,action);
                let incr=to-self.high;
                result[action]=incr>0 && (incr>=self.min_raise || to==self.street[p]+self.stack[p]);
            }
        }
        result
    }
    fn first_pending_after(&self,after:usize)->Option<usize>{
        (1..=N).map(|k|(after+k)%N).find(|&p|self.pending[p] && !self.folded[p] && self.stack[p]>0)
    }
    fn completed(&self)->bool{
        self.alive()==1 || (0..N).all(|p|!self.pending[p] || self.folded[p] || self.stack[p]==0)
    }
    fn act(&mut self,action:Action)->Result<(),String>{
        if self.completed(){return Err("round already complete".into());}
        let p=self.actor;
        if !self.pending[p] || self.folded[p] || self.stack[p]==0{return Err("actor not eligible".into());}
        let call=self.call(p);
        let legal=self.legal(p);
        match action {
            Action::Fold if legal[0] =>{self.folded[p]=true;self.pending[p]=false;self.acted[p]=true;self.update_public_range(p,0);}
            Action::Call=>{
                self.pay(p,call);
                self.pending[p]=false;
                self.acted[p]=true;
                self.update_public_range(p,1);
            }
            Action::Raise(to) if legal[2..].iter().any(|&v|v) =>{
                let delta=to-self.street[p];
                if delta<=call || delta>self.stack[p] || to<=self.high {return Err("invalid raise amount".into());}
                let increment=to-self.high;
                let allin=delta==self.stack[p];
                if increment<self.min_raise && !allin{return Err("short raise not all-in".into());}
                self.pay(p,delta);
                self.high=to;
                self.raise_count=self.raise_count.saturating_add(1);
                self.raise_per_seat[p]=self.raise_per_seat[p].saturating_add(1);
                self.pending[p]=false;
                self.update_public_range(p,2);
                if increment>=self.min_raise {
                    self.min_raise=increment;
                    self.acted=[false;N];
                    self.acted[p]=true;
                    for q in 0..N {
                        if q!=p && !self.folded[q] && self.stack[q]>0 {self.pending[q]=true;}
                    }
                }else{
                    // A short all-in does not change the full-raise size.
                    // legal() checks cumulative reopening for each player.
                    self.acted[p]=true;
                    for q in 0..N {
                        if q!=p && !self.folded[q] && self.stack[q]>0 && self.street[q]<self.high {
                            self.pending[q]=true;
                        }
                    }
                }
            }
            _=>return Err("illegal action".into()),
        }
        if self.alive()>1 {
            if let Some(next)=self.first_pending_after(p){self.actor=next;}
        }
        Ok(())
    }
    fn next_street(&mut self)->Result<(),String>{
        if !self.completed(){return Err("outstanding betting".into());}
        if self.round>=3{return Err("already at river".into());}
        self.round+=1;
        self.visible=match self.round{1=>3,2=>4,_=>5};
        self.street=[0;N];self.high=0;self.min_raise=BB;
        self.acted=[false;N];
        let can_bet=self.live_bettors()>1;
        self.pending=std::array::from_fn(|p|can_bet && !self.folded[p] && self.stack[p]>0);
        self.actor=self.first_pending_after(self.button).unwrap_or(self.button);
        Ok(())
    }
    fn settle(&self,rake:RakeConfig)->Settlement {
        assert_eq!(self.total.iter().sum::<i64>(),self.pot,"pot/contribution mismatch");
        let ranks=std::array::from_fn(|p| {
            if self.folded[p] || self.alive()==1 {0} else {
                score7(&[self.hole[p][0],self.hole[p][1],
                    self.board[0],self.board[1],self.board[2],self.board[3],self.board[4]])
            }
        });
        accounting::settle(self.total,self.folded,ranks,self.button,self.visible>=3,rake)
    }
    #[cfg(test)]
    fn net(&self)->[i64;N]{self.settle(RakeConfig::NONE).net}
    fn texture(&self)->usize{
        if self.visible<3{return 0;}
        let mut ranks=[0u8;13];let mut suits=[0u8;4];
        for &card in &self.board[..self.visible]{
            ranks[(card%13) as usize]+=1;suits[(card/13) as usize]+=1;
        }
        if ranks.iter().any(|&n|n>=2){1}
        else if suits.iter().any(|&n|n>=3){2}
        else{3}
    }
    fn view(&self,p:usize)->View{
        let mut visible_board=[255u8;5];
        visible_board[..self.visible].copy_from_slice(&self.board[..self.visible]);
        View{hole:self.hole[p],board:visible_board,visible:self.visible,
            pot:self.pot,call:self.call(p),stack:self.stack[p],street:self.round,
            on_button:p==self.button,active:self.alive(),legal:self.legal(p),
            pressure:self.raise_count.min(2),texture:self.texture(),
            range_bucket:self.opponent_range_bucket(p)}
    }
}
#[derive(Clone,Copy)]
struct View{
    hole:[u8;2],board:[u8;5],visible:usize,pot:i64,call:i64,stack:i64,
    street:u8,on_button:bool,active:usize,legal:[bool;ACTIONS],
    pressure:u8,texture:usize,range_bucket:usize,
}
/// The checkpoint chooses its observation schema explicitly. Legacy models
/// retain their original 64 values; new models preserve each visible card.
fn neural_input(t:&Table6,p:usize,version:ObservationVersion)->[f32;NN_INPUT]{
    observation::input(t,p,version)
}
fn neural_prob(net:&Net,t:&Table6,p:usize)->[f32;ACTIONS]{
    net.forward(&neural_input(t,p,net.observation_version()),t.legal(p)).prob
}
fn print_neural_model(net:&Net){
    let (format,encoder,inputs)=match net.observation_version(){
        ObservationVersion::LegacyV1=>("SPKNN001","legacy64",64),
        ObservationVersion::SuitAwareV2=>("SPKNN002","cards128_v2",128),
    };
    println!("SIXMAX_NEURAL_MODEL format={} encoder={} inputs={} hidden=32 actions=6 provenance_complete={} steps={} updates={}",
        format,encoder,inputs,net.provenance_complete(),net.steps,net.updates);
}
fn neural_option<'a>(args:&'a [String],flag:&str)->io::Result<Option<&'a str>>{
    let mut value=None;
    for (index,argument) in args.iter().enumerate(){
        if argument==flag {
            if value.is_some(){return Err(io::Error::new(io::ErrorKind::InvalidInput,format!("duplicate {flag}")));}
            value=Some(args.get(index+1).filter(|s|!s.starts_with("--"))
                .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,format!("missing {flag} value")))?.as_str());
        }
    }
    Ok(value)
}
fn neural_arg<T:std::str::FromStr>(args:&[String],flag:&str,default:T)->io::Result<T>{
    match neural_option(args,flag)? {
        Some(value)=>value.parse().map_err(|_|io::Error::new(io::ErrorKind::InvalidInput,format!("invalid {flag} value: {value}"))),
        None=>Ok(default),
    }
}
fn training_model(path:&str,seed:u64,resume:bool)->io::Result<Net>{
    if resume{return Net::load_for_training(path);}
    match std::fs::symlink_metadata(path){
        Ok(_)=>Err(io::Error::new(io::ErrorKind::AlreadyExists,
            "checkpoint exists; use --resume for SPKNN002 or select a new --checkpoint path")),
        Err(e) if e.kind()==io::ErrorKind::NotFound=>Ok(Net::new(seed)),
        Err(e)=>Err(e),
    }
}
fn save_training_model(net:&Net,path:&str,resume:bool)->io::Result<()>{
    if resume {net.save(path)}else{net.save_new(path)}
}
#[derive(Clone)]
struct NeuralVisit {features:[f32;NN_INPUT],legal:[bool;ACTIONS],action:usize}
fn neural_reward(net_units:i64)->f32{
    net_units as f32/(BB_UNITS as f32*100.0)
}
fn play_neural(
    net:&mut Net,seed:u64,button:usize,rate:f32,
    baseline:Option<(&Bot,Option<&FrozenPolicy>)>,rake:RakeConfig,
)->Settlement{
    let mut t=Table6::new(seed,button);
    let mut rng=Rng::new(seed^0xbaba9caf_19d0e911);
    let mut trajectory:[Vec<NeuralVisit>;N]=std::array::from_fn(|_|Vec::new());
    for street in 0..4{
        if street>0 {
            if t.alive()==1 {break;}
            t.next_street().expect("street progression");
        }
        let mut iterations=0;
        while !t.completed(){
            iterations+=1;
            assert!(iterations<=200,"neural betting loop");
            let p=t.actor;
            let view=t.view(p);
            let x=neural_input(&t,p,net.observation_version());
            let legal=view.legal;
            let distribution=if let Some((tabular,phh))=baseline{
                if p==0 {net.forward(&x,legal).prob}
                else if let Some(model)=phh {
                    let coarse=[legal[0],legal[1],legal[2..].iter().any(|&v|v)];
                    let raw=model.predict_multi(view.street as usize,view.hole,view.pot,view.call,view.on_button,view.active,coarse).0;
                    let mut out=[0f32;ACTIONS];out[0]=raw[0];out[1]=raw[1];
                    let weights=[0.30,0.35,0.25,0.10];
                    let denom=(2..ACTIONS).filter(|&i|legal[i]).map(|i|weights[i-2]).sum::<f32>();
                    if denom>0.0 {
                        for i in 2..ACTIONS{
                            if legal[i]{out[i]=raw[2]*weights[i-2]/denom;}
                        }
                    }
                    out
                }else{probs(&tabular.table[state(view)],legal)}
            }else{net.forward(&x,legal).prob};
            let action=draw(distribution,&mut rng);
            let bet=if action==0{Action::Fold}else if action==1{Action::Call}
                else{Action::Raise(t.raise_to(p,action))};
            t.act(bet).expect("neural legal action");
            if rate>0.0 && baseline.is_none(){
                trajectory[p].push(NeuralVisit{features:x,legal,action});
            }
        }
        if t.alive()==1 {break;}
    }
    let settlement=t.settle(rake);
    let net_chips=settlement.net;
    if rate>0.0 && baseline.is_none(){
        let rewards=net_chips.map(neural_reward);
        let mut samples=Vec::new();
        for (p,visits) in trajectory.iter().enumerate(){
            for visit in visits {
                samples.push(PolicySample{features:&visit.features,legal:visit.legal,
                    action:visit.action,reward:rewards[p]});
            }
        }
        // Every observation/action was generated by this still-frozen policy.
        // Use each seat's own terminal return and apply one update for the hand.
        net.reinforce_hand(&samples,&rewards,rate).expect("valid on-policy neural hand");
    }
    settlement
}
/// Generate real full-game states under a frozen tabular teacher, then
/// minimize neural cross-entropy to the teacher's action distribution.
pub fn neural_distill(args:&[String])->io::Result<()>{
    let rake=RakeConfig::from_args(args)?;
    print_rules(rake);
    let teacher_path=neural_option(args,"--teacher")?
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --teacher"))?;
    let path=neural_option(args,"--checkpoint")?.unwrap_or("sixmax-neural.bin");
    let hands:usize=neural_arg(args,"--hands",100000)?;
    let seed:u64=neural_arg(args,"--seed",52)?;
    let lr:f32=neural_arg(args,"--learning-rate",0.004)?;
    if hands==0 || !lr.is_finite() || !(0.0..=0.05).contains(&lr) {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"invalid distillation configuration"));
    }
    let teachers=load(teacher_path)?;
    let resume=args.iter().any(|v|v=="--resume");
    let mut nn=training_model(path,seed,resume)?;
    let start_hand=nn.next_training_hand(seed)?;
    let count=u64::try_from(hands).map_err(|_|io::Error::new(io::ErrorKind::InvalidInput,"too many hands"))?;
    let end_hand=start_hand.checked_add(count).ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"training hand index overflow"))?;
    nn.record_training(TrainingSegment{kind:TrainingKind::Distillation,seed,start_hand,hands:count,
        learning_rate:lr,rake_bps:rake.basis_points,rake_cap_units:rake.cap_units,no_flop_no_drop:rake.no_flop_no_drop})?;
    // The tabular format does not retain all teacher training exposures.
    nn.mark_provenance_incomplete();
    print_neural_model(&nn);
    let start=Instant::now();
    let mut sums=[0f64;N];
    for h in start_hand..end_hand {
        let hand_seed=seed ^ h.wrapping_mul(0x9e3779b97f4a7c15);
        let mut table=Table6::new(hand_seed,(h%N as u64) as usize);
        let mut rng=Rng::new(hand_seed^0x4d7f291ca0b8f11d);
        for street in 0..4 {
            if street>0 {
                if table.alive()==1 {break;}
                table.next_street().expect("teacher round transition");
            }
            let mut steps=0;
            while !table.completed(){
                steps+=1;
                assert!(steps<=200,"teacher exceeded betting limit");
                let p=table.actor;
                let view=table.view(p);
                let x=neural_input(&table,p,nn.observation_version());
                let teacher_prob=probs(&teachers[p].table[state(view)],view.legal);
                let nn_prob=nn.forward(&x,view.legal).prob;
                let mut kl=0f64;
                for a in 0..ACTIONS {
                    if teacher_prob[a]>0.0 {
                        kl+=teacher_prob[a] as f64*
                          ((teacher_prob[a] as f64).ln()-(nn_prob[a] as f64).max(1e-12).ln());
                    }
                }
                sums[p]+=kl;
                nn.imitate(&x,view.legal,teacher_prob,lr)?;
                let choice=draw(teacher_prob,&mut rng);
                let act=if choice==0{Action::Fold}else if choice==1{Action::Call}
                    else{Action::Raise(table.raise_to(p,choice))};
                table.act(act).expect("teacher legal action");
            }
            if table.alive()==1{break;}
        }
        let settlement=table.settle(rake);
        assert_eq!(settlement.net.iter().sum::<i64>(),-settlement.rake);
    }
    save_training_model(&nn,path,resume)?;
    println!("SIXMAX_NEURAL_DISTILL hands={} steps={} updates={} start_hand={} end_hand={} train_seed={} provenance=unverified_teacher mean_kl_sum_per_hand={:.5} elapsed={:.3}s model={}",
        hands,nn.steps,nn.updates,start_hand,end_hand,seed,sums.iter().sum::<f64>()/hands as f64,
        start.elapsed().as_secs_f64(),path);
    Ok(())
}

pub fn neural_train(args:&[String])->io::Result<()>{
    let rake=RakeConfig::from_args(args)?;
    print_rules(rake);
    let hands:usize=neural_arg(args,"--hands",20000)?;
    let seed:u64=neural_arg(args,"--seed",4101)?;
    let lr:f32=neural_arg(args,"--learning-rate",0.0005)?;
    let path=neural_option(args,"--checkpoint")?.unwrap_or("sixmax-neural.bin");
    if hands==0 || !lr.is_finite() || !(0.0..=0.05).contains(&lr){
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"invalid hands / neural learning rate"));
    }
    let resume=args.iter().any(|v|v=="--resume");
    let mut net=training_model(path,seed,resume)?;
    let start_hand=net.next_training_hand(seed)?;
    let count=u64::try_from(hands).map_err(|_|io::Error::new(io::ErrorKind::InvalidInput,"too many hands"))?;
    let end_hand=start_hand.checked_add(count).ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"training hand index overflow"))?;
    net.record_training(TrainingSegment{kind:TrainingKind::SelfPlay,seed,start_hand,hands:count,
        learning_rate:lr,rake_bps:rake.basis_points,rake_cap_units:rake.cap_units,no_flop_no_drop:rake.no_flop_no_drop})?;
    print_neural_model(&net);
    let started=Instant::now();
    let mut total_rake=0i64;
    for hand in start_hand..end_hand{
        let deal_seed=seed ^ hand.wrapping_mul(0x9e3779b97f4a7c15);
        let result=play_neural(&mut net,deal_seed,(hand%N as u64) as usize,lr,None,rake);
        total_rake+=result.rake;
    }
    save_training_model(&net,path,resume)?;
    println!("SIXMAX_HOUSE rake_bb={:.5}",total_rake as f64/BB_UNITS as f64);
    println!("SIXMAX_NEURAL_TRAIN hands={} steps={} updates={} start_hand={} end_hand={} train_seed={} learning_rate={} reward_scale_bb=100 objective=net_after_rake update=one_frozen_hand elapsed={:.3}s hands_per_sec={:.2} checkpoint={}",
        hands,net.steps,net.updates,start_hand,end_hand,seed,lr,started.elapsed().as_secs_f64(),
        hands as f64/started.elapsed().as_secs_f64().max(1e-9),path);
    Ok(())
}
pub fn neural_eval(args:&[String])->io::Result<()>{
    let rake=RakeConfig::from_args(args)?;
    print_rules(rake);
    let path=neural_option(args,"--checkpoint")?
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --checkpoint"))?;
    let tab_path=neural_option(args,"--tabular")?
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --tabular"))?;
    let phh_path=neural_option(args,"--phh-model")?;
    let hands:usize=neural_arg(args,"--hands",6000)?;
    let seed:u64=neural_arg(args,"--seed",98722)?;
    if hands<2*N || hands%N!=0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"--hands must be a multiple of six and at least 12 (two independent clusters)"));
    }
    let phh=phh_path.map(FrozenPolicy::load).transpose()?;
    let base=load(tab_path)?[0].clone();
    let mut net=Net::load(path)?;
    let groups=hands/N;
    league::validate_neural_eval_split(&net,seed,groups)?;
    print_neural_model(&net);
    let split=if net.observation_version()==ObservationVersion::LegacyV1 {"unverified_legacy"}else{"unverified_external"};
    let started=Instant::now();
    let mut sum=0f64;
    let mut sumsq=0f64;
    let mut total_rake=0i64;
    for group in 0..groups {
        let deal_seed=seed ^ (group as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let mut group_value=0f64;
        for button in 0..N {
            let result=play_neural(&mut net,deal_seed,button,0.0,Some((&base,phh.as_ref())),rake);
            group_value+=result.net[0] as f64/BB_UNITS as f64;
            total_rake+=result.rake;
        }
        sum+=group_value;
        sumsq+=group_value*group_value;
    }
    let (avg,ci)=cluster_result(sum,sumsq,groups,N as f64);
    println!("SIXMAX_NEURAL_EVAL hands={} clusters={} candidate=neural opponent={} bb100={:+.3} ci95_cluster_half={:.3} table_rake_bb100={:.5} eval_seed={} split={} elapsed={:.3}s",
        hands,groups,if phh.is_some(){"phh_imitation"}else{"tabular"},avg,ci,
        total_rake as f64/BB_UNITS as f64*100.0/hands as f64,seed,split,started.elapsed().as_secs_f64());
    Ok(())
}

fn state(v:View)->usize{
    let a=v.hole[0]%13;
    let b=v.hole[1]%13;
    let strength=if a==b{0}else if a.max(b)>=11{1}else if a.max(b)>=8{2}else if v.hole[0]/13==v.hole[1]/13{3}else{4};
    let players=if v.active<=2{0}else if v.active<=4{1}else{2};
    let ratio=v.call as f64/(v.pot+v.call).max(1) as f64;
    let price=if v.call==0{0}else if ratio<0.28{1}else{2};
    let pos=if v.on_button{0}else if v.street==0{1}else{2};
    let mut key=v.street as usize;
    for (factor,bucket) in [
        (5,strength),(3,players),(3,price),(3,pos),
        (4,v.texture),(3,v.pressure as usize),(3,v.range_bucket)
    ] {
        key=key*factor+bucket;
    }
    key
}
#[derive(Clone,Copy)]
struct PolicyRow{weights:[f32;ACTIONS],baseline:f32,visits:u32}
impl Default for PolicyRow{
    fn default()->Self{Self{weights:[-0.8,0.5,-0.3,-0.5,-0.7,-1.2],baseline:0.0,visits:0}}
}
#[derive(Clone)]
struct Bot{table:Box<[PolicyRow]>}
impl Bot {fn new()->Self{Self{table:vec![PolicyRow::default();STATES].into_boxed_slice()}}}
fn probs(row:&PolicyRow,legal:[bool;ACTIONS])->[f32;ACTIONS]{
    let max=(0..ACTIONS).filter(|&i|legal[i]).map(|i|row.weights[i]).fold(f32::NEG_INFINITY,f32::max);
    let mut out=[0f32;ACTIONS];let mut total=0f32;
    for i in 0..ACTIONS{if legal[i]{out[i]=(row.weights[i]-max).exp();total+=out[i];}}
    for x in &mut out{*x/=total;}
    out
}
fn draw(prob:[f32;ACTIONS],rng:&mut Rng)->usize{
    let x=rng.next() as f64/u64::MAX as f64;
    let mut sum=0f64;
    for (a,p) in prob.iter().enumerate(){sum+=*p as f64;if x<sum{return a;}}
    (0..ACTIONS).rev().find(|&i|prob[i]>0.0).unwrap_or(1)
}
#[derive(Clone)]
struct Visit{key:usize,action:usize,probs:[f32;ACTIONS]}
fn reinforce(bot:&mut Bot,history:&[Visit],net:i64,rate:f32){
    if rate<=0.0{return;}
    let reward=(net as f32/(BB_UNITS as f32*20.0)).clamp(-5.0,5.0);
    for visit in history{
        let row=&mut bot.table[visit.key];
        let adv=(reward-row.baseline).clamp(-5.0,5.0);
        for a in 0..ACTIONS{
            let chosen=if a==visit.action{1.0}else{0.0};
            row.weights[a]=(row.weights[a]+rate*adv*(chosen-visit.probs[a])).clamp(-10.0,10.0);
        }
        row.baseline+=0.02*(reward-row.baseline);
        row.visits=row.visits.saturating_add(1);
    }
}
/// Project historical 6-player fold/call/raise probabilities to the six
/// discretized actions. This is only a warm-start, NOT an EV label.
fn initialize_from_phh(bots:&mut [Bot;N],model:&FrozenPolicy)->usize{
    let mut initialized=0usize;
    for key in 0..STATES {
        let mut rem=key;
        let _range=rem%3;rem/=3;
        let _pressure=rem%3;rem/=3;
        let _texture=rem%4;rem/=4;
        let position=rem%3;rem/=3;
        let price=rem%3;rem/=3;
        let players=rem%3;rem/=3;
        let strength=rem%5;rem/=5;
        let street=rem;
        let hole=match strength{
            0=>[12,25], // AA
            1=>[12,24], // AK offsuit
            2=>[9,19],  // J8 offsuit
            3=>[5,7],   // 7c9c suited
            _=>[0,14],  // 2c3d offsuit
        };
        let active=match players{0=>2,1=>4,_=>6};
        let (call,pot)=match price{0=>(0,100),1=>(20,100),_=>(70,100)};
        let legal=[call>0,true,true];
        let (p,seen)=model.predict_multi(street,hole,pot,call,position==0,active,legal);
        if !seen{continue;}
        let weights=[0.30f32,0.35,0.25,0.10];
        let spread=[p[0],p[1],p[2]*weights[0],p[2]*weights[1],p[2]*weights[2],p[2]*weights[3]];
        for bot in bots.iter_mut(){
            for a in 0..ACTIONS{
                // temper demonstrator to avoid near-deterministic overfitting
                bot.table[key].weights[a]=0.65*spread[a].max(1e-4).ln();
            }
        }
        initialized+=1;
    }
    initialized
}

#[derive(Default)]
struct RiskTelemetry {hero_allins:u64,hero_max_single_action:i64,hero_max_exposure:i64}
fn play(bots:&mut [Bot;N],seed:u64,button:usize,rate:f32,random_opponent:bool,phh_opponent:Option<&FrozenPolicy>,phh_candidate:bool,action_counts:&mut [u64;ACTIONS],rake:RakeConfig)->Settlement{
    play_audited(bots,seed,button,rate,random_opponent,phh_opponent,phh_candidate,action_counts,rake,None)
}
fn play_audited(bots:&mut [Bot;N],seed:u64,button:usize,rate:f32,random_opponent:bool,phh_opponent:Option<&FrozenPolicy>,phh_candidate:bool,action_counts:&mut [u64;ACTIONS],rake:RakeConfig,mut risk:Option<&mut RiskTelemetry>)->Settlement{
    let mut t=Table6::new(seed,button);
    if let Some(telemetry)=risk.as_deref_mut(){
        // Forced blinds are exposure even when everyone folds before hero acts.
        telemetry.hero_max_single_action=telemetry.hero_max_single_action.max(t.total[0]);
        telemetry.hero_max_exposure=telemetry.hero_max_exposure.max(t.total[0]);
    }
    let mut rng=Rng::new(seed^0xbeecafefeed55eed);
    let mut history:[Vec<Visit>;N]=std::array::from_fn(|_|Vec::new());
    for street in 0..4{
        if street>0 {
            if t.alive()==1{break;}
            t.next_street().expect("betting transition");
        }
        let mut steps=0;
        while !t.completed(){
            steps+=1;
            assert!(steps<=200,"betting loop exceeds cap");
            let p=t.actor;
            let v=t.view(p);
            let key=state(v);
            let values=if let Some(model)=phh_opponent {
                if p!=0 || phh_candidate {
                    let coarse=[v.legal[0],v.legal[1],v.legal[2..].iter().any(|&x|x)];
                    let raw=model.predict_multi(v.street as usize,v.hole,v.pot,v.call,v.on_button,v.active,coarse).0;
                    let mut spread=[0.0f32;ACTIONS];
                    spread[0]=raw[0];spread[1]=raw[1];
                    let weights=[0.30f32,0.35,0.25,0.10];
                    let normalizer=(2..ACTIONS).filter(|&a|v.legal[a])
                        .map(|a|weights[a-2]).sum::<f32>();
                    for a in 2..ACTIONS {
                        if v.legal[a] && normalizer>0.0 {spread[a]=raw[2]*weights[a-2]/normalizer;}
                    }
                    spread
                }else{probs(&bots[p].table[key],v.legal)}
            }else if random_opponent && p!=0 {
                let n=v.legal.iter().filter(|&&x|x).count() as f32;
                std::array::from_fn(|a|if v.legal[a]{1.0/n}else{0.0})
            }else{probs(&bots[p].table[key],v.legal)};
            let choice=draw(values,&mut rng);
            action_counts[choice]+=1;
            let action=match choice{
                0=>Action::Fold,
                1=>Action::Call,
                a=>Action::Raise(t.raise_to(p,a)),
            };
            let before=t.total[p];
            t.act(action).expect("policy proposed legal action");
            if p==0 {
                if let Some(telemetry)=risk.as_deref_mut(){
                    let paid=t.total[p]-before;
                    telemetry.hero_max_single_action=telemetry.hero_max_single_action.max(paid);
                    telemetry.hero_max_exposure=telemetry.hero_max_exposure.max(t.total[p]);
                    // Count calls and size-capped raises that consume the stack,
                    // not only selections of the dedicated shove action label.
                    if paid>0 && t.stack[p]==0 {telemetry.hero_allins+=1;}
                }
            }
            history[p].push(Visit{key,action:choice,probs:values});
        }
        if t.alive()==1{break;}
    }
    let settlement=t.settle(rake);
    let net=settlement.net;
    for p in 0..N {
        reinforce(&mut bots[p],&history[p],net[p],rate);
    }
    settlement
}
fn save(path:&str,bots:&[Bot;N])->io::Result<()>{
    let mut file=File::create(path)?;
    file.write_all(MAGIC)?;
    for b in bots {
        for r in &b.table{
            for x in r.weights {file.write_all(&x.to_le_bytes())?;}
            file.write_all(&r.baseline.to_le_bytes())?;
            file.write_all(&r.visits.to_le_bytes())?;
        }
    }
    Ok(())
}
fn load(path:&str)->io::Result<[Bot;N]>{
    let mut file=File::open(path)?;
    let mut magic=[0u8;8];file.read_exact(&mut magic)?;
    if &magic!=MAGIC{return Err(io::Error::new(io::ErrorKind::InvalidData,"bad 6max checkpoint"));}
    let mut bots=std::array::from_fn(|_|Bot::new());
    for bot in &mut bots {
        for row in &mut bot.table {
            for x in &mut row.weights{
                let mut b=[0u8;4];file.read_exact(&mut b)?;*x=f32::from_le_bytes(b);
                if !x.is_finite(){return Err(io::Error::new(io::ErrorKind::InvalidData,"nonfinite weight"));}
            }
            let mut b=[0u8;4];file.read_exact(&mut b)?;row.baseline=f32::from_le_bytes(b);
            if !row.baseline.is_finite(){return Err(io::Error::new(io::ErrorKind::InvalidData,"nonfinite baseline"));}
            let mut v=[0u8;4];file.read_exact(&mut v)?;row.visits=u32::from_le_bytes(v);
        }
    }
    let mut trailing=[0u8;1];
    if file.read(&mut trailing)?!=0{return Err(io::Error::new(io::ErrorKind::InvalidData,"trailing 6max checkpoint bytes"));}
    Ok(bots)
}
pub fn run(args:&[String])->io::Result<()>{
    let rake=RakeConfig::from_args(args)?;
    print_rules(rake);
    let hands:usize=super::arg(args,"--hands",100000);
    let seed:u64=super::arg(args,"--seed",42);
    let rate:f32=super::arg(args,"--learning-rate",0.025);
    let path=args.windows(2).find(|w|w[0]=="--checkpoint").map(|w|w[1].as_str()).unwrap_or("sixmax.bin");
    let resume=args.iter().any(|a|a=="--resume");
    if hands==0 || !rate.is_finite() || !(0.0..=1.0).contains(&rate){return Err(io::Error::new(io::ErrorKind::InvalidInput,"invalid hands/learning-rate"));}
    let mut bots=if resume{load(path)?}else{std::array::from_fn(|_|Bot::new())};
    if let Some(model_path)=args.windows(2).find(|w|w[0]=="--phh-init").map(|w|w[1].as_str()) {
        if resume{return Err(io::Error::new(io::ErrorKind::InvalidInput,"--phh-init and --resume are mutually exclusive"));}
        let model=FrozenPolicy::load(model_path)?;
        let count=initialize_from_phh(&mut bots,&model);
        println!("SIXMAX_WARMSTART historical_model={} initialized_buckets={} total_buckets={}",model_path,count,STATES);
    }
    let start=Instant::now();
    let mut totals=[0i64;N];
    let mut total_rake=0i64;
    let mut action_counts=[0u64;ACTIONS];
    for i in 0..hands{
        let s=seed^((i as u64).wrapping_mul(0x9e3779b97f4a7c15));
        let result=play(&mut bots,s,i%N,rate,false,None,false,&mut action_counts,rake);
        for p in 0..N{totals[p]+=result.net[p];}
        total_rake+=result.rake;
    }
    save(path,&bots)?;
    assert_eq!(totals.iter().sum::<i64>(),-total_rake);
    println!("SIXMAX_HOUSE rake_bb={:.5}",total_rake as f64/BB_UNITS as f64);
    println!("SIXMAX_ACTIONS total={} fold={} call={} bet33={} bet75={} pot={} allin={}",
        action_counts.iter().sum::<u64>(),action_counts[0],action_counts[1],
        action_counts[2],action_counts[3],action_counts[4],action_counts[5]);
    println!("SIXMAX hands={} time={:.3}s hands_per_sec={:.1} checkpoint={}",hands,
        start.elapsed().as_secs_f64(),hands as f64/start.elapsed().as_secs_f64().max(1e-9),path);
    for p in 0..N {println!("SIXMAX player={} net_units={} bb100={:+.3} visited_states={}",p,totals[p],
        totals[p] as f64/BB_UNITS as f64*100.0/hands as f64,
        bots[p].table.iter().filter(|r|r.visits>0).count());}
    Ok(())
}
pub fn evaluate(args:&[String])->io::Result<()>{
    let rake=RakeConfig::from_args(args)?;
    print_rules(rake);
    let path=args.windows(2).find(|w|w[0]=="--checkpoint").map(|w|w[1].as_str())
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing checkpoint"))?;
    let hands:usize=super::arg(args,"--hands",100002);
    let seed:u64=super::arg(args,"--seed",42);
    if hands<2*N || hands%N!=0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"hands must be divisible by 6 and at least 12"));
    }
    let original=load(path)?;
    let opponent=args.windows(2).find(|w|w[0]=="--opponent").map(|w|w[1].as_str()).unwrap_or("random");
    if opponent!="random" && opponent!="phh" && opponent!="clone" {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"opponent must be random, phh or clone"));
    }
    let phh_path=args.windows(2).find(|w|w[0]=="--phh-model").map(|w|w[1].as_str());
    let phh=if opponent=="phh" || args.iter().any(|w|w=="--candidate-phh") {
        Some(FrozenPolicy::load(phh_path.ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --phh-model"))?)?)
    }else{None};
    let candidate_phh=args.iter().any(|w|w=="--candidate-phh");
    if candidate_phh && opponent!="phh" {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"--candidate-phh requires --opponent phh"));
    }
    // One fixed candidate in seat 0, six positions per independent deal.
    let champion=original[0].clone();
    let mut bots=std::array::from_fn(|_|Bot::new());
    bots[0]=champion.clone();
    if opponent=="clone" {for bot in &mut bots[1..] {*bot=champion.clone();}}
    let groups=hands/N;
    let mut totals=0f64;
    let mut group_sq=0f64;
    let mut total_rake=0i64;
    let mut action_counts=[0u64;ACTIONS];
    for group in 0..groups {
        let pair_seed=seed ^ (group as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let mut group_result=0f64;
        for position in 0..N {
            let result=play(&mut bots,pair_seed,position,0.0,opponent=="random",phh.as_ref(),candidate_phh,&mut action_counts,rake);
            group_result+=result.net[0] as f64/BB_UNITS as f64;
            total_rake+=result.rake;
        }
        totals+=group_result;
        group_sq+=group_result*group_result;
    }
    let (mean,half)=cluster_result(totals,group_sq,groups,N as f64);
    println!("SIXMAX_EVAL_ACTIONS total={} fold={} call={} bet33={} bet75={} pot={} allin={}",
        action_counts.iter().sum::<u64>(),action_counts[0],action_counts[1],
        action_counts[2],action_counts[3],action_counts[4],action_counts[5]);
    println!("SIXMAX_EVAL hands={} clusters={} bb100={:+.3} ci95_six_hand_cluster_half={:.3} table_rake_bb100={:.5} opponent={} candidate={} eval_seed={}",
        hands,groups,mean,half,total_rake as f64/BB_UNITS as f64*100.0/hands as f64,
        opponent,if candidate_phh{"phh"}else{"selfplay"},seed);
    Ok(())
}

/// Direct frozen team duel, three seats per generation, rotating button
/// and team-seat assignment. Scores are per player per 100 hands.
pub fn duel(args:&[String])->io::Result<()>{
    let rake=RakeConfig::from_args(args)?;
    print_rules(rake);
    let left=args.windows(2).find(|w|w[0]=="--left").map(|w|w[1].as_str())
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --left"))?;
    let right=args.windows(2).find(|w|w[0]=="--right").map(|w|w[1].as_str())
        .ok_or_else(||io::Error::new(io::ErrorKind::InvalidInput,"missing --right"))?;
    let hands:usize=super::arg(args,"--hands",120000);
    let seed:u64=super::arg(args,"--seed",91832);
    if hands<4*N || hands%(2*N)!=0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput,"hands must be divisible by 12 and at least 24"));
    }
    let a=load(left)?[0].clone();
    let b=load(right)?[0].clone();
    let mut seats:[Bot;N]=std::array::from_fn(|p|if p%2==0{a.clone()}else{b.clone()});
    let mut totals=[0f64;2];
    let mut pair_sq=[0f64;2];
    let mut total_rake=0i64;
    let mut action_counts=[0u64;ACTIONS];
    let pairs=hands/(2*N);
    let start=Instant::now();
    for pair in 0..pairs {
        let deal_seed=seed ^ (pair as u64).wrapping_mul(0x9e3779b97f4a7c15);
        let mut pair_net=[0f64;2];
        for flip in 0..2 {
            if flip==1 {
                for seat in (0..N).step_by(2){seats.swap(seat,seat+1);}
            }
            for button in 0..N {
                let result=play(&mut seats,deal_seed,button,0.0,false,None,false,&mut action_counts,rake);
                for p in 0..N {
                    let team=usize::from((p%2==0)!=(flip==0));
                    pair_net[team]+=result.net[p] as f64/BB_UNITS as f64;
                }
                total_rake+=result.rake;
            }
            if flip==1 {
                for seat in (0..N).step_by(2){seats.swap(seat,seat+1);}
            }
        }
        for team in 0..2 {
            totals[team]+=pair_net[team];
            pair_sq[team]+=pair_net[team]*pair_net[team];
        }
    }
    let (left_mean,left_half)=cluster_result(totals[0],pair_sq[0],pairs,2.0*N as f64*3.0);
    let (right_mean,right_half)=cluster_result(totals[1],pair_sq[1],pairs,2.0*N as f64*3.0);
    println!("SIXMAX_DUEL left={} right={} hands={} clusters={} left_bb100={:+.3} right_bb100={:+.3} left_ci95_half={:.3} right_ci95_half={:.3} table_rake_bb100={:.5} eval_seed={} elapsed={:.3}s",
        left,right,hands,pairs,left_mean,right_mean,left_half,right_half,
        total_rake as f64/BB_UNITS as f64*100.0/hands as f64,seed,start.elapsed().as_secs_f64());
    println!("SIXMAX_DUEL_NOTE three seats per strategy; matched button positions and team assignments; net per player after rake; frozen policies; no GTO guarantee");
    Ok(())
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn blind_and_preflop_order(){
        let t=Table6::new(7,0);
        assert_eq!((t.sb,t.bb,t.actor),(1,2,3));
        assert_eq!(t.pot,3);
    }
    #[test]fn blind_rotation(){
        for i in 0..N{
            let t=Table6::new(11,i);
            assert_eq!(t.sb,(i+1)%N);
            assert_eq!(t.bb,(i+2)%N);
        }
    }
    #[test]fn fold_everyone_else_and_conserve(){
        let mut t=Table6::new(4,0);
        while t.alive()>1 {
            let p=t.actor;
            if p==t.bb {t.act(Action::Call).unwrap();}
            else if t.legal(p)[0] {t.act(Action::Fold).unwrap();}
            else{t.act(Action::Call).unwrap();}
            if t.completed(){break;}
        }
        assert_eq!(t.net().iter().sum::<i64>(),0);
    }
    #[test]fn illegal_bet_rejected(){
        let mut t=Table6::new(1,0);
        assert!(t.act(Action::Raise(3)).is_err());
        assert_eq!(t.pot,3);
    }
    #[test]fn full_raise_reopens_action(){
        let mut t=Table6::new(11,0);
        for _ in 0..5{t.act(Action::Call).unwrap();}
        assert_eq!(t.actor,2);
        t.act(Action::Raise(8)).unwrap();
        assert!(t.pending[3]);
    }
    #[test]fn sidepot_conserves(){
        let mut t=Table6::new(9,0);
        t.total=[10,20,30,30,40,50];
        t.pot=t.total.iter().sum();
        t.stack=std::array::from_fn(|p|STACK-t.total[p]);
        t.folded=[false,false,true,false,true,false];
        let w=t.settle(RakeConfig::NONE);
        assert_eq!(w.payouts.iter().sum::<i64>(),t.pot*MONEY_SCALE);
    }
    #[test]fn complete_table_reference_isolated(){
        let t=Table6::new(5,1);
        assert_eq!(t.hole.iter().flatten().copied().collect::<std::collections::HashSet<_>>().len(),12);
        let _=t.view(0);
    }
    #[test]fn future_board_is_hidden_in_observations(){
        let mut t=Table6::new(13,0);
        let before=t.view(3);
        assert_eq!(before.board,[255;5]);
        t.visible=3;
        let flop=t.view(3);
        assert_eq!(flop.board[3..],[255,255]);
        assert_eq!(flop.board[..3],t.board[..3]);
    }
    #[test]fn public_range_update_normalized(){
        let mut t=Table6::new(9,0);
        t.update_public_range(3,2);
        let r=t.estimated_ranges[3];
        assert!((r.iter().sum::<f32>()-1.0).abs()<1e-6);
        assert!(r[0]>0.15);
        assert_eq!(t.view(1).range_bucket,1);
    }
    #[test]fn six_bet_size_actions_are_legal(){
        let t=Table6::new(9,0);
        let actions=t.legal(t.actor);
        assert_eq!(actions.len(),6);
        assert!(actions[2..].iter().any(|&a|a));
        for a in 2..ACTIONS {
            if actions[a]{
                let to=t.raise_to(t.actor,a);
                assert!(to>t.high);
            }
        }
    }
    #[test]fn short_allin_does_not_reopen_raising(){
        let mut t=Table6::new(23,0);
        t.act(Action::Raise(20)).unwrap(); // seat 3 raises from BB=2
        assert_eq!(t.actor,4);
        t.stack[4]=30;
        t.act(Action::Raise(30)).unwrap(); // short all-in, only +10 over 20
        while t.actor!=3 && !t.completed(){
            t.act(Action::Call).unwrap();
        }
        assert_eq!(t.actor,3);
        let legal=t.legal(3);
        assert!(legal[1]);
        assert!(!legal[2..].iter().any(|&x|x),"short raise reopened betting");
        t.act(Action::Call).unwrap();
        assert!(t.completed());
    }
    #[test]fn phh_warmstart_is_finite_and_preserves_dimensions(){
        let bots:[Bot;N]=std::array::from_fn(|_|Bot::new());
        assert_eq!(bots[0].table.len(),STATES);
        let visible=Table6::new(31,0).view(4);
        assert!(state(visible)<STATES);
    }
    #[test]fn deterministic_three_way_sidepot_payout(){
        let mut t=Table6::new(5,0);
        t.total=[10,20,30,0,0,0];
        t.pot=60;
        t.stack=[190,180,170,200,200,200];
        t.folded=[false,false,false,true,true,true];
        // Main pot: five-high club straight flush (seat 0);
        // next pot: trip nines (seat 1); unmatched last tier (seat 2).
        t.hole[0]=[12,3]; // Ac,5c
        t.hole[1]=[46,20]; // 9s,9d
        t.hole[2]=[44,18]; // 7s,7d
        t.board=[0,1,2,16,33]; // 2c 3c 4c 5d 9h
        let paid=t.settle(RakeConfig::NONE);
        assert_eq!(paid.payouts,[300_000,200_000,100_000,0,0,0]);
    }
    #[test]fn seeded_games_conserve(){
        let mut bots=std::array::from_fn(|_|Bot::new());
        let mut counts=[0u64;ACTIONS];
        for i in 0..500 {
            let result=play(&mut bots,i*137+4,(i as usize)%N,0.03,false,None,false,&mut counts,RakeConfig::NONE);
            assert_eq!(result.net.iter().sum::<i64>(),0);
        }
    }
}
