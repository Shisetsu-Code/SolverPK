use std::path::{Path,PathBuf};
use std::process::{Command,Output};

fn directory(label:&str)->PathBuf{
    let p=std::env::temp_dir().join(format!("solverpk-neural-v2-{label}-{}",std::process::id()));
    std::fs::create_dir_all(&p).unwrap();p
}
fn train(path:&Path,hands:&str,resume:bool)->Output{
    let mut cmd=Command::new(env!("CARGO_BIN_EXE_solverpk"));
    cmd.args(["--sixmax-neural-train","--hands",hands,"--seed","4471",
      "--learning-rate","0.0005","--rake-bps","500","--rake-cap-bb","3",
      "--checkpoint",path.to_str().unwrap()]);
    if resume{cmd.arg("--resume");}cmd.output().unwrap()
}
fn successful(output:Output)->String{
    assert!(output.status.success(),"{}",String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}
#[test]
fn resumed_neural_training_matches_a_continuous_run_byte_for_byte(){
    let dir=directory("resume");let continuous=dir.join("continuous.bin");let split=dir.join("split.bin");
    successful(train(&continuous,"36",false));
    successful(train(&split,"12",false));
    successful(train(&split,"24",true));
    assert_eq!(std::fs::read(&continuous).unwrap(),std::fs::read(&split).unwrap(),
        "resume repeated earlier deals or changed update/metadata ordering");
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn new_training_writes_a_versioned_suit_aware_checkpoint(){
    let dir=directory("format");let path=dir.join("new.bin");
    successful(train(&path,"12",false));
    let bytes=std::fs::read(&path).unwrap();
    assert_eq!(&bytes[..8],b"SPKNN002","new input semantics must not masquerade as SPKNN001");
    std::fs::remove_dir_all(dir).unwrap();
}
#[test]
fn historical_checkpoint_cannot_be_silently_resumed_or_overwritten(){
    let dir=directory("legacy");let path=dir.join("old.bin");
    // Valid literal legacy fixture: steps/baseline zero, all 2278 weights zero.
    let mut bytes=b"SPKNN001".to_vec();bytes.extend_from_slice(&[0u8;12]);
    bytes.resize(20+2278*4,0);std::fs::write(&path,&bytes).unwrap();
    let output=train(&path,"12",true);
    assert!(!output.status.success(),"legacy weights were accepted for a changed training schema");
    let err=String::from_utf8_lossy(&output.stderr);
    assert!(err.contains("SPKNN001")||err.contains("legacy"),"{err}");
    assert_eq!(std::fs::read(&path).unwrap(),bytes);
    std::fs::remove_dir_all(dir).unwrap();
}
