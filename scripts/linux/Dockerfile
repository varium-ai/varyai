FROM ubuntu:22.04

ARG TARGETARCH
ARG NODE_VERSION=22.12.0
ENV DEBIAN_FRONTEND=noninteractive

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates build-essential pkg-config curl wget file patchelf \
    libssl-dev libgtk-3-dev librsvg2-dev libayatana-appindicator3-dev \
    libwebkit2gtk-4.1-dev libsoup-3.0-dev libxdo-dev libfuse2 \
    rpm xdg-utils rsync git python3 xz-utils \
    && rm -rf /var/lib/apt/lists/*

RUN case "$TARGETARCH" in amd64) node_arch=x64 ;; arm64) node_arch=arm64 ;; *) exit 1 ;; esac \
    && archive="node-v${NODE_VERSION}-linux-${node_arch}.tar.xz" \
    && curl -fSL "https://nodejs.org/dist/v${NODE_VERSION}/${archive}" -o "/tmp/${archive}" \
    && curl -fSL "https://nodejs.org/dist/v${NODE_VERSION}/SHASUMS256.txt" -o /tmp/SHASUMS256.txt \
    && cd /tmp && awk -v archive="$archive" '$2 == archive' SHASUMS256.txt | sha256sum --check --strict \
    && tar -xJf "$archive" -C /usr/local --strip-components=1 \
    && rm "$archive" SHASUMS256.txt \
    && npm install --global pnpm@10.12.3

ENV CARGO_HOME=/opt/cargo RUSTUP_HOME=/opt/rustup
ENV PATH=/opt/cargo/bin:$PATH
RUN curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs -o /tmp/rustup.sh \
    && sh /tmp/rustup.sh -y --profile minimal --default-toolchain stable \
    && rustup component add rustfmt clippy \
    && rm /tmp/rustup.sh

# AppImage helper tools can run without mounting FUSE in the container.
ENV APPIMAGE_EXTRACT_AND_RUN=1 CI=true
COPY build.sh /usr/local/bin/varyai-linux-build
RUN chmod +x /usr/local/bin/varyai-linux-build
WORKDIR /work/source
ENTRYPOINT ["/usr/local/bin/varyai-linux-build"]
