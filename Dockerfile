FROM debian:bookworm-slim

ARG TARGETARCH
ARG VERSION

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates \
    curl \
    git \
    && rm -rf /var/lib/apt/lists/*

# Download binary from release or copy local
COPY codex-${TARGETARCH}* /usr/local/bin/codex 2>/dev/null || true
COPY codex /usr/local/bin/codex 2>/dev/null || true

RUN if [ ! -f /usr/local/bin/codex ]; then \
      ARCH=${TARGETARCH}; \
      if [ "$ARCH" = "amd64" ]; then ARCH=x86_64; fi; \
      if [ "$ARCH" = "arm64" ]; then ARCH=aarch64; fi; \
      curl -L -o /usr/local/bin/codex "https://github.com/hungpham10/assistant/releases/download/rust-v${VERSION}/codex-${ARCH}-unknown-linux-musl" && \
      chmod +x /usr/local/bin/codex; \
    else \
      chmod +x /usr/local/bin/codex; \
    fi

WORKDIR /workspace
ENTRYPOINT ["codex"]
