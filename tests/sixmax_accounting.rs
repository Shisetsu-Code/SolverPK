use std::path::Path;
use std::process::Command;

fn run(args: &[&str]) -> String {
    let output=Command::new(env!("CARGO_BIN_EXE_solverpk")).args(args).output().unwrap();
    assert!(output.status.success(),"args={args:?}\n{}",String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap()
}

fn value(line:&str,name:&str)->f64 {
    let prefix=format!("{name}=");
    line.split_whitespace().find_map(|part|part.strip_prefix(&prefix)).unwrap().parse().unwrap()
}

fn checkpoint(directory:&Path)->String {
    directory.join("tabular.bin").to_str().unwrap().to_owned()
}

#[test]
fn full_cli_training_and_duels_account_for_rake_and_bb_units() {
    let directory=std::env::temp_dir().join(format!("solverpk-accounting-cli-{}",std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path=checkpoint(&directory);
    let trained=run(&["--sixmax-train","--hands","120","--seed","817","--learning-rate","0",
        "--checkpoint",&path,"--rake-bps","500","--rake-cap-bb","3"]);
    let house=trained.lines().find(|line|line.starts_with("SIXMAX_HOUSE ")).unwrap();
    let rake_bb=value(house,"rake_bb");
    let mut units=0i64;
    let mut players=0;
    for line in trained.lines().filter(|line|line.starts_with("SIXMAX player=")) {
        let net=value(line,"net_units") as i64;
        units+=net;
        let hand_derived_bb100=net as f64/20_000.0*100.0/120.0;
        assert!((value(line,"bb100")-hand_derived_bb100).abs()<0.00051);
        players+=1;
    }
    assert_eq!(players,6);
    assert!(rake_bb>0.0);
    assert_eq!(units,-(rake_bb*20_000.0).round() as i64);
    let frozen_before=std::fs::read(&path).unwrap();
    for rake in ["0","500"] {
        let output=run(&["--sixmax-duel","--left",&path,"--right",&path,"--hands","120",
            "--seed","991","--rake-bps",rake,"--rake-cap-bb","3"]);
        let line=output.lines().find(|line|line.starts_with("SIXMAX_DUEL ")).unwrap();
        let left=value(line,"left_bb100");
        let right=value(line,"right_bb100");
        let house=value(line,"table_rake_bb100");
        assert_eq!(left,right,"identical frozen policies must split paired results equally");
        if rake=="0" {assert_eq!((left,right,house),(0.0,0.0,0.0));}
        else {
            assert!(left<0.0 && right<0.0 && house>0.0);
            assert!((3.0*(left+right)+house).abs()<0.0031);
        }
    }
    assert_eq!(std::fs::read(&path).unwrap(),frozen_before);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn sixmax_cli_never_silently_defaults_to_rake_free() {
    let output=Command::new(env!("CARGO_BIN_EXE_solverpk"))
        .args(["--sixmax-neural-eval","--hands","120"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("explicit --rake-bps"));
}

#[test]
fn sixmax_cli_rejects_an_insufficient_number_of_deal_clusters() {
    let output=Command::new(env!("CARGO_BIN_EXE_solverpk"))
        .args(["--sixmax-neural-eval","--hands","6","--checkpoint","unused",
            "--tabular","unused","--rake-bps","0"]).output().unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("two independent clusters"));
}
