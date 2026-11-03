use loupist_desktop::eval_harness::{world, Result};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    let mut options = world::Options::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--keep" {
            options.keep = true;
            continue;
        }
        if arg == "--help" {
            println!("world_scale_eval --suite <scale-corpus.json> --out <new.jsonl> --pi-agent-dir <absolute> [--trials 3] [--trial-index 0|1|2] [--scenarios W-SCALE-000,W-SCALE-020,W-SCALE-100] [--model provider/model] [--thinking medium] [--world-model provider/model] [--world-thinking high] [--turn-timeout-ms 600000] [--world-timeout-ms 120000] [--stage-timeout-ms 900000] [--resources <staged resources>] [--keep]");
            return Ok(());
        }
        let value = args.next().ok_or("missing option value")?;
        match arg.as_str() {
            "--suite" => options.suite = PathBuf::from(value),
            "--out" => options.out = PathBuf::from(value),
            "--pi-agent-dir" => options.pi_agent_dir = PathBuf::from(value),
            "--trials" => options.trials = value.parse()?,
            "--trial-index" => options.trial_index = Some(value.parse()?),
            "--scenarios" => options.scenarios = value.split(',').map(str::to_owned).collect(),
            "--condition" => options.condition = Some(value),
            "--model" => options.model = value,
            "--thinking" => options.thinking = value,
            "--world-model" => options.world_model = value,
            "--world-thinking" => options.world_thinking = value,
            "--turn-timeout-ms" => options.turn_timeout_ms = value.parse()?,
            "--world-timeout-ms" => options.world_timeout_ms = value.parse()?,
            "--stage-timeout-ms" => options.stage_timeout_ms = value.parse()?,
            "--resources" => options.resources = Some(PathBuf::from(value)),
            _ => return Err(format!("unknown option: {arg}").into()),
        }
    }
    world::scale::run(options).await
}
