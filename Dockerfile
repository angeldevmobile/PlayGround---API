# Build context: raíz de este repo.
# Render: New > Web Service > Runtime Docker (no hace falta Root Directory).
# Railway: Root Directory = playground-api/, Dockerfile = Dockerfile
#
# ORION_VERSION / ORION_RELEASE_URL: binario orion linux-x64 de GitHub Releases,
# fijado a un tag concreto. Ambos son build args: se puede probar otra versión
# con --build-arg ORION_VERSION=vX.Y.Z sin tocar el archivo.
#
# Debian 13 (trixie) en todas las etapas, no 12 (bookworm): el binario de orion
# se compila en ubuntu-latest y exige GLIBC_2.39, mientras que bookworm trae
# 2.36. Con bookworm el loader falla en runtime con "GLIBC_2.39 not found".

FROM rust:1-slim-trixie AS builder
WORKDIR /build

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release

# Versión FIJADA, no "latest". Dos razones:
#   1. Reproducibilidad: el repo dice qué compilador corre el playground. Con
#      "latest" había que sondear el servicio para averiguarlo.
#   2. Caché de Docker: esta capa no depende de nada que cambie, así que con
#      una URL constante Docker la reutiliza y NO vuelve a descargar. Un deploy
#      salía verde sirviendo el binario viejo. Al subir la versión aquí, la
#      capa cambia y se rehace sola.
# Al publicar un release nuevo: sube el número y haz push.
ARG ORION_VERSION=v0.1.2
ARG ORION_RELEASE_URL=https://github.com/angeldevmobile/Orion/releases/download/${ORION_VERSION}/orion-linux-x64
RUN curl -fsSL "${ORION_RELEASE_URL}" -o /tmp/orion && chmod +x /tmp/orion \
    && /tmp/orion --version

# Librerías dinámicas que pide orion y que distroless/cc no incluye:
# libssl.so.3, libcrypto.so.3 y libz.so.1. Las demás (libgcc_s, libm, libc)
# ya vienen en la imagen cc.
FROM debian:trixie-slim AS libs
RUN apt-get update && \
    apt-get install -y --no-install-recommends libssl3 zlib1g && \
    rm -rf /var/lib/apt/lists/*

# Runtime: distroless, sin shell ni gestor de paquetes, superficie mínima.
# Importa porque este servicio ejecuta código arbitrario de usuarios.
FROM gcr.io/distroless/cc-debian13

COPY --from=libs /usr/lib/x86_64-linux-gnu/libssl.so.3 /usr/lib/x86_64-linux-gnu/
COPY --from=libs /usr/lib/x86_64-linux-gnu/libcrypto.so.3 /usr/lib/x86_64-linux-gnu/
COPY --from=libs /usr/lib/x86_64-linux-gnu/libz.so.1 /usr/lib/x86_64-linux-gnu/

COPY --from=builder /build/target/release/playground-api /usr/local/bin/playground-api
COPY --from=builder /tmp/orion /usr/local/bin/orion

# Solo informativo. El puerto real lo decide la variable PORT (default 3001).
EXPOSE 3001

CMD ["/usr/local/bin/playground-api"]
