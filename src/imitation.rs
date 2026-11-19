//! Small supervised PHH imitation baseline, separate from the Heads-Up actor critic.
//! Accepts SPKOBS02 observations, splits by source hand, and prevents hidden-card leakage.
use std::fs::File;
use std::io::{self,Read,Write,BufReader};
const MAGIC:&[u8;8]=b"SPKOBS02";
const ROW:usize=96;
const BUCKETS:usize=4*5*2*4*3*2;
const ACTIONS:usize=3;
const SMOOTH:f64=1.0;
fn invalid(s:&str)->io::Error{io::Error::new(io::ErrorKind::InvalidData,s)}
#[derive(Clone)]
struct Counts([[u64;ACTIONS];BUCKETS]);
impl Counts{
    fn new()->Self{Self([[0;ACTIONS];BUCKETS])}
    fn probability(&self,key:usize,legal:u8)->[f64;3]{
        let mut values=[0.0;3];let mut total=0.0;
        for action in 0..3{
            if legal&(1<<action)!=0{
                values[action]=self.0[key][action] as f64+SMOOTH;
                total+=values[action];
            }
        }
        if total>0.0{for x in &mut values{*x/=total;}}
        values
    }
}
fn int(record:&[u8;ROW],off:usize)->i64{i64::from_le_bytes(record[off..off+8].try_into().unwrap())}
fn key(record:&[u8;ROW])->usize{
    let street=(record[11] as usize).min(3);
    let a=record[16];let b=record[17];
    let ranks=if a==255 || b==255 {4} else if a%13==b%13 {0} else {
        let high=(a%13).max(b%13);
        if high>=11{1}else if high>=8{2}else{3}
    };
    let suited=if a!=255 && b!=255 && a/13==b/13{1}else{0};
    let pot=int(record,24).max(0) as f64;
    let call=int(record,32).max(0) as f64;
    let price=if call==0.0 {0} else if call/(pot+call).max(1.0)<0.2{1}else if call/(pot+call).max(1.0)<0.4{2}else{3};
    let players=int(record,72);
    let multi=if players<=2 {0}else if players<=4{1}else{2};
    let pos=if record[10]==record[14]{1}else{0};
    (((((street*5+ranks)*2+suited)*4+price)*3+multi)*2)+pos
}

