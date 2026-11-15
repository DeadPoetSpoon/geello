# syntax=docker/dockerfile:1
# Both variants build the checked-out, locked source for x86_64 musl.
ARG RUST_IMAGE=rust:1.96.0-alpine3.23
ARG ALPINE_IMAGE=alpine:3.23.3
FROM ${RUST_IMAGE} AS builder
RUN apk add --no-cache build-base cmake clang-dev llvm-dev linux-headers
WORKDIR /source
COPY . .
ARG GEELLO_FEATURES=server-full
# Vulkan's loader/ICD uses dlopen; dynamically link musl for that boundary.
ENV RUSTFLAGS="-C target-feature=-crt-static"
RUN cargo build --locked --release --target x86_64-unknown-linux-musl --no-default-features --features ${GEELLO_FEATURES}

FROM ${ALPINE_IMAGE} AS runtime
RUN apk add --no-cache libgcc libstdc++ vulkan-loader \
    && addgroup -g 10001 geello && adduser -D -H -u 10001 -G geello geello \
    && mkdir -p /data /run/geello && chown geello:geello /data /run/geello
COPY --from=builder /source/target/x86_64-unknown-linux-musl/release/geello /usr/local/bin/geello
COPY deploy/server.gpu.toml /etc/geello/server.toml
ENV XDG_RUNTIME_DIR=/run/geello
USER 10001:10001
VOLUME ["/data"]
EXPOSE 3000
STOPSIGNAL SIGTERM
ENTRYPOINT ["/usr/local/bin/geello"]
CMD ["--config", "/etc/geello/server.toml"]
