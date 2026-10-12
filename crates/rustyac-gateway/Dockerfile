# SPDX-License-Identifier: MIT OR Apache-2.0
#
# rustyAC's WebSocket gateway as an image: one static program, nothing else.
#
#   docker build -t rustyac-gateway crates/rustyac-gateway
#   docker run --rm -p 127.0.0.1:9601:9601 -e GATEWAY_SERVER=<host>:9600 \
#       -e GATEWAY_ORIGINS=https://rustyac.example.com rustyac-gateway
#
# The build context is this folder alone (the crate has no dependency, so nothing is
# downloaded but the compiler's musl target). The image holds no Assetto Corsa file.
#
# Old processors (no AVX): the program is built for the plain x86-64 baseline
# (`-C target-cpu=x86-64`: SSE2 and nothing newer), from Rust's standard library, which is
# built for the same baseline. `python crates/rustyac-gateway/check_isa.py <binary>` lists
# every instruction outside the baseline of a build made this way: none needs AVX.

# The Rust version is the one CI builds with (RUST_TOOLCHAIN in .github/workflows/ci.yml).
FROM rust:1.97.0-slim-bookworm AS build

RUN rustup target add x86_64-unknown-linux-musl

WORKDIR /src
COPY Cargo.toml ./
COPY src ./src

ENV RUSTFLAGS="-C target-cpu=x86-64 -C target-feature=+crt-static" \
    CARGO_PROFILE_RELEASE_PANIC=abort \
    CARGO_PROFILE_RELEASE_STRIP=symbols
RUN cargo build --release --target x86_64-unknown-linux-musl --bin rustyac-gateway

# ---- stage 2: the program alone ------------------------------------------------------------
FROM scratch

COPY --from=build /src/target/x86_64-unknown-linux-musl/release/rustyac-gateway /rustyac-gateway

# (no root, no shell, no files)
USER 65534:65534
ENV GATEWAY_LISTEN=0.0.0.0:9601
EXPOSE 9601
HEALTHCHECK --interval=30s --timeout=5s --start-period=3s --retries=3 \
    CMD ["/rustyac-gateway", "--check", "127.0.0.1:9601"]
ENTRYPOINT ["/rustyac-gateway"]
