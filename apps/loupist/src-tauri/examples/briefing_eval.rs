use loupist_desktop::eval_harness::{briefing, Result};
use std::path::PathBuf;
#[tokio::main]
async fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let (mut agent, mut out) = (None, None);
    while let Some(arg) = args.next() {
        let value = args.next().ok_or("Missing option value")?;
        match arg.as_str() {
            "--pi-agent-dir" => agent = Some(PathBuf::from(value)),
            "--out" => out = Some(PathBuf::from(value)),
            _ => return Err(format!("Unknown option {arg}").into()),
        }
    }
    briefing::run(
        agent.ok_or("--pi-agent-dir required")?,
        out.ok_or("--out required")?,
    )
    .await
}
