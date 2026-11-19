//! Small trainable neural actor for six-handed Hold'em, Rust stdlib only.
//! 64 -> 32 (ReLU) -> 6 (masked softmax), REINFORCE with baseline.
//! This is an actual neural network, not tabular action counts.
use crate::Rng;
use std::fs::File;
use std::io::{self,Read,Write};
pub const INPUT:usize=64;
pub const HIDDEN:usize=32;
pub const ACTIONS:usize=6;
const MAGIC:&[u8;8]=b"SPKNN001";
#[derive(Clone)]
pub struct Net {
    w1:Box<[[f32;INPUT];HIDDEN]>,
    b1:[f32;HIDDEN],
    w2:[[f32;HIDDEN];ACTIONS],
    b2:[f32;ACTIONS],
    baseline:f32,
    pub steps:u64,
}
pub struct Forward {
    hidden:[f32;HIDDEN],
    pub prob:[f32;ACTIONS],
}
#[derive(Clone,Copy)]
pub struct PolicySample<'a> {
    pub features:&'a [f32;INPUT],
    pub legal:[bool;ACTIONS],
    pub action:usize,
    pub reward:f32,
}
impl Net {
    pub fn new(seed:u64)->Self {
        let mut rng=Rng::new(seed);
        let mut w1=Box::new([[0f32;INPUT];HIDDEN]);
        let mut w2=[[0f32;HIDDEN];ACTIONS];
        for j in 0..HIDDEN {for i in 0..INPUT {
            w1[j][i]=((rng.next() as f64/u64::MAX as f64)*2.0-1.0) as f32*0.13;
        }}
        for a in 0..ACTIONS{for j in 0..HIDDEN{
            w2[a][j]=((rng.next() as f64/u64::MAX as f64)*2.0-1.0) as f32*0.16;
        }}
        Self{w1,b1:[0.0;HIDDEN],w2,b2:[0.0;ACTIONS],baseline:0.0,steps:0}
    }
    pub fn forward(&self,x:&[f32;INPUT],legal:[bool;ACTIONS])->Forward {
        let mut hidden=[0f32;HIDDEN];
        for (j,h) in hidden.iter_mut().enumerate(){
            let mut sum=self.b1[j];
            for (i,&v) in x.iter().enumerate(){sum+=self.w1[j][i]*v;}
            *h=sum.max(0.0);
        }
        let mut logits=[f32::NEG_INFINITY;ACTIONS];
        for a in 0..ACTIONS {
            if legal[a]{
                let mut score=self.b2[a];
                for (j,&h) in hidden.iter().enumerate(){score+=self.w2[a][j]*h;}
                logits[a]=score;
            }
        }
        let max=logits.iter().copied().fold(f32::NEG_INFINITY,f32::max);
        let mut prob=[0f32;ACTIONS];let mut total=0.0;
        for a in 0..ACTIONS{if legal[a]{prob[a]=(logits[a]-max).exp();total+=prob[a];}}
        if total>0.0 {for p in &mut prob{*p/=total;}}
        Forward{hidden,prob}
    }
    // Transitional adapter for the regression run. It intentionally retains
    // the old sequential optimizer until the failing batch tests are observed.
    pub fn reinforce_hand(&mut self,samples:&[PolicySample<'_>],_player_rewards:&[f32],lr:f32)->io::Result<()> {
        for sample in samples {
            self.reinforce(sample.features,sample.legal,sample.action,sample.reward,lr);
        }
        Ok(())
    }
    /// Online policy gradient; snapshot pre-update params for exact backprop.
    pub fn reinforce(&mut self,x:&[f32;INPUT],legal:[bool;ACTIONS],action:usize,reward:f32,lr:f32){
        if lr<=0.0{return;}
        let forward=self.forward(x,legal);
        let advantage=(reward-self.baseline).clamp(-3.0,3.0);
        self.baseline+=0.0005*(reward-self.baseline);
        let mut delta=[0f32;ACTIONS];
        for a in 0..ACTIONS{
            if legal[a]{delta[a]=(f32::from(a==action)-forward.prob[a])*advantage;}
        }
        let mut hidden_delta=[0f32;HIDDEN];
        for j in 0..HIDDEN{
            let mut d=0.0;
            for a in 0..ACTIONS{d+=self.w2[a][j]*delta[a];}
            hidden_delta[j]=if forward.hidden[j]>0.0{d}else{0.0};
        }
        for a in 0..ACTIONS{
            for j in 0..HIDDEN{self.w2[a][j]=(self.w2[a][j]+lr*delta[a]*forward.hidden[j]).clamp(-8.0,8.0);}
            self.b2[a]=(self.b2[a]+lr*delta[a]).clamp(-8.0,8.0);
        }
        for j in 0..HIDDEN{
            for (i,&v) in x.iter().enumerate(){
                self.w1[j][i]=(self.w1[j][i]+lr*hidden_delta[j]*v).clamp(-8.0,8.0);
            }
            self.b1[j]=(self.b1[j]+lr*hidden_delta[j]).clamp(-8.0,8.0);
        }
        self.steps=self.steps.saturating_add(1);
    }
    /// Cross-entropy policy distillation from an established tabular teacher.
    /// No opponent private cards or post-decision observations are provided.
    pub fn imitate(&mut self,x:&[f32;INPUT],legal:[bool;ACTIONS],target:[f32;ACTIONS],lr:f32){
        if lr<=0.0{return;}
        let fw=self.forward(x,legal);
        let mut grad=[0f32;ACTIONS];
        for a in 0..ACTIONS{
            if legal[a]{grad[a]=target[a]-fw.prob[a];}
        }
        let mut hd=[0f32;HIDDEN];
        for j in 0..HIDDEN{
            let mut d=0.0f32;
            for a in 0..ACTIONS{d+=self.w2[a][j]*grad[a];}
            if fw.hidden[j]>0.0{hd[j]=d;}
        }
        for a in 0..ACTIONS{
            for j in 0..HIDDEN{self.w2[a][j]=(self.w2[a][j]+lr*grad[a]*fw.hidden[j]).clamp(-8.0,8.0);}
            self.b2[a]=(self.b2[a]+lr*grad[a]).clamp(-8.0,8.0);
        }
        for j in 0..HIDDEN{
            for i in 0..INPUT{
                self.w1[j][i]=(self.w1[j][i]+lr*hd[j]*x[i]).clamp(-8.0,8.0);
            }
            self.b1[j]=(self.b1[j]+lr*hd[j]).clamp(-8.0,8.0);
        }
        self.steps=self.steps.saturating_add(1);
    }
    pub fn save(&self,path:&str)->io::Result<()>{
        let mut file=File::create(path)?;
        file.write_all(MAGIC)?;
        file.write_all(&self.steps.to_le_bytes())?;
        file.write_all(&self.baseline.to_le_bytes())?;
        for row in self.w1.iter(){for v in row{file.write_all(&v.to_le_bytes())?;}}
        for v in self.b1{file.write_all(&v.to_le_bytes())?;}
        for row in self.w2 {for v in row{file.write_all(&v.to_le_bytes())?;}}
        for v in self.b2 {file.write_all(&v.to_le_bytes())?;}
        Ok(())
    }
    pub fn load(path:&str)->io::Result<Self>{
        let mut file=File::open(path)?;
        let mut magic=[0u8;8];file.read_exact(&mut magic)?;
        if &magic!=MAGIC{return Err(io::Error::new(io::ErrorKind::InvalidData,"invalid neural checkpoint"));}
        fn u64read<R:Read>(r:&mut R)->io::Result<u64>{let mut b=[0;8];r.read_exact(&mut b)?;Ok(u64::from_le_bytes(b))}
        fn f<R:Read>(r:&mut R)->io::Result<f32>{
            let mut b=[0;4];r.read_exact(&mut b)?;
            let x=f32::from_le_bytes(b);
            if !x.is_finite(){return Err(io::Error::new(io::ErrorKind::InvalidData,"nonfinite neural weight"));}
            Ok(x)
        }
        fn weight<R:Read>(r:&mut R)->io::Result<f32>{
            let x=f(r)?;
            // Initialization and every optimizer update keep parameters in
            // this range; finite outliers can still overflow the forward pass.
            if !(-8.0..=8.0).contains(&x){
                return Err(io::Error::new(io::ErrorKind::InvalidData,"neural weight outside optimizer bounds"));
            }
            Ok(x)
        }
        let mut net=Self::new(13);
        net.steps=u64read(&mut file)?;net.baseline=f(&mut file)?;
        for row in net.w1.iter_mut(){for v in row{*v=weight(&mut file)?;}}
        for v in &mut net.b1{*v=weight(&mut file)?;}
        for row in &mut net.w2{for v in row{*v=weight(&mut file)?;}}
        for v in &mut net.b2{*v=weight(&mut file)?;}
        let mut extra=[0u8;1];
        if file.read(&mut extra)?!=0{return Err(io::Error::new(io::ErrorKind::InvalidData,"trailing checkpoint bytes"));}
        Ok(net)
    }
}
#[cfg(test)]
#[path = "neural_v2_tests.rs"]
mod v2_tests;
#[cfg(test)]
mod tests{
    use super::*;
    #[test]fn masked_softmax_and_gradient(){
        let mut net=Net::new(19);
        let mut x=[0f32;INPUT];x[0]=1.0;x[10]=0.5;
        let legal=[false,true,true,true,false,false];
        let before=net.forward(&x,legal).prob;
        assert_eq!(before[0],0.0);
        assert!((before.iter().sum::<f32>()-1.0).abs()<1e-5);
        for _ in 0..200{net.reinforce(&x,legal,2,1.0,0.02);}
        let after=net.forward(&x,legal).prob;
        assert!(after[2]>before[2],"chosen action probability did not learn");
    }
    #[test]fn distillation_decreases_cross_entropy(){
        let mut net=Net::new(37);
        let x=[0.2;INPUT];let legal=[true;ACTIONS];
        let target=[0.0,1.0,0.0,0.0,0.0,0.0];
        let before=net.forward(&x,legal).prob[1];
        for _ in 0..200 {net.imitate(&x,legal,target,0.01);}
        assert!(net.forward(&x,legal).prob[1]>before);
    }
    #[test]fn save_load_reproduces_outputs(){
        let net=Net::new(23);
        let path=std::env::temp_dir().join(format!("solverpk-neural-{}.bin",std::process::id()));
        net.save(path.to_str().unwrap()).unwrap();
        let restored=Net::load(path.to_str().unwrap()).unwrap();
        std::fs::remove_file(path).unwrap();
        let x=[0.25;INPUT];let mask=[true;ACTIONS];
        assert_eq!(net.forward(&x,mask).prob,restored.forward(&x,mask).prob);
    }
    #[test]fn zero_rate_imitation_keeps_model_frozen(){
        let mut net=Net::new(41);
        let before=net.clone();
        net.imitate(&[0.2;INPUT],[true;ACTIONS],[0.0,1.0,0.0,0.0,0.0,0.0],0.0);
        assert_eq!(net.steps,before.steps);
        assert_eq!(net.w1,before.w1);
        assert_eq!(net.b1,before.b1);
        assert_eq!(net.w2,before.w2);
        assert_eq!(net.b2,before.b2);
        assert_eq!(net.baseline,before.baseline);
    }
    #[test]fn reinforcement_counter_does_not_wrap(){
        let mut net=Net::new(43);
        net.steps=u64::MAX;
        net.reinforce(&[0.2;INPUT],[true;ACTIONS],1,1.0,0.01);
        assert_eq!(net.steps,u64::MAX);
        assert!(net.forward(&[0.2;INPUT],[true;ACTIONS]).prob.iter().all(|p|p.is_finite()));
    }
    #[test]fn imitation_counter_does_not_wrap(){
        let mut net=Net::new(47);
        net.steps=u64::MAX;
        net.imitate(&[0.2;INPUT],[true;ACTIONS],[0.0,1.0,0.0,0.0,0.0,0.0],0.01);
        assert_eq!(net.steps,u64::MAX);
    }
    #[test]fn checkpoint_rejects_finite_weights_that_overflow_inference(){
        let mut net=Net::new(53);
        net.w1[0][0]=f32::MAX;
        let path=std::env::temp_dir().join(format!("solverpk-neural-overflow-{}.bin",std::process::id()));
        net.save(path.to_str().unwrap()).unwrap();
        let result=Net::load(path.to_str().unwrap());
        std::fs::remove_file(path).unwrap();
        assert!(matches!(result,Err(e) if e.kind()==io::ErrorKind::InvalidData));
    }
}
