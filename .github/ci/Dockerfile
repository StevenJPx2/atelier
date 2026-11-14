# Tooling only: no project code, credentials or Docker socket enter this image.
FROM node:24-bookworm-slim@sha256:dc26ae92c9fb15cf6b1d63e20f0944b2efe0f115b20e00be48fb23fceb061c54 AS node
FROM golang:1.25-bookworm@sha256:3b4a11519ad929d1e1d261a12cff056f0c85b735253d7d861346b9c6f8b36437 AS go
FROM rust:1.99.1-bookworm@sha256:e6f2cff35d0f309acf8433b4f05878ed77092602c5065cc81801177da0bb3a9c

# shellcheck keeps actionlint's run-script checks, as on the hosted runner.
RUN apt-get update && apt-get install --yes --no-install-recommends \
    ca-certificates curl git libssl-dev pkg-config procps python3 python3-venv \
    shellcheck unzip \
    && rm -rf /var/lib/apt/lists/*
# Debian 12's pipx 1.1.0 cannot run `ruff==VERSION`; pin a current one.
RUN python3 -m venv /opt/pipx \
    && /opt/pipx/bin/pip install --no-cache-dir pipx==1.7.1 \
    && ln -s /opt/pipx/bin/pipx /usr/local/bin/pipx
COPY --from=node /usr/local/bin /usr/local/bin
COPY --from=node /usr/local/lib/node_modules /usr/local/lib/node_modules
COPY --from=go /usr/local/go /usr/local/go
RUN rustup component add rustfmt clippy

RUN useradd --uid 10001 --create-home ci && install -d -o ci -g ci /work
COPY cargo.py /opt/ci/bin/cargo
COPY entrypoint.sh /opt/ci/entrypoint.sh
COPY reports.py test_reports.py /opt/ci/
RUN chmod 0555 /opt/ci/bin/cargo /opt/ci/entrypoint.sh

ENV HOME=/home/ci \
    PATH=/opt/ci/bin:/usr/local/cargo/bin:/usr/local/go/bin:/usr/local/bin:/usr/bin:/bin
USER 10001:10001
WORKDIR /work
ENTRYPOINT ["/opt/ci/entrypoint.sh"]
