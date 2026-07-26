# Stage 1: Build
FROM rust:latest AS builder

WORKDIR /app
COPY . .

ARG FEATURES=default
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release --features "$FEATURES" && \
    cp target/release/fusion-router /usr/local/bin/fusion-router

# Stage 2: Runtime
FROM gcr.io/distroless/cc-debian12:latest

COPY --from=builder /usr/local/bin/fusion-router /usr/local/bin/fusion-router

EXPOSE 8080

HEALTHCHECK --interval=10s --timeout=3s --start-period=5s --retries=3 \
    CMD ["/usr/local/bin/fusion-router", "--health-check"]

USER 65534:65534

ENTRYPOINT ["/usr/local/bin/fusion-router"]
