# playground-api

API de ejecución para el playground de [Orion](https://github.com/angeldevmobile/Orion).
Recibe código Orion por HTTP, lo ejecuta en un contenedor aislado y devuelve la
salida en JSON.

Escrita en Rust con axum y tokio.

## Endpoints

### `POST /run`

Ejecuta un fragmento de código Orion.

Petición:

```json
{
  "code": "show \"hola\""
}
```

Respuesta:

```json
{
  "stdout": "hola\n",
  "stderr": "",
  "ok": true,
  "time_ms": 12,
  "exec_ms": 3.832
}
```

| Campo | Significado |
| ----- | ----------- |
| `stdout` | Lo que el programa imprimió con `show` |
| `stderr` | Errores de compilación y de ejecución. Vacío si todo salió bien |
| `ok` | Código de salida del proceso. **Esta es la señal de éxito o error** |
| `time_ms` | Petición completa: escribir el archivo, lanzar el proceso y leer la salida |
| `exec_ms` | Solo la ejecución del programa, medido por el intérprete. Ausente si no llegó a correr |

Tres detalles al consumir la respuesta:

- La respuesta es JSON, no Orion. `ok` viaja como `true`/`false` porque son los
  booleanos que define JSON. Los `yes`/`no` de Orion viven dentro del string
  `code`, que es lo único que llega al intérprete.
- Decide éxito o error con `ok`, no con `stderr`. Son equivalentes hoy, pero `ok`
  es el contrato.
- `stderr` llega con secuencias de color ANSI, porque el intérprete formatea sus
  errores para terminal. En el navegador hay que quitarlas o convertirlas a HTML.
  El arreglo de fondo es del intérprete (que no pinte colores fuera de una
  terminal ni con `NO_COLOR`), y está anotado en el BACKLOG de Orion.

Las rutas del archivo temporal se sustituyen por `main.orx` en los mensajes de
error, así que las referencias que ve el usuario son a su propio código.

El campo `code` lleva código Orion, no Python ni JavaScript. Para imprimir se usa
`show`, que acepta paréntesis o no, y los booleanos son `yes` y `no`:

```
-- comentario de línea
nombre = "mundo"
activo = yes

show "hola " + nombre

if activo {
    show "listo"
}
```

Códigos de error:

| Código | Motivo |
| ------ | ------ |
| `413`  | El código supera los 10 KB |
| `429`  | Se excedió el límite de peticiones |
| `500`  | Fallo al invocar el intérprete |

### `GET /health`

Devuelve `ok` en texto plano. Pensado para health checks del proveedor de hosting.
Se mantiene en texto a propósito: Render lo usa como `healthCheckPath` y cambiarle
la forma es arriesgado sin ganar nada. La información de build vive en `/version`.

### `GET /version`

Qué compilador sirve este contenedor.

```json
{
  "orion": "v0.1.6",
  "orion_raw": "Orion VM v0.1.6 (Rust) — pipeline completo: lexer + parser + codegen + VM",
  "api": "0.1.0"
}
```

| Campo | Significado |
| ----- | ----------- |
| `orion` | Tag del compilador, o `null` si no se pudo determinar |
| `orion_raw` | Salida literal de `orion --version`; si `orion` es `null`, explica por qué |
| `api` | Versión de este servicio, tomada de `Cargo.toml` |

El Dockerfile fija el binario a un tag concreto (`ARG ORION_VERSION`), y hasta
ahora no había forma de comprobar desde fuera qué versión estaba realmente
desplegada: dos releases pueden comportarse igual al ejecutar código y aun así
importar cuál corre. La única fuente era el panel de Render.

La versión se resuelve **una sola vez al arrancar**, no por petición: el binario
no cambia mientras el contenedor vive. Si falta, el servicio arranca igual y este
endpoint devuelve `null` en vez de caerse.

## Límites

* Tiempo máximo de ejecución: 10 segundos por petición. Al agotarse, el
  proceso se mata: no se queda corriendo en segundo plano.
* Tamaño máximo del código: 10 KB.
* Salida: se guardan como mucho 256 KB de `stdout` y otros tantos de `stderr`.
  Lo que pase de ahí se descarta, y la salida termina con un aviso de que se
  cortó.
* Rate limit: 10 peticiones por minuto y por IP, en ventana deslizante. Detrás
  de un proxy hay que activar `TRUST_PROXY` (ver más abajo), o el límite lo
  comparten todos los usuarios.

Superado el tiempo límite, la respuesta llega con `ok: false` y el motivo en `stderr`.

Cada ejecución corre en su propia carpeta temporal, que es su directorio de
trabajo. Lo que el programa escriba (`pdf.create("x.pdf")`, `fs.write`…) se
queda ahí y se borra al terminar: una ejecución no ve los archivos de otra.

## Variables de entorno

| Variable | Por defecto | Para qué |
| -------- | ----------- | -------- |
| `PORT` | `3001` | Puerto de escucha |
| `ALLOWED_ORIGINS` | `https://docs-orion.onrender.com,http://localhost:8080` | Orígenes que pueden llamar a la API desde el navegador (CORS), separados por comas. Si ninguno es válido, el servicio no arranca |
| `TRUST_PROXY` | sin definir | Con `1`, la IP para el límite de peticiones sale de `X-Forwarded-For`. **Activarla en Render o Railway**: sin ella, todas las peticiones llegan con la IP del proxy. No activarla sin un proxy delante, porque cualquiera podría inventarse una IP en cada petición |

Con `TRUST_PROXY` se toma la **última** IP de `X-Forwarded-For`, no la
primera: el cliente puede mandar la cabecera con lo que quiera, pero el proxy
añade al final la IP desde la que le llegó la conexión.

## Ejecución local

Requiere el binario `orion` disponible en el `PATH`.

```bash
cargo run --release
```

El servidor escucha en `0.0.0.0:3001`. La variable de entorno `PORT` cambia el puerto:

```bash
PORT=8080 cargo run --release
```

Prueba rápida:

```bash
curl -X POST http://localhost:3001/run \
  -H "Content-Type: application/json" \
  -d '{"code":"show \"hola\""}'
```

## Docker

La imagen se construye en dos etapas: compila la API con Rust y descarga el
binario de Orion desde GitHub Releases. El runtime es distroless, sin shell ni
gestor de paquetes.

```bash
docker build -t playground-api .
docker run -p 8080:8080 -e PORT=8080 playground-api
```

La versión de Orion está fijada en el Dockerfile (`ARG ORION_VERSION`). Para
probar otra sin tocar el archivo:

```bash
docker build --build-arg ORION_VERSION=v0.1.5 -t playground-api .
```

Para publicar una versión nueva en el playground, se sube `ORION_VERSION` en el
Dockerfile y se hace push. Conviene comprobar antes que los ejemplos de la web
de documentación siguen funcionando con ella.

La imagen corre como usuario sin privilegios (`distroless:nonroot`, UID 65532).

## Despliegue

* **Render**: New > Web Service, runtime Docker. No hace falta configurar Root Directory.
* **Railway**: Root Directory `playground-api/`, Dockerfile `Dockerfile`.

En ambos casos, define `PORT` con el puerto que asigne el proveedor, y
`TRUST_PROXY=1`, porque los dos ponen un proxy delante del servicio.

## Estructura

```
src/
  main.rs      Router, handlers y arranque del servidor
  runner.rs    Carpeta por ejecución, lanzamiento del intérprete y tope de salida
  limiter.rs   Rate limiting en memoria por IP
Dockerfile     Build multietapa con runtime distroless
```

## Notas de seguridad

CORS solo admite los orígenes de `ALLOWED_ORIGINS`: por defecto, la web de
documentación y su servidor de desarrollo. Esto impide que otra web use el
playground desde el navegador de sus visitantes, pero no protege la API en sí:
cualquiera puede llamarla con `curl`. De eso se encarga el límite de peticiones.

El servicio ejecuta código arbitrario, y el código de Orion puede leer y
escribir archivos, abrir conexiones de red y lanzar procesos. La contención
depende del contenedor:

* Corre sin root (`distroless:nonroot`) y sin shell.
* Cada ejecución tiene su carpeta, que se borra al terminar, y el proceso se
  mata al agotar el tiempo.
* Conviene desplegarlo con CPU y memoria limitadas, y **sin credenciales en el
  entorno**: un programa puede leer sus propias variables de entorno.
