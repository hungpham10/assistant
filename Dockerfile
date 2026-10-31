FROM debian:bookworm-slim

ARG TARGETARCH
ARG VERSION

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    tar \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /workspace

RUN set -e && \
    ARCH=${TARGETARCH} && \
    if [ "$ARCH" = "amd64" ]; then TARGET="x86_64-unknown-linux-musl"; fi && \
    if [ "$ARCH" = "arm64" ]; then TARGET="aarch64-unknown-linux-musl"; fi && \
    curl -L -o /tmp/codex.tar.gz "https://github.com/hungpham10/assistant/releases/download/rust-v${VERSION}/open-interpreter-package-${TARGET}.tar.gz" && \
    tar -xzf /tmp/codex.tar.gz -C /tmp && \
    find /tmp -name 'codex' -type f | head -1 | xargs -I{} install -m 755 {} /usr/local/bin/codex && \
    rm -rf /tmp/codex.tar.gz /tmp/codex*

ENTRYPOINT ["codex"]
