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
  "code": "print(\"hola\")"
}
```

Respuesta:

```json
{
  "stdout": "hola\n",
  "stderr": "",
  "ok": true,
  "time_ms": 12
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

## Límites

* Tiempo máximo de ejecución: 10 segundos por petición.
* Tamaño máximo del código: 10 KB.
* Rate limit: 10 peticiones por minuto y por IP, en ventana deslizante.

Superado el tiempo límite, la respuesta llega con `ok: false` y el motivo en `stderr`.

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
  -d '{"code":"print(\"hola\")"}'
```

## Docker

La imagen se construye en dos etapas: compila la API con Rust y descarga el
binario de Orion desde GitHub Releases. El runtime es distroless, sin shell ni
gestor de paquetes.

```bash
docker build -t playground-api .
docker run -p 8080:8080 -e PORT=8080 playground-api
```

Para fijar una versión concreta de Orion en vez de la última:

```bash
docker build \
  --build-arg ORION_RELEASE_URL=https://github.com/angeldevmobile/Orion/releases/download/v1.0.0/orion-linux-x64 \
  -t playground-api .
```

## Despliegue

* **Render**: New > Web Service, runtime Docker. No hace falta configurar Root Directory.
* **Railway**: Root Directory `playground-api/`, Dockerfile `Dockerfile`.

En ambos casos, define `PORT` con el puerto que asigne el proveedor.

## Estructura

```
src/
  main.rs      Router, handlers y arranque del servidor
  runner.rs    Escritura del archivo temporal y ejecución del intérprete
  limiter.rs   Rate limiting en memoria por IP
Dockerfile     Build multietapa con runtime distroless
```

## Notas de seguridad

CORS está abierto a cualquier origen porque el servicio está pensado para
alimentar el playground web. La ejecución de código arbitrario depende del
aislamiento del contenedor, así que conviene desplegarlo con recursos limitados
y sin credenciales en el entorno.
