use std::fs;
use std::process::Stdio;
use std::time::Instant;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::{timeout, Duration};
use uuid::Uuid;

/// Nombre con el que se guarda y se muestra el archivo. Al ejecutarse desde su
/// propia carpeta, el intérprete ya lo reporta así; la sustitución de rutas de
/// más abajo queda como red por si algún mensaje trae la ruta absoluta.
const DISPLAY_NAME: &str = "main.orx";

/// Límite de tiempo de cada ejecución.
const LIMITE_TIEMPO: Duration = Duration::from_secs(10);

/// Lo que se guarda de stdout y de stderr, cada uno. Sin tope, un
/// `while yes { show "x" }` llenaba la memoria del contenedor en los 10
/// segundos de margen, porque la salida entera se acumulaba antes de responder.
const MAX_SALIDA: usize = 256 * 1024;

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

/// Lee todo lo que llegue, pero guarda como mucho `MAX_SALIDA` bytes. El resto
/// se sigue leyendo y se tira: si se dejara de leer, la tubería se llenaría y
/// el programa se quedaría bloqueado escribiendo hasta agotar el tiempo.
async fn leer_con_tope<R: AsyncRead + Unpin>(mut r: R) -> (Vec<u8>, bool) {
    let mut guardado = Vec::new();
    let mut cortado = false;
    let mut trozo = [0u8; 8192];
    loop {
        match r.read(&mut trozo).await {
            Ok(0) | Err(_) => break,
            Ok(n) => {
                let cabe = MAX_SALIDA.saturating_sub(guardado.len()).min(n);
                guardado.extend_from_slice(&trozo[..cabe]);
                if cabe < n {
                    cortado = true;
                }
            }
        }
    }
    (guardado, cortado)
}

fn a_texto(bytes: &[u8], cortado: bool, carpeta: &str) -> String {
    let mut t = String::from_utf8_lossy(bytes).replace(carpeta, "");
    if cortado {
        t.push_str(&format!(
            "\n… salida cortada: solo se muestran los primeros {} KB\n",
            MAX_SALIDA / 1024
        ));
    }
    t
}

/// Borra la carpeta de la ejecución al salir, pase lo que pase.
struct Carpeta(std::path::PathBuf);

impl Drop for Carpeta {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub async fn run_code(code: &str) -> Result<RunResult, Box<dyn std::error::Error + Send + Sync>> {
    // Una carpeta por ejecución como directorio de trabajo: lo que escriba el
    // programa se borra al terminar y una ejecución no ve los archivos de otra.
    let carpeta = Carpeta(std::env::temp_dir().join(format!("orion_{}", Uuid::new_v4())));
    fs::create_dir_all(&carpeta.0)?;
    fs::write(carpeta.0.join(DISPLAY_NAME), code)?;
    let ruta_carpeta = format!("{}{}", carpeta.0.to_string_lossy(), std::path::MAIN_SEPARATOR);

    let start = Instant::now();

    // kill_on_drop: si se agota el tiempo, el futuro se suelta y con él el
    // proceso. Sin esto tokio NO lo mata: un `while yes {}` respondía
    // "tiempo excedido" y seguía gastando CPU para siempre.
    let mut hijo = Command::new("orion")
        .arg(DISPLAY_NAME)
        .current_dir(&carpeta.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .spawn()?;
    let salida = hijo.stdout.take().expect("stdout con pipe");
    let errores = hijo.stderr.take().expect("stderr con pipe");

    let trabajo = async move {
        let ((out, out_cortado), (err, err_cortado)) =
            tokio::join!(leer_con_tope(salida), leer_con_tope(errores));
        let estado = hijo.wait().await;
        (out, out_cortado, err, err_cortado, estado)
    };

    let result = timeout(LIMITE_TIEMPO, trabajo).await;
    let elapsed = start.elapsed().as_millis() as u64;

    match result {
        Ok((out, out_cortado, err, err_cortado, Ok(estado))) => {
            let stdout = a_texto(&out, out_cortado, &ruta_carpeta);
            let stderr = a_texto(&err, err_cortado, &ruta_carpeta);
            let (stderr, exec_ms) = split_exec_time(&stderr);

            Ok(RunResult {
                stdout,
                stderr,
                ok: estado.success(),
                time_ms: elapsed,
                exec_ms,
            })
        }
        Ok((_, _, _, _, Err(e))) => Err(Box::new(e)),
        Err(_) => Ok(RunResult {
            stdout: String::new(),
            stderr: format!(
                "Tiempo de ejecución excedido (límite: {} segundos)",
                LIMITE_TIEMPO.as_secs()
            ),
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

    #[tokio::test]
    async fn la_salida_se_corta_en_el_tope_y_se_sigue_leyendo() {
        let datos = vec![b'x'; MAX_SALIDA + 1000];
        let (guardado, cortado) = leer_con_tope(&datos[..]).await;
        assert_eq!(guardado.len(), MAX_SALIDA);
        assert!(cortado);
    }

    #[tokio::test]
    async fn la_salida_corta_no_se_marca() {
        let (guardado, cortado) = leer_con_tope(&b"hola\n"[..]).await;
        assert_eq!(guardado, b"hola\n");
        assert!(!cortado);
    }
}
