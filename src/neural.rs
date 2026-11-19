//! Suit-aware 128 -> 32 ReLU -> 6 masked-softmax actor, Rust stdlib only.
//! SPKNN001 retains its original 64-input inference and is evaluation-only.
//! SPKNN002 uses one frozen-policy update per hand and linear monetary returns.
use crate::Rng;
use std::fs::{self,File,OpenOptions};
use std::io::{self,Cursor,Read,Write};
use std::path::Path;
use std::sync::atomic::{AtomicU64,Ordering};
pub const LEGACY_INPUT:usize=64;
pub const INPUT:usize=128;
pub const HIDDEN:usize=32;
pub const ACTIONS:usize=6;
const MAGIC_V1:&[u8;8]=b"SPKNN001";
const MAGIC_V2:&[u8;8]=b"SPKNN002";
const ENCODER_VERSION:u32=2;
const OPTIMIZER_VERSION:u32=2;
const MAX_TRAINING_SEGMENTS:usize=4096;
const V2_HEADER_BYTES:usize=53;
const SEGMENT_BYTES:usize=40;
const PARAMETER_COUNT:usize=HIDDEN*INPUT+HIDDEN+ACTIONS*HIDDEN+ACTIONS;
const MAX_CHECKPOINT_BYTES:usize=V2_HEADER_BYTES+MAX_TRAINING_SEGMENTS*SEGMENT_BYTES+PARAMETER_COUNT*4+8;
static TEMP_SEQUENCE:AtomicU64=AtomicU64::new(0);

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum ObservationVersion {LegacyV1,SuitAwareV2}

#[derive(Clone,Copy,Debug,PartialEq,Eq)]
pub enum TrainingKind {SelfPlay,Distillation}

#[derive(Clone,Copy,Debug,PartialEq)]
pub struct TrainingSegment {
    pub kind:TrainingKind,
    pub seed:u64,
    pub start_hand:u64,
    pub hands:u64,
    pub learning_rate:f32,
    pub rake_bps:u16,
    pub rake_cap_units:i64,
    pub no_flop_no_drop:bool,
}

