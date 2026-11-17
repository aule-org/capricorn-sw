# Build from the repository root: docker build -f apps/agent/Dockerfile .
FROM python:3.14-slim AS build
COPY --from=ghcr.io/astral-sh/uv:0.13.6 /uv /usr/local/bin/uv
ENV UV_COMPILE_BYTECODE=1 UV_LINK_MODE=copy UV_PYTHON_DOWNLOADS=never
WORKDIR /app
COPY apps/agent/pyproject.toml apps/agent/uv.lock ./
RUN uv sync --locked --no-dev --no-install-project
COPY apps/agent/src ./src
RUN uv sync --locked --no-dev --no-editable

FROM python:3.14-slim
RUN useradd --system --uid 10001 --home-dir /tmp capricorn
COPY --from=build /app/.venv /app/.venv
COPY contracts /app/contracts
ENV PATH=/app/.venv/bin:$PATH \
    CAPRICORN_CONTRACTS_DIR=/app/contracts \
    HOME=/tmp \
    PYTHONUNBUFFERED=1
USER capricorn
CMD ["python", "-m", "capricorn_agent.worker", "start"]
