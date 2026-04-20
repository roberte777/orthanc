FROM rust:1 AS chef
RUN cargo install cargo-chef --locked
RUN curl -L --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash
RUN cargo binstall dioxus-cli --root /.cargo -y --force
ENV PATH="/.cargo/bin:$PATH"
RUN rustup target add wasm32-unknown-unknown
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json
COPY . .
RUN cargo build --release -p orthanc_server
RUN dx bundle --release --package orthanc_ui --platform web

FROM debian:bookworm-slim AS runtime
RUN apt-get update && apt-get install -y --no-install-recommends \
      ffmpeg \
      ca-certificates \
      libssl3 \
      tini \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/orthanc_server /usr/local/bin/orthanc_server
COPY --from=builder /app/target/dx/orthanc_ui/release/web/ /usr/local/app/ui/
COPY docker/entrypoint.sh /usr/local/bin/entrypoint.sh
RUN chmod +x /usr/local/bin/entrypoint.sh

RUN mkdir -p /data
ENV DATABASE_URL=sqlite:///data/orthanc.db
ENV SERVER_ADDR=0.0.0.0:3001
ENV IP=0.0.0.0
ENV PORT=8080

EXPOSE 8080 3001
VOLUME ["/data"]

ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/entrypoint.sh"]
