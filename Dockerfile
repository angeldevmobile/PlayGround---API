# Build context: raíz de este repo.
# Render: New > Web Service > Runtime Docker (no hace falta Root Directory).
# Railway: Root Directory = playground-api/, Dockerfile = Dockerfile
#
# ORION_RELEASE_URL: binario orion linux-x64 de GitHub Releases.
# Trae default para que el build funcione sin configurar nada; sobreescribible
# como build arg para fijar una versión concreta en vez de "latest".

FROM rust:1-slim-bookworm AS builder
WORKDIR /build

RUN apt-get update && \
    apt-get install -y --no-install-recommends curl ca-certificates && \
    rm -rf /var/lib/apt/lists/*

COPY . .
RUN cargo build --release

ARG ORION_RELEASE_URL=https://github.com/angeldevmobile/Orion/releases/latest/download/orion-linux-x64
RUN curl -fsSL "${ORION_RELEASE_URL}" -o /tmp/orion && chmod +x /tmp/orion

# Runtime: distroless — sin shell, sin paquetes, superficie de ataque mínima
FROM gcr.io/distroless/cc-debian12

COPY --from=builder /build/target/release/playground-api /usr/local/bin/playground-api
COPY --from=builder /tmp/orion /usr/local/bin/orion

EXPOSE 8080

CMD ["/usr/local/bin/playground-api"]
