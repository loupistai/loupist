use loupist_desktop::eval_harness::{concurrent, Result};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let mut options = concurrent::Options {
        suite: PathBuf::from("apps/loupist/evals/concurrent/suite-v2.json"),
        out: PathBuf::from("concurrent-results.jsonl"),
        pi_agent_dir: PathBuf::new(),
        model: "openai-codex/gpt-6-luna".into(),
        thinking: "max".into(),
        trials: 3,
        trial_index: None,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--help" {
            println!("concurrent_eval --suite <json> --out <new.jsonl> --pi-agent-dir <absolute> [--model provider/model] [--thinking max] [--trials 3] [--trial-index 0|1|2]");
            return Ok(());
        }
        let value = args.next().ok_or("missing option value")?;
        match arg.as_str() {
            "--suite" => options.suite = value.into(),
            "--out" => options.out = value.into(),
            "--pi-agent-dir" => options.pi_agent_dir = value.into(),
            "--model" => options.model = value,
            "--thinking" => options.thinking = value,
            "--trials" => options.trials = value.parse()?,
            "--trial-index" => options.trial_index = Some(value.parse()?),
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    concurrent::run(options).await
}
