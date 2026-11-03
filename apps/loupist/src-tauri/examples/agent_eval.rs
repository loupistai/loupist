use loupist_desktop::eval_harness::{self, Options, Result};
use std::path::PathBuf;
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut suite, mut out, mut pi_agent_dir) = (None, None, None);
    let mut agent_assets = None;
    let (mut scenarios, mut trials, mut model, mut thinking, mut keep) =
        (vec![], 1, None, "medium".to_string(), false);
    let (mut resume, mut tags, mut turn_timeout_ms) = (false, vec![], 600000);
    while let Some(arg) = args.next() {
        if arg == "--resume" {
            resume = true;
            continue;
        }
        if arg == "--keep" {
            keep = true;
            continue;
        }
        if arg == "--help" {
            println!("agent_eval --suite <path> --out <traces.jsonl> [--scenarios id,id] [--trials N] [--model provider/model] [--thinking level] --pi-agent-dir <abs> [--agent-assets <dir>] [--keep] [--resume] [--tags tag,tag] [--turn-timeout-ms 600000]");
            return Ok(());
        }
        let value = args.next().ok_or("missing option value")?;
        match arg.as_str() {
            "--suite" => suite = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            "--pi-agent-dir" => pi_agent_dir = Some(PathBuf::from(value)),
            "--agent-assets" => agent_assets = Some(PathBuf::from(value)),
            "--scenarios" => scenarios = value.split(',').map(str::to_owned).collect(),
            "--tags" => tags = value.split(',').map(str::to_owned).collect(),
            "--turn-timeout-ms" => turn_timeout_ms = value.parse()?,
            "--trials" => trials = value.parse()?,
            "--model" => model = Some(value),
            "--thinking" => thinking = value,
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    eval_harness::run(Options {
        suite: suite.ok_or("--suite required")?,
        out: out.ok_or("--out required")?,
        pi_agent_dir: pi_agent_dir.ok_or("--pi-agent-dir required")?,
        agent_assets,
        scenarios,
        trials,
        model,
        thinking,
        keep,
        resume,
        tags,
        turn_timeout_ms,
    })
    .await
}
