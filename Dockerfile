FROM rust:latest AS rbuilder
WORKDIR /build
COPY . .
COPY config /root/.kube/config
RUN cargo test --release
RUN cargo build --release
RUN strip ./target/release/secret-sync

FROM gcr.io/distroless/cc-debian12:latest AS release
WORKDIR /app
COPY --from=rbuilder /build/target/release/secret-sync .

CMD ["./secret-sync", "run"]

