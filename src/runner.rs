use std::fs;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub ok: bool,
    pub time_ms: u64,
}

pub async fn run_code(code: &str) -> Result<RunResult, Box<dyn std::error::Error + Send + Sync>> {
    let id = Uuid::new_v4();
    let path = std::env::temp_dir().join(format!("orion_{}.orx", id));

    fs::write(&path, code)?;

    let start = Instant::now();

    let result = timeout(
        Duration::from_secs(10),
        Command::new("orion")
            .arg(&path)
            .output(),
    )
    .await;

    let elapsed = start.elapsed().as_millis() as u64;
    let _ = fs::remove_file(&path);

    match result {
        Ok(Ok(output)) => Ok(RunResult {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            ok: output.status.success(),
            time_ms: elapsed,
        }),
        Ok(Err(e)) => Err(Box::new(e)),
        Err(_) => Ok(RunResult {
            stdout: String::new(),
            stderr: "Tiempo de ejecución excedido (límite: 10 segundos)".to_string(),
            ok: false,
            time_ms: elapsed,
        }),
    }
}