/// Frozen behavior policy for evaluation in the Heads-Up arena.
/// PHH learned from 6-player hands; pool the player-count dimension explicitly
/// for transfer to Heads-Up and track unseen-state fallback.
pub(crate) struct FrozenPolicy {
    pooled: [[[u64; 3]; 2]; 4*5*2*4],
    original: [[[[u64;3];2];3];4*5*2*4],
}
impl FrozenPolicy {
    pub(crate) fn load(path: &str) -> io::Result<Self> {
        let mut file=BufReader::new(File::open(path)?);
        let mut magic=[0u8;8];
        file.read_exact(&mut magic)?;
        if &magic!=b"SPKIMT01" { return Err(invalid("not SPKIMT01 model")); }
        let mut count=[0u8;4];
        file.read_exact(&mut count)?;
        if u32::from_le_bytes(count) as usize!=BUCKETS {return Err(invalid("wrong bucket count"));}
        let mut pooled=[[[0u64;3];2];4*5*2*4];
        let mut original=[[[[0u64;3];2];3];4*5*2*4];
        for street_rank_suit_price in 0..4*5*2*4 {
            for multi in 0..3 {
                for pos in 0..2 {
                    for action in 0..3 {
                        let mut bytes=[0u8;8];
                        file.read_exact(&mut bytes)?;
                        let value=u64::from_le_bytes(bytes);
                        original[street_rank_suit_price][multi][pos][action]=value;
                        pooled[street_rank_suit_price][pos][action]
                            =pooled[street_rank_suit_price][pos][action].saturating_add(value);
                    }
                }
            }
        }
        let mut trailing=[0u8;1];
        if file.read(&mut trailing)?!=0 {return Err(invalid("trailing model bytes"));}
        Ok(Self{pooled,original})
    }
    pub(crate) fn predict_multi(
        &self, street: usize, hole:[u8;2], pot:i64, call:i64,
        on_button:bool, active:usize, legal:[bool;3]
    )->([f32;3],bool) {
        let a=hole[0];let b=hole[1];
        let ranks=if a==255 || b==255 {4} else if a%13==b%13 {0} else {
            let high=(a%13).max(b%13);
            if high>=11{1}else if high>=8{2}else{3}
        };
        let suited=usize::from(a!=255 && b!=255 && a/13==b/13);
        let odds=call as f64/(pot+call).max(1) as f64;
        let price=if call==0{0}else if odds<0.2{1}else if odds<0.4{2}else{3};
        let group=(((street.min(3)*5+ranks)*2+suited)*4)+price;
        let multi=if active<=2{0}else if active<=4{1}else{2};
        let counts=&self.original[group][multi][usize::from(on_button)];
        let seen=counts.iter().sum::<u64>()>0;
        let mut probs=[0f32;3];let mut total=0f32;
        for i in 0..3{if legal[i]{probs[i]=(counts[i] as f32)+1.0;total+=probs[i];}}
        for p in &mut probs{*p/=total;}
        (probs,seen)
    }
    pub(crate) fn predict(
        &self, street: usize, hole:[u8;2], pot:i64, call:i64,
        on_button: bool, legal:[bool;3],
    ) -> ([f32;3],bool) {
        let a=hole[0];let b=hole[1];
        let ranks=if a==255 || b==255 {4} else if a%13==b%13{0} else {
            let high=(a%13).max(b%13);
            if high>=11 {1} else if high>=8 {2} else {3}
        };
        let suited=usize::from(a!=255 && b!=255 && a/13==b/13);
        let price=if call<=0 {0} else {
            let ratio=call as f64/(pot.max(0)+call).max(1) as f64;
            if ratio<0.2{1}else if ratio<0.4{2}else{3}
        };
        let group=(((street.min(3)*5+ranks)*2+suited)*4)+price;
        let counts=&self.pooled[group][usize::from(on_button)];
        let observations=counts.iter().sum::<u64>();
        let mut probs=[0f32;3];let mut total=0.0;
        for action in 0..3 {
            if legal[action] {
                // Laplace smoothing is the same as the PHH trainer.
                probs[action]=(counts[action] as f64+SMOOTH) as f32;
                total+=probs[action] as f64;
            }
        }
        for p in &mut probs {*p=(*p as f64/total) as f32;}
        (probs,observations>0)
    }
}

