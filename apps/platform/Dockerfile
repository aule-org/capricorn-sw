# Build from the repository root: docker build -f apps/platform/Dockerfile .
FROM rust:1-slim-trixie AS build
# The repository's own layout: the platform depends on the session engine by
# path (../../engine), and the engine's crates take their settings from its
# workspace file.
WORKDIR /src
COPY engine/Cargo.toml ./engine/
COPY engine/crates ./engine/crates
COPY apps/platform/Cargo.toml apps/platform/Cargo.lock ./apps/platform/
COPY apps/platform/src ./apps/platform/src
COPY apps/platform/migrations ./apps/platform/migrations
COPY apps/platform/examples ./apps/platform/examples
WORKDIR /src/apps/platform
RUN cargo build --release --locked --bin aule-platform

FROM debian:trixie-slim
RUN apt-get update \
    && apt-get install --yes --no-install-recommends ca-certificates \
    && rm -rf /var/lib/apt/lists/* \
    && useradd --system --uid 10001 --home-dir /tmp aule
COPY --from=build /src/apps/platform/target/release/aule-platform /usr/local/bin/aule-platform
USER aule
ENTRYPOINT ["aule-platform"]
CMD ["serve"]
