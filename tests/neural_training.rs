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

#[test]
fn starting_a_new_run_does_not_replace_an_existing_checkpoint(){
    let dir=directory("no-clobber");let path=dir.join("keep.bin");
    successful(train(&path,"12",false));
    let before=std::fs::read(&path).unwrap();
    let output=train(&path,"12",false);
    assert!(!output.status.success(),"a fresh run silently replaced an existing trained model");
    assert_eq!(std::fs::read(&path).unwrap(),before);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn malformed_or_duplicate_training_arguments_do_not_silently_change_provenance(){
    let dir=directory("invalid-args");
    for (i,extra) in [vec!["--seed","not-a-seed"],vec!["--learning-rate","not-a-rate"],
        vec!["--hands","18"],vec!["--seed"]].iter().enumerate(){
        let path=dir.join(format!("invalid-{i}.bin"));
        let output=Command::new(env!("CARGO_BIN_EXE_solverpk"))
            .args(["--sixmax-neural-train","--hands","12","--rake-bps","0",
                "--checkpoint",path.to_str().unwrap()]).args(extra).output().unwrap();
        assert!(!output.status.success(),"invalid configuration was accepted: {extra:?}");
        assert!(!path.exists(),"invalid input wrote a misleading checkpoint");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn distillation_resume_is_exact_and_teacher_provenance_stays_unverified(){
    let dir=directory("distill");let teacher=dir.join("teacher.bin");
    let run=|args:&[&str]|successful(Command::new(env!("CARGO_BIN_EXE_solverpk")).args(args).output().unwrap());
    run(&["--sixmax-train","--hands","12","--seed","713","--learning-rate","0",
        "--checkpoint",teacher.to_str().unwrap(),"--rake-bps","0"]);
    let continuous=dir.join("continuous.bin");let split=dir.join("split.bin");
    let distill=|path:&Path,hands:&str,resume:bool|{
        let mut args=vec!["--sixmax-neural-distill","--teacher",teacher.to_str().unwrap(),
            "--hands",hands,"--seed","717","--learning-rate","0.001","--rake-bps","0",
            "--checkpoint",path.to_str().unwrap()];
        if resume{args.push("--resume");}run(&args)
    };
    distill(&continuous,"12",false);distill(&split,"6",false);distill(&split,"6",true);
    assert!(std::fs::read(&continuous).unwrap()==std::fs::read(&split).unwrap(),
        "distillation resume did not preserve weights and metadata");
    let out=run(&["--sixmax-neural-league","--checkpoint",split.to_str().unwrap(),
        "--hands","12","--seed","3003","--rake-bps","0"]);
    assert!(out.lines().filter(|s|s.starts_with("SIXMAX_LEAGUE_ROW "))
        .all(|s|s.contains("split=unverified_external")),"{out}");
    std::fs::remove_dir_all(dir).unwrap();
}
