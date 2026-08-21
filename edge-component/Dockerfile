# Multi-stage cross-compilation: x86_64/arm64 host -> aarch64-unknown-linux-gnu.
#
# ort linking (Phase 3 spike outcome): ort 2.0.0-rc.12's default
# `download-binaries` feature fetches pyke's prebuilt ONNX Runtime for
# aarch64-unknown-linux-gnu and links it STATICALLY — no libonnxruntime.so
# on the target, satisfying NFR-5 (glibc-only dynamic deps). No extra env
# vars needed.
#
# The Greengrass SDK builds its bundled C library at compile time, so the
# builder needs gcc-aarch64-linux-gnu (cross C toolchain) and libclang
# (bindgen).
#
# Build:  docker build -t edge-ai-classifier-build -f Dockerfile .
# Output: scripts/build.sh extracts /edge-ai-classifier from the image.

# Rust >= 1.89 required: gg_sdk uses `[0; _]` (generic_arg_infer).
# trixie base (gcc-14 cross toolchain): pyke's prebuilt ONNX Runtime
# static lib needs GCC-13+ libstdc++ symbols (__cxa_call_terminate), so
# bookworm's gcc-12 cannot link it. Consequence: the binary's glibc floor
# is set by this toolchain (~2.38) — see the note in docs/architecture.md
# for older-glibc fleets.
FROM public.ecr.aws/docker/library/rust:1.93-slim AS builder
RUN apt-get update && apt-get install -y --no-install-recommends \
    gcc-aarch64-linux-gnu \
    libc6-dev-arm64-cross \
    libclang-dev \
    clang \
    cmake \
    pkg-config \
    && rm -rf /var/lib/apt/lists/*
RUN rustup target add aarch64-unknown-linux-gnu

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src/ src/
COPY benches/ benches/
COPY tests/ tests/

ENV CARGO_TARGET_AARCH64_UNKNOWN_LINUX_GNU_LINKER=aarch64-linux-gnu-gcc
# The gg_sdk C build uses these for its cross-compiled objects.
ENV CC_aarch64_unknown_linux_gnu=aarch64-linux-gnu-gcc
ENV AR_aarch64_unknown_linux_gnu=aarch64-linux-gnu-ar

RUN cargo build --release --features greengrass --target aarch64-unknown-linux-gnu
RUN aarch64-linux-gnu-strip target/aarch64-unknown-linux-gnu/release/edge-ai-classifier \
    && ls -la target/aarch64-unknown-linux-gnu/release/edge-ai-classifier

FROM scratch AS output
COPY --from=builder /app/target/aarch64-unknown-linux-gnu/release/edge-ai-classifier /