#[derive(Clone)]
pub struct Net {
    w1:Box<[[f32;INPUT];HIDDEN]>,
    b1:[f32;HIDDEN],
    w2:[[f32;HIDDEN];ACTIONS],
    b2:[f32;ACTIONS],
    baseline:f32,
    pub steps:u64,
    pub updates:u64,
    observation_version:ObservationVersion,
    training_segments:Vec<TrainingSegment>,
    provenance_complete:bool,
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
        Self{w1,b1:[0.0;HIDDEN],w2,b2:[0.0;ACTIONS],baseline:0.0,steps:0,updates:0,
            observation_version:ObservationVersion::SuitAwareV2,training_segments:Vec::new(),provenance_complete:true}
    }
    pub fn observation_version(&self)->ObservationVersion {self.observation_version}
    pub fn training_segments(&self)->&[TrainingSegment] {&self.training_segments}
    pub fn provenance_complete(&self)->bool {self.provenance_complete}
    pub fn mark_provenance_incomplete(&mut self) {self.provenance_complete=false;}
    fn require_trainable(&self)->io::Result<()> {
        if self.observation_version==ObservationVersion::LegacyV1 {
            return Err(invalid_input("legacy SPKNN001 checkpoint is evaluation-only; initialize a new SPKNN002 checkpoint for training"));
        }
        Ok(())
    }
    fn validate_rate(&self,lr:f32)->io::Result<()> {
        self.require_trainable()?;
        if !lr.is_finite() || !(0.0..=0.05).contains(&lr) {
            return Err(invalid_input("invalid neural learning rate"));
        }
        Ok(())
    }
    pub fn next_training_hand(&self,seed:u64)->io::Result<u64> {
        let mut next=0;
        for segment in &self.training_segments {
            let end=validate_segment(segment)?;
            if segment.seed==seed {next=next.max(end);}
        }
        Ok(next)
    }
    pub fn record_training(&mut self,segment:TrainingSegment)->io::Result<()> {
        self.require_trainable()?;
        validate_new_segment(&self.training_segments,&segment)?;
        if let Some(previous)=self.training_segments.last_mut() {
            let end=validate_segment(previous)?;
            if same_training_config(previous,&segment) && end==segment.start_hand {
                previous.hands=previous.hands.checked_add(segment.hands)
                    .ok_or_else(||invalid_input("training hand count overflow"))?;
                return Ok(());
            }
        }
        if self.training_segments.len()>=MAX_TRAINING_SEGMENTS {
            return Err(invalid_input("too many neural training segments"));
        }
        self.training_segments.push(segment);
        Ok(())
    }
    pub fn forward(&self,x:&[f32;INPUT],legal:[bool;ACTIONS])->Forward {
        let width=match self.observation_version {
            ObservationVersion::LegacyV1=>LEGACY_INPUT,
            ObservationVersion::SuitAwareV2=>INPUT,
        };
        let mut hidden=[0f32;HIDDEN];
        for (j,h) in hidden.iter_mut().enumerate(){
            let mut sum=self.b1[j];
            // Do not add padded zero terms to legacy inference: retain the
            // historical multiplication/addition sequence exactly.
            for (i,&v) in x[..width].iter().enumerate(){sum+=self.w1[j][i]*v;}
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
    /// Simultaneous independent-player policy gradient at a frozen hand policy.
    /// Each score uses its own player's linear net monetary return. Opponents'
    /// policies are held fixed when taking that player's gradient; this is not
    /// the derivative of the sum of all utilities (zero in a rake-free game).
    /// Normalize by the fixed number of players, never by decision count.
    pub fn reinforce_hand(&mut self,samples:&[PolicySample<'_>],player_rewards:&[f32],lr:f32)->io::Result<()> {
        self.validate_rate(lr)?;
        if lr==0.0 {return Ok(());}
        if player_rewards.is_empty() || player_rewards.iter().any(|r|!r.is_finite()) {
            return Err(invalid_input("a neural hand requires finite player returns"));
        }
        if samples.is_empty() {return Ok(());}
        let mut gradient=Gradient::new();
        // No weights or baseline change until ALL sample gradients are formed.
        for sample in samples {
            validate_observation(sample.features,sample.legal)?;
            if sample.action>=ACTIONS || !sample.legal[sample.action] || !sample.reward.is_finite() {
                return Err(invalid_input("invalid neural policy sample"));
            }
            let forward=self.checked_forward(sample.features,sample.legal)?;
            let advantage=sample.reward as f64-self.baseline as f64;
            let mut delta=[0.0;ACTIONS];
            for a in 0..ACTIONS {
                if sample.legal[a] {
                    delta[a]=(f64::from(a==sample.action)-forward.prob[a] as f64)*advantage;
                }
            }
            gradient.accumulate(self,sample.features,&forward,delta);
        }
        let count=player_rewards.len() as f64;
        let mean=player_rewards.iter().map(|&reward|reward as f64).sum::<f64>()/count;
        let baseline=(self.baseline as f64+0.0005*(mean-self.baseline as f64)) as f32;
        if !baseline.is_finite() {return Err(invalid_input("nonfinite neural baseline update"));}
        self.apply_gradient(&gradient,lr as f64/count)?;
        self.baseline=baseline;
        self.steps=self.steps.saturating_add(samples.len() as u64);
        self.updates=self.updates.saturating_add(1);
        Ok(())
    }
    /// Convenience for a single-player, single-decision update.
    #[cfg(test)]
    fn reinforce(&mut self,x:&[f32;INPUT],legal:[bool;ACTIONS],action:usize,reward:f32,lr:f32)->io::Result<()> {
        self.reinforce_hand(&[PolicySample{features:x,legal,action,reward}],&[reward],lr)
    }
    /// Cross-entropy policy distillation from an established tabular teacher.
    /// No opponent private cards or post-decision observations are provided.
    pub fn imitate(&mut self,x:&[f32;INPUT],legal:[bool;ACTIONS],target:[f32;ACTIONS],lr:f32)->io::Result<()> {
        self.validate_rate(lr)?;
        if lr==0.0 {return Ok(());}
        validate_observation(x,legal)?;
        for a in 0..ACTIONS {
            if !target[a].is_finite() || target[a]<0.0 || (!legal[a] && target[a]!=0.0) {
                return Err(invalid_input("invalid neural distillation target"));
            }
        }
        let total=target.iter().map(|&p|p as f64).sum::<f64>();
        if (total-1.0).abs()>1e-4 {return Err(invalid_input("neural distillation target must sum to one"));}
        let forward=self.checked_forward(x,legal)?;
        let mut delta=[0.0;ACTIONS];
        for a in 0..ACTIONS {
            if legal[a] {delta[a]=target[a] as f64/total-forward.prob[a] as f64;}
        }
        let mut gradient=Gradient::new();
        gradient.accumulate(self,x,&forward,delta);
        self.apply_gradient(&gradient,lr as f64)?;
        self.steps=self.steps.saturating_add(1);
        self.updates=self.updates.saturating_add(1);
        Ok(())
    }
    fn checked_forward(&self,x:&[f32;INPUT],legal:[bool;ACTIONS])->io::Result<Forward> {
        let forward=self.forward(x,legal);
        if forward.hidden.iter().chain(forward.prob.iter()).any(|v|!v.is_finite())
            || forward.prob.iter().sum::<f32>()<=0.0 {
            return Err(invalid_input("nonfinite neural forward pass"));
        }
        Ok(forward)
    }
    fn apply_gradient(&mut self,gradient:&Gradient,scale:f64)->io::Result<()> {
        if !scale.is_finite() || gradient.w1.iter().flatten().chain(gradient.b1.iter())
            .chain(gradient.w2.iter().flatten()).chain(gradient.b2.iter()).any(|v|!v.is_finite()) {
            return Err(invalid_input("nonfinite neural gradient"));
        }
        // Parameter projection is the only clipping. Neither monetary returns
        // nor advantages nor the aggregate gradient norm are clipped.
        for j in 0..HIDDEN {
            for i in 0..INPUT {self.w1[j][i]=(self.w1[j][i] as f64+scale*gradient.w1[j][i]).clamp(-8.0,8.0) as f32;}
            self.b1[j]=(self.b1[j] as f64+scale*gradient.b1[j]).clamp(-8.0,8.0) as f32;
        }
        for a in 0..ACTIONS {
            for j in 0..HIDDEN {self.w2[a][j]=(self.w2[a][j] as f64+scale*gradient.w2[a][j]).clamp(-8.0,8.0) as f32;}
            self.b2[a]=(self.b2[a] as f64+scale*gradient.b2[a]).clamp(-8.0,8.0) as f32;
        }
        Ok(())
    }
    pub fn save(&self,path:&str)->io::Result<()>{
        self.save_with_mode(path,false)
    }
    /// Atomically publish a fresh checkpoint without replacing an existing file.
    pub fn save_new(&self,path:&str)->io::Result<()>{
        self.save_with_mode(path,true)
    }
    fn save_with_mode(&self,path:&str,create_new:bool)->io::Result<()>{
        let mut bytes=Vec::new();
        let width=match self.observation_version {
            ObservationVersion::LegacyV1=>{
                bytes.extend_from_slice(MAGIC_V1);
                bytes.extend_from_slice(&self.steps.to_le_bytes());
                bytes.extend_from_slice(&self.baseline.to_le_bytes());
                LEGACY_INPUT
            }
            ObservationVersion::SuitAwareV2=>{
                bytes.extend_from_slice(MAGIC_V2);
                for value in [INPUT as u32,HIDDEN as u32,ACTIONS as u32,ENCODER_VERSION,OPTIMIZER_VERSION] {
                    bytes.extend_from_slice(&value.to_le_bytes());
                }
                bytes.extend_from_slice(&self.steps.to_le_bytes());
                bytes.extend_from_slice(&self.updates.to_le_bytes());
                bytes.extend_from_slice(&self.baseline.to_le_bytes());
                bytes.push(u8::from(self.provenance_complete));
                let count=u32::try_from(self.training_segments.len())
                    .map_err(|_|invalid_input("too many neural training segments"))?;
                bytes.extend_from_slice(&count.to_le_bytes());
                for segment in &self.training_segments {
                    bytes.push(match segment.kind {TrainingKind::SelfPlay=>1,TrainingKind::Distillation=>2});
                    bytes.extend_from_slice(&segment.seed.to_le_bytes());
                    bytes.extend_from_slice(&segment.start_hand.to_le_bytes());
                    bytes.extend_from_slice(&segment.hands.to_le_bytes());
                    bytes.extend_from_slice(&segment.learning_rate.to_le_bytes());
                    bytes.extend_from_slice(&segment.rake_bps.to_le_bytes());
                    bytes.extend_from_slice(&segment.rake_cap_units.to_le_bytes());
                    bytes.push(u8::from(segment.no_flop_no_drop));
                }
                INPUT
            }
        };
        for row in self.w1.iter(){for v in &row[..width]{bytes.extend_from_slice(&v.to_le_bytes());}}
        for v in self.b1{bytes.extend_from_slice(&v.to_le_bytes());}
        for row in self.w2 {for v in row{bytes.extend_from_slice(&v.to_le_bytes());}}
        for v in self.b2 {bytes.extend_from_slice(&v.to_le_bytes());}
        if self.observation_version==ObservationVersion::SuitAwareV2 {
            let hash=checksum(&bytes);
            bytes.extend_from_slice(&hash.to_le_bytes());
        }
        save_atomic(Path::new(path),&bytes,create_new)
    }
    pub fn load(path:&str)->io::Result<Self>{
        let mut bytes=Vec::new();
        File::open(path)?.take((MAX_CHECKPOINT_BYTES+1) as u64).read_to_end(&mut bytes)?;
        if bytes.len()>MAX_CHECKPOINT_BYTES {return Err(invalid_data("neural checkpoint exceeds size limit"));}
        let magic=bytes.get(..8).ok_or_else(||invalid_data("truncated neural checkpoint"))?;
        let (version,payload)=if magic==MAGIC_V1 {
            (ObservationVersion::LegacyV1,bytes.as_slice())
        }else if magic==MAGIC_V2 {
            if bytes.len()<V2_HEADER_BYTES+8 {return Err(invalid_data("truncated neural checkpoint"));}
            let end=bytes.len()-8;
            let expected=u64::from_le_bytes(bytes[end..].try_into().unwrap());
            if checksum(&bytes[..end])!=expected {return Err(invalid_data("neural checkpoint checksum mismatch"));}
            (ObservationVersion::SuitAwareV2,&bytes[..end])
        }else{return Err(invalid_data("invalid neural checkpoint version"));};
        let mut reader=Cursor::new(payload);
        reader.set_position(8);
        let mut net=Self{
            w1:Box::new([[0.0;INPUT];HIDDEN]),b1:[0.0;HIDDEN],w2:[[0.0;HIDDEN];ACTIONS],b2:[0.0;ACTIONS],
            baseline:0.0,steps:0,updates:0,observation_version:version,
            training_segments:Vec::new(),provenance_complete:false,
        };
        let width=if version==ObservationVersion::SuitAwareV2 {
            for expected in [INPUT as u32,HIDDEN as u32,ACTIONS as u32,ENCODER_VERSION,OPTIMIZER_VERSION] {
                if read_u32(&mut reader)?!=expected {return Err(invalid_data("unsupported neural architecture, encoder or optimizer version"));}
            }
            net.steps=read_u64(&mut reader)?;
            net.updates=read_u64(&mut reader)?;
            net.baseline=read_float(&mut reader)?;
            net.provenance_complete=read_bool(&mut reader)?;
            let count=read_u32(&mut reader)? as usize;
            if count>MAX_TRAINING_SEGMENTS {return Err(invalid_data("too many neural training segments"));}
            for _ in 0..count {
                let kind=match read_u8(&mut reader)? {
                    1=>TrainingKind::SelfPlay,2=>TrainingKind::Distillation,
                    _=>return Err(invalid_data("invalid neural training kind")),
                };
                let segment=TrainingSegment{
                    kind,seed:read_u64(&mut reader)?,start_hand:read_u64(&mut reader)?,hands:read_u64(&mut reader)?,
                    learning_rate:read_float(&mut reader)?,rake_bps:read_u16(&mut reader)?,
                    rake_cap_units:read_i64(&mut reader)?,no_flop_no_drop:read_bool(&mut reader)?,
                };
                validate_new_segment(&net.training_segments,&segment)
                    .map_err(|error|invalid_data(error.to_string()))?;
                net.training_segments.push(segment);
            }
            INPUT
        }else{
            net.steps=read_u64(&mut reader)?;
            net.baseline=read_float(&mut reader)?;
            LEGACY_INPUT
        };
        for row in net.w1.iter_mut(){for v in &mut row[..width]{*v=read_weight(&mut reader)?;}}
        for v in &mut net.b1{*v=read_weight(&mut reader)?;}
        for row in &mut net.w2{for v in row{*v=read_weight(&mut reader)?;}}
        for v in &mut net.b2{*v=read_weight(&mut reader)?;}
        if reader.position()!=payload.len() as u64 {return Err(invalid_data("trailing checkpoint bytes"));}
        Ok(net)
    }
    pub fn load_for_training(path:&str)->io::Result<Self> {
        let net=Self::load(path)?;
        net.require_trainable()?;
        Ok(net)
    }
}

struct Gradient {
    w1:Box<[[f64;INPUT];HIDDEN]>,
    b1:[f64;HIDDEN],
    w2:[[f64;HIDDEN];ACTIONS],
    b2:[f64;ACTIONS],
}
impl Gradient {
    fn new()->Self {
        Self{w1:Box::new([[0.0;INPUT];HIDDEN]),b1:[0.0;HIDDEN],w2:[[0.0;HIDDEN];ACTIONS],b2:[0.0;ACTIONS]}
    }
    fn accumulate(&mut self,net:&Net,x:&[f32;INPUT],forward:&Forward,delta:[f64;ACTIONS]) {
        for a in 0..ACTIONS {
            for j in 0..HIDDEN {self.w2[a][j]+=delta[a]*forward.hidden[j] as f64;}
            self.b2[a]+=delta[a];
        }
        for j in 0..HIDDEN {
            if forward.hidden[j]>0.0 {
                let mut hidden_delta=0.0;
                for a in 0..ACTIONS {hidden_delta+=net.w2[a][j] as f64*delta[a];}
                for (i,&value) in x.iter().enumerate() {self.w1[j][i]+=hidden_delta*value as f64;}
                self.b1[j]+=hidden_delta;
            }
        }
    }
}

fn invalid_input(message:impl Into<String>)->io::Error {io::Error::new(io::ErrorKind::InvalidInput,message.into())}
fn invalid_data(message:impl Into<String>)->io::Error {io::Error::new(io::ErrorKind::InvalidData,message.into())}
fn validate_observation(x:&[f32;INPUT],legal:[bool;ACTIONS])->io::Result<()> {
    if x.iter().any(|v|!v.is_finite()) || !legal.iter().any(|&yes|yes) {
        return Err(invalid_input("invalid neural observation or legal-action mask"));
    }
    Ok(())
}
fn validate_segment(segment:&TrainingSegment)->io::Result<u64> {
    if segment.hands==0 || !segment.learning_rate.is_finite() || !(0.0..=0.05).contains(&segment.learning_rate)
        || segment.rake_bps>10_000 || segment.rake_cap_units<0 {
        return Err(invalid_input("invalid neural training segment"));
    }
    segment.start_hand.checked_add(segment.hands).ok_or_else(||invalid_input("training hand index overflow"))
}
fn validate_new_segment(previous:&[TrainingSegment],segment:&TrainingSegment)->io::Result<()> {
    let end=validate_segment(segment)?;
    for prior in previous {
        let prior_end=validate_segment(prior)?;
        if prior.seed==segment.seed && segment.start_hand<prior_end && prior.start_hand<end {
            return Err(invalid_input("overlapping neural training hands for the same seed"));
        }
    }
    Ok(())
}
fn same_training_config(a:&TrainingSegment,b:&TrainingSegment)->bool {
    a.kind==b.kind && a.seed==b.seed && a.learning_rate==b.learning_rate
        && a.rake_bps==b.rake_bps && a.rake_cap_units==b.rake_cap_units
        && a.no_flop_no_drop==b.no_flop_no_drop
}

// FNV-1a detects accidental corruption; it is not an authenticity signature.
fn checksum(bytes:&[u8])->u64 {
    let mut hash=0xcbf29ce484222325_u64;
    for &byte in bytes {hash=(hash^byte as u64).wrapping_mul(0x100000001b3);}
    hash
}
fn read_u8<R:Read>(reader:&mut R)->io::Result<u8> {
    let mut bytes=[0;1];reader.read_exact(&mut bytes)?;Ok(bytes[0])
}
fn read_bool<R:Read>(reader:&mut R)->io::Result<bool> {
    match read_u8(reader)? {0=>Ok(false),1=>Ok(true),_=>Err(invalid_data("invalid neural checkpoint boolean"))}
}
fn read_u16<R:Read>(reader:&mut R)->io::Result<u16> {
    let mut bytes=[0;2];reader.read_exact(&mut bytes)?;Ok(u16::from_le_bytes(bytes))
}
fn read_u32<R:Read>(reader:&mut R)->io::Result<u32> {
    let mut bytes=[0;4];reader.read_exact(&mut bytes)?;Ok(u32::from_le_bytes(bytes))
}
fn read_u64<R:Read>(reader:&mut R)->io::Result<u64> {
    let mut bytes=[0;8];reader.read_exact(&mut bytes)?;Ok(u64::from_le_bytes(bytes))
}
fn read_i64<R:Read>(reader:&mut R)->io::Result<i64> {
    let mut bytes=[0;8];reader.read_exact(&mut bytes)?;Ok(i64::from_le_bytes(bytes))
}
fn read_float<R:Read>(reader:&mut R)->io::Result<f32> {
    let mut bytes=[0;4];reader.read_exact(&mut bytes)?;
    let value=f32::from_le_bytes(bytes);
    if !value.is_finite() {return Err(invalid_data("nonfinite neural checkpoint value"));}
    Ok(value)
}
fn read_weight<R:Read>(reader:&mut R)->io::Result<f32> {
    let value=read_float(reader)?;
    if !(-8.0..=8.0).contains(&value) {return Err(invalid_data("neural weight outside optimizer bounds"));}
    Ok(value)
}
fn save_atomic(path:&Path,bytes:&[u8],create_new:bool)->io::Result<()> {
    let parent=path.parent().filter(|p|!p.as_os_str().is_empty()).unwrap_or_else(||Path::new("."));
    let filename=path.file_name().ok_or_else(||invalid_input("checkpoint path has no filename"))?;
    for _ in 0..32 {
        let mut temporary=std::ffi::OsString::from(".");
        temporary.push(filename);
        temporary.push(format!(".{}.{}.tmp",std::process::id(),TEMP_SEQUENCE.fetch_add(1,Ordering::Relaxed)));
        let temporary=parent.join(temporary);
        let mut file=match OpenOptions::new().write(true).create_new(true).open(&temporary) {
            Ok(file)=>file,
            Err(error) if error.kind()==io::ErrorKind::AlreadyExists=>continue,
            Err(error)=>return Err(error),
        };
        let result=(||->io::Result<()> {
            file.write_all(bytes)?;
            file.sync_all()?;
            drop(file);
            if create_new {
                // A hard link creates the final name only if absent, on the
                // same filesystem as the fully written/synced temporary file.
                fs::hard_link(&temporary,path)?;
                fs::remove_file(&temporary)?;
            }else{
                fs::rename(&temporary,path)?;
            }
            #[cfg(unix)]
            File::open(parent)?.sync_all()?;
            Ok(())
        })();
        if result.is_err() {let _=fs::remove_file(&temporary);}
        return result;
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists,"could not allocate atomic checkpoint temporary file"))
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
        for _ in 0..200{net.reinforce(&x,legal,2,1.0,0.02).unwrap();}
        let after=net.forward(&x,legal).prob;
        assert!(after[2]>before[2],"chosen action probability did not learn");
    }
    #[test]fn distillation_decreases_cross_entropy(){
        let mut net=Net::new(37);
        let x=[0.2;INPUT];let legal=[true;ACTIONS];
        let target=[0.0,1.0,0.0,0.0,0.0,0.0];
        let before=net.forward(&x,legal).prob[1];
        for _ in 0..200 {net.imitate(&x,legal,target,0.01).unwrap();}
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
        net.imitate(&[0.2;INPUT],[true;ACTIONS],[0.0,1.0,0.0,0.0,0.0,0.0],0.0).unwrap();
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
        net.reinforce(&[0.2;INPUT],[true;ACTIONS],1,1.0,0.01).unwrap();
        assert_eq!(net.steps,u64::MAX);
        assert!(net.forward(&[0.2;INPUT],[true;ACTIONS]).prob.iter().all(|p|p.is_finite()));
    }
    #[test]fn imitation_counter_does_not_wrap(){
        let mut net=Net::new(47);
        net.steps=u64::MAX;
        net.imitate(&[0.2;INPUT],[true;ACTIONS],[0.0,1.0,0.0,0.0,0.0,0.0],0.01).unwrap();
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
