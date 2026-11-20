# Build from the repository root: docker build -f apps/platform/Dockerfile .
FROM rust:1-slim-trixie AS build
WORKDIR /app
COPY apps/platform/Cargo.toml apps/platform/Cargo.lock ./
COPY apps/platform/src ./src
COPY apps/platform/migrations ./migrations
COPY apps/platform/examples ./examples
RUN cargo build --release --locked --bin aule-platform

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /tmp aule
COPY --from=build /app/target/release/aule-platform /usr/local/bin/aule-platform
USER aule
ENTRYPOINT ["aule-platform"]
CMD ["serve"]
