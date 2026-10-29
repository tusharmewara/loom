# loom-egress-proxy container image.
#
# The crate is deliberately standalone, so this build needs only this
# directory. Pure-Rust deps (tokio, anyhow) mean the musl target builds
# without a C toolchain.
# ARG BASE=rust:1-slim for a slim pull; a locally-cached rust image can be
# passed instead to avoid the download.
ARG BASE=rust:1-slim
FROM ${BASE} AS build
WORKDIR /src
COPY . .
RUN rustup target add x86_64-unknown-linux-musl \
    && cargo build --release --target x86_64-unknown-linux-musl

FROM alpine:3
RUN adduser -D -S non-root
COPY --from=build /src/target/x86_64-unknown-linux-musl/release/loom-egress-proxy /usr/local/bin/loom-egress-proxy
USER non-root
ENTRYPOINT ["loom-egress-proxy"]
