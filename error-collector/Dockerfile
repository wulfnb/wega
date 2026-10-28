# syntax=docker/dockerfile:1
# Build from the Wega repository root so the wire/auth models are shared.
FROM rust:1.98-alpine AS builder
RUN apk add --no-cache musl-dev
WORKDIR /build
COPY error-collector/Cargo.toml error-collector/Cargo.lock ./error-collector/
COPY src/diagnostic_event.rs src/error_report_model.rs src/error_auth.rs ./src/
COPY error-collector/src ./error-collector/src/
ENV RUSTFLAGS="-C target-feature=+crt-static --remap-path-prefix=/build=."
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/build/target \
    target=$(rustc -vV | sed -n 's/^host: //p') \
    && cargo build --locked --release --manifest-path error-collector/Cargo.toml --target "$target" --target-dir /build/target \
    && cp "/build/target/$target/release/wega-error-collector" /wega-error-collector
FROM scratch
LABEL org.opencontainers.image.title="Wega error collector" \
      org.opencontainers.image.licenses="MPL-2.0"
COPY --from=builder /wega-error-collector /wega-error-collector
COPY LICENSE /LICENSE
USER 65532:65532
ENV DATA_DIR=/data BIND_ADDRESS=0.0.0.0:7090 RETENTION_DAYS=30
EXPOSE 7090
HEALTHCHECK --interval=30s --timeout=5s --start-period=5s --retries=3 CMD ["/wega-error-collector", "healthcheck"]
ENTRYPOINT ["/wega-error-collector"]
