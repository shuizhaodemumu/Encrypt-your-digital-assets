FROM rust:1-bookworm AS builder
WORKDIR /build
COPY online-license ./online-license
COPY license-server/Cargo.toml license-server/Cargo.lock ./license-server/
COPY license-server/src ./license-server/src
COPY license-server/static ./license-server/static
RUN cargo build --locked --release -j 2 --manifest-path license-server/Cargo.toml
FROM debian:bookworm-slim
RUN groupadd -g 10001 license && useradd -u 10001 -g license -M license && mkdir /data && chown license:license /data
COPY --from=builder /build/license-server/target/release/assets-license-server /usr/local/bin/
USER 10001:10001
VOLUME /data
EXPOSE 8080
ENTRYPOINT ["assets-license-server"]
