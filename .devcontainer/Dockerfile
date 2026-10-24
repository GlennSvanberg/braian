FROM mcr.microsoft.com/devcontainers/rust:1-bookworm

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        build-essential cmake pkg-config \
        libssl-dev libsasl2-dev libcurl4-openssl-dev zlib1g-dev \
        postgresql-client \
    && rm -rf /var/lib/apt/lists/*

RUN curl -L --proto '=https' --tlsv1.2 -sSf \
        https://raw.githubusercontent.com/cargo-bins/cargo-binstall/main/install-from-binstall-release.sh | bash \
    && cargo binstall -y --locked just cargo-nextest bacon cargo-deny \
    && cargo install --locked sqlx-cli --no-default-features --features rustls,postgres \
    && cargo install --locked wasm-bindgen-cli --version 0.2.129 \
    && rm -rf /usr/local/cargo/registry
