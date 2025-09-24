FROM rust:latest AS rbuilder
ARG SKIP_TESTS
WORKDIR /build
COPY . .
RUN mkdir /root/.kube
RUN [ -z "$SKIP_TESTS" ] && cp config /root/.kube/config && cargo test --release || echo Skipping tests
RUN cargo build --release
RUN strip ./target/release/secret-sync

FROM debian:trixie-slim AS release
WORKDIR /app
COPY --from=rbuilder /build/target/release/secret-sync .

CMD ["./secret-sync", "run"]

