use std::fs;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

/// Nombre con el que se muestra el archivo en los mensajes de error. El
/// intérprete reporta la ruta temporal real, que para quien escribe código en
/// el playground no significa nada y además expone rutas internas.
const DISPLAY_NAME: &str = "main.orx";

pub struct RunResult {
    pub stdout: String,
    pub stderr: String,
    pub ok: bool,
    pub time_ms: u64,
    pub exec_ms: Option<f64>,
}

/// El intérprete escribe su tiempo de ejecución en stderr como `[Orion] 1.23 ms`
/// incluso cuando todo sale bien. Se extrae a un campo propio para que stderr
/// quede solo con errores reales y el cliente pueda usarlo como tal.
fn split_exec_time(stderr: &str) -> (String, Option<f64>) {
    let mut exec_ms = None;
    let mut kept: Vec<&str> = Vec::with_capacity(stderr.lines().count());

    for line in stderr.lines() {
        if exec_ms.is_none() {
            if let Some(value) = line
                .trim()
                .strip_prefix("[Orion]")
                .and_then(|rest| rest.trim().strip_suffix("ms"))
                .and_then(|num| num.trim().parse::<f64>().ok())
            {
                exec_ms = Some(value);
                continue;
            }
        }
        kept.push(line);
    }

    if exec_ms.is_none() {
        return (stderr.to_string(), None);
    }

    let mut rest = kept.join("\n");
    if stderr.ends_with('\n') && !rest.is_empty() {
        rest.push('\n');
    }

    (rest, exec_ms)
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
    let temp_path = path.to_string_lossy().into_owned();
    let _ = fs::remove_file(&path);

    match result {
        Ok(Ok(output)) => {
            let stdout = String::from_utf8_lossy(&output.stdout).replace(&temp_path, DISPLAY_NAME);
            let stderr = String::from_utf8_lossy(&output.stderr).replace(&temp_path, DISPLAY_NAME);
            let (stderr, exec_ms) = split_exec_time(&stderr);

            Ok(RunResult {
                stdout,
                stderr,
                ok: output.status.success(),
                time_ms: elapsed,
                exec_ms,
            })
        }
        Ok(Err(e)) => Err(Box::new(e)),
        Err(_) => Ok(RunResult {
            stdout: String::new(),
            stderr: "Tiempo de ejecución excedido (límite: 10 segundos)".to_string(),
            ok: false,
            time_ms: elapsed,
            exec_ms: None,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extrae_el_tiempo_y_deja_stderr_vacio() {
        let (rest, ms) = split_exec_time("[Orion] 3.832 ms\n");
        assert_eq!(rest, "");
        assert_eq!(ms, Some(3.832));
    }

    #[test]
    fn conserva_los_errores_reales() {
        let (rest, ms) = split_exec_time("error: algo falló\n[Orion] 0.19 ms\n");
        assert_eq!(rest, "error: algo falló\n");
        assert_eq!(ms, Some(0.19));
    }

    #[test]
    fn sin_linea_de_tiempo_no_toca_nada() {
        let original = "error en ejecución\n\n  linea 1\n";
        let (rest, ms) = split_exec_time(original);
        assert_eq!(rest, original);
        assert_eq!(ms, None);
    }
}
