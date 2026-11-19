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
fn input(path:&str)->io::Result<Vec<[u8;ROW]>>{
    let mut file=BufReader::new(File::open(path)?);
    let mut magic=[0u8;8];file.read_exact(&mut magic)?;
    if &magic!=MAGIC{return Err(invalid("not SPKOBS02 file"));}
    let mut records=Vec::new();
    loop{
        let mut row=[0u8;ROW];
        let mut start=[0u8;1];
        if file.read(&mut start)?==0{break;}
        row[0]=start[0];
        file.read_exact(&mut row[1..])?;
        if row[12]>2 || row[11]>3 || row[13]<2{return Err(invalid("invalid action/street/player count"));}
        if row[15]&(1<<row[12])==0{return Err(invalid("recorded action is illegal"));}
        records.push(row);
    }
    Ok(records)
}
pub fn run(args:&[String])->io::Result<()>{
    let source=args.windows(2).find(|w|w[0]=="--source").map(|w|w[1].as_str()).ok_or_else(||invalid("missing --source"))?;
    let out=args.windows(2).find(|w|w[0]=="--output").map(|w|w[1].as_str()).unwrap_or("phh-imitation.bin");
    let records=input(source)?;
    let mut model=Counts::new();let mut train=[0u64;3];let mut test=[0u64;3];
    // Group split by hand ID: no decisions from a hand cross folds.
    for r in &records{
        let id=u64::from_le_bytes(r[0..8].try_into().unwrap());
        if id%5!=0{
            model.0[key(r)][r[12] as usize]+=1;
            train[r[12] as usize]+=1;
        }else{test[r[12] as usize]+=1;}
    }
    let mut loss=0.0;let mut null_loss=0.0;let mut correct=0u64;let mut evaluated=0u64;
    for r in &records{
        let id=u64::from_le_bytes(r[0..8].try_into().unwrap());
        if id%5!=0{continue;}
        let action=r[12] as usize;
        let probs=model.probability(key(r),r[15]);
        let legal=(0..ACTIONS).filter(|&a|r[15]&(1<<a)!=0).count();
        loss-=probs[action].max(1e-12).ln();
        null_loss+=(legal as f64).ln();
        if probs[action]==probs.iter().copied().fold(0.0,f64::max){correct+=1;}
        evaluated+=1;
    }
    let mut file=File::create(out)?;
    file.write_all(b"SPKIMT01")?;
    file.write_all(&(BUCKETS as u32).to_le_bytes())?;
    for b in &model.0 {for x in b{file.write_all(&x.to_le_bytes())?;}}
    println!("IMITATION source={} decisions={} train={} test={} train_actions={:?} test_actions={:?} heldout_log_loss={:.5} uniform_legal_log_loss={:.5} heldout_accuracy={:.4} output={}",
        source,records.len(),train.iter().sum::<u64>(),test.iter().sum::<u64>(),
        train,test,if evaluated>0{loss/evaluated as f64}else{f64::NAN},
        if evaluated>0{null_loss/evaluated as f64}else{f64::NAN},
        if evaluated>0{correct as f64/evaluated as f64}else{f64::NAN},out);
    println!("IMITATION NOTE: this is behavior cloning from historical actions, not a GTO solver or verified improvement in EV");
    Ok(())
}
#[cfg(test)]
mod tests{
    use super::*;
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
}
