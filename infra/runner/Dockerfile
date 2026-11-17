FROM ghcr.io/actions/actions-runner:2.338.2@sha256:6d0b2d80d80f143f77dcb676e50920100bbb547e39e883454d57f45ab8c31300

USER root
RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential cmake pkg-config libssl-dev libcjson-dev libmbedtls-dev python3 python3-venv python-is-python3 zstd \
    && rm -rf /var/lib/apt/lists/* \
    && mkdir -p /opt/hostedtoolcache \
    && chown runner:runner /opt/hostedtoolcache

ENV RUNNER_TOOL_CACHE=/opt/hostedtoolcache
COPY --chmod=755 entrypoint.sh /usr/local/bin/aule-runner
USER runner
WORKDIR /home/runner
ENTRYPOINT ["/usr/local/bin/aule-runner"]