fn scan<F:FnMut(&[u8;ROW])->io::Result<()>>(path:&str,mut consume:F)->io::Result<()>{
    let mut file=BufReader::new(File::open(path)?);
    let mut magic=[0u8;8];file.read_exact(&mut magic)?;
    if &magic!=MAGIC{return Err(invalid("not SPKOBS02 file"));}
    loop{
        let mut row=[0u8;ROW];
        if file.read(&mut row[0..1])?==0{break;}
        file.read_exact(&mut row[1..])?;
        if row[12]>2 || row[11]>3 || row[13]<2{return Err(invalid("invalid action/street/player count"));}
        if row[15]&(1<<row[12])==0{return Err(invalid("recorded action is illegal"));}
        consume(&row)?;
    }
    Ok(())
}
pub fn run(args:&[String])->io::Result<()>{
    let source=args.windows(2).find(|w|w[0]=="--source").map(|w|w[1].as_str()).ok_or_else(||invalid("missing --source"))?;
    let out=args.windows(2).find(|w|w[0]=="--output").map(|w|w[1].as_str()).unwrap_or("phh-imitation.bin");
    let mut model=Counts::new();let mut train=[0u64;3];let mut test=[0u64;3];
    scan(source,|r|{
        let id=u64::from_le_bytes(r[0..8].try_into().unwrap());
        if id%5!=0{
            model.0[key(r)][r[12] as usize]+=1;
            train[r[12] as usize]+=1;
        }else{test[r[12] as usize]+=1;}
        Ok(())
    })?;
    let mut loss=0.0;let mut null_loss=0.0;let mut correct=0u64;let mut evaluated=0u64;
    scan(source,|r|{
        let id=u64::from_le_bytes(r[0..8].try_into().unwrap());
        if id%5!=0{return Ok(());}
        let action=r[12] as usize;
        let probs=model.probability(key(r),r[15]);
        let legal=(0..ACTIONS).filter(|&a|r[15]&(1<<a)!=0).count();
        loss-=probs[action].max(1e-12).ln();
        null_loss+=(legal as f64).ln();
        if probs[action]==probs.iter().copied().fold(0.0,f64::max){correct+=1;}
        evaluated+=1;
        Ok(())
    })?;
    let mut file=File::create(out)?;
    file.write_all(b"SPKIMT01")?;
    file.write_all(&(BUCKETS as u32).to_le_bytes())?;
    for b in &model.0 {for x in b{file.write_all(&x.to_le_bytes())?;}}
    println!("IMITATION source={} decisions={} train={} test={} train_actions={:?} test_actions={:?} heldout_log_loss={:.5} uniform_legal_log_loss={:.5} heldout_accuracy={:.4} output={}",
        source,train.iter().sum::<u64>()+test.iter().sum::<u64>(),train.iter().sum::<u64>(),test.iter().sum::<u64>(),
        train,test,if evaluated>0{loss/evaluated as f64}else{f64::NAN},
        if evaluated>0{null_loss/evaluated as f64}else{f64::NAN},
        if evaluated>0{correct as f64/evaluated as f64}else{f64::NAN},out);
    println!("IMITATION NOTE: this is behavior cloning from historical actions, not a GTO solver or verified improvement in EV");
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
    #[test] fn transfer_respects_legal_actions(){
        let model=FrozenPolicy{pooled:[[[0u64;3];2];4*5*2*4],original:[[[[0u64;3];2];3];4*5*2*4]};
        let (p,seen)=model.predict(0,[51,50],3,1,true,[false,true,true]);
        assert!(!seen);
        assert_eq!(p,[0.0,0.5,0.5]);
    }
    #[test] fn no_opponent_cards_in_feature_key(){
        let mut x=[0u8;96];
        x[11]=2;x[16]=51;x[17]=50;x[72..80].copy_from_slice(&6i64.to_le_bytes());
        let a=key(&x);
        x[23]=17;x[88]=23;
        assert_eq!(key(&x),a);
    }
    #[test] fn normalize_policy(){
        let counts=Counts::new();let p=counts.probability(0,0b110);
        assert_eq!(p,[0.0,0.5,0.5]);
    }
    #[test] fn seen_state_does_not_overflow_action_counts(){
        let mut model=FrozenPolicy{pooled:[[[0u64;3];2];4*5*2*4],original:[[[[0u64;3];2];3];4*5*2*4]};
        // Unknown hole cards, preflop, no bet: group 32, off button.
        model.pooled[32][0]=[u64::MAX,1,0];
        model.original[32][0][0]=[u64::MAX,1,0];
        let (hu,hu_seen)=model.predict(0,[255,255],3,0,false,[true;3]);
        let (multi,multi_seen)=model.predict_multi(0,[255,255],3,0,false,2,[true;3]);
        assert!(hu_seen && multi_seen);
        assert!(hu.iter().chain(&multi).all(|p|p.is_finite()));
        assert!((hu.iter().sum::<f32>()-1.0).abs()<1e-6);
        assert!((multi.iter().sum::<f32>()-1.0).abs()<1e-6);
    }
    #[test] fn overflowing_pooled_checkpoint_counts_are_rejected(){
        let path=std::env::temp_dir().join(format!("solverpk-imitation-overflow-{}.bin",std::process::id()));
        let mut data=Vec::from(*b"SPKIMT01");
        data.extend_from_slice(&(BUCKETS as u32).to_le_bytes());
        for bucket in 0..BUCKETS{
            for action in 0..3{
                let count:u64=if action==0 && bucket==0{u64::MAX}else if action==0 && bucket==2{1}else{0};
                data.extend_from_slice(&count.to_le_bytes());
            }
        }
        std::fs::write(&path,data).unwrap();
        let result=FrozenPolicy::load(path.to_str().unwrap());
        std::fs::remove_file(path).unwrap();
        assert!(matches!(result,Err(e) if e.kind()==io::ErrorKind::InvalidData));
    }
}
