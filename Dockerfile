# syntax=docker/dockerfile:1.7
ARG RUST_IMAGE=rust:1.98.0-slim-trixie
FROM ${RUST_IMAGE} AS builder
ARG TARGETARCH
ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_INCREMENTAL=0 \
    CARGO_PROFILE_RELEASE_DEBUG=0 \
    CARGO_PROFILE_RELEASE_OPT_LEVEL=2 \
    CARGO_PROFILE_RELEASE_LTO=false \
    RUSTFLAGS="-C link-arg=-fuse-ld=lld"
# Shared with the CI jobs that build the same binaries outside Docker.
COPY docker/builder-packages.txt /tmp/builder-packages.txt
RUN apt-get update && xargs -a /tmp/builder-packages.txt apt-get install -y --no-install-recommends \
    && rm -rf /var/lib/apt/lists/*
# Third-party license notices (cargo-about). Set LICENSE_NOTICES=0 to skip the
# tool for quick local builds; the image then carries only LICENSE and NOTICE.
ARG LICENSE_NOTICES=1
ARG CARGO_ABOUT_VERSION=0.9.2
RUN --mount=type=cache,id=servo-registry-${TARGETARCH},target=/usr/local/cargo/registry \
    if [ "${LICENSE_NOTICES}" = 1 ]; then \
        cargo install cargo-about --locked --features cli --version "${CARGO_ABOUT_VERSION}"; \
    fi
WORKDIR /src
COPY Cargo.toml Cargo.lock build.rs ./
COPY vendor ./vendor
COPY src ./src
COPY engines ./engines
COPY scripts/build-wega-engine.py scripts/v8_prebuilt.py ./scripts/
COPY assets/icons/browser-icon-256.png ./assets/icons/browser-icon-256.png
ARG BUILD_PROFILE=release
ARG BUILD_JOBS=4
ARG BUILD_FEATURES=desktop
RUN --mount=type=cache,id=servo-registry-${TARGETARCH},target=/usr/local/cargo/registry \
    --mount=type=cache,id=servo-git-${TARGETARCH},target=/usr/local/cargo/git \
    --mount=type=cache,id=wega-target-${TARGETARCH}-${BUILD_PROFILE},target=/src/target \
    touch src/main.rs assets/icons/browser-icon-256.png && \
    cargo build --locked --features "${BUILD_FEATURES}" --profile "${BUILD_PROFILE}" --jobs "${BUILD_JOBS}" && \
    python3 scripts/build-wega-engine.py --profile "${BUILD_PROFILE}" --jobs "${BUILD_JOBS}" && \
    mkdir -p /out && \
    if [ "${BUILD_PROFILE}" = dev ]; then cp target/debug/wega target/debug/wega-engine /out/; \
    else cp "target/${BUILD_PROFILE}/wega" "target/${BUILD_PROFILE}/wega-engine" /out/; fi && \
    strip /out/wega /out/wega-engine
# Copied after the build so notice edits do not invalidate the compile layer.
COPY about.toml about.hbs LICENSE NOTICE ./
COPY scripts/third-party-notices.sh ./scripts/third-party-notices.sh
# Appended to NOTICE by the script (CD passes the release commit).
ARG WEGA_SOURCE_COMMIT=
RUN --mount=type=cache,id=servo-registry-${TARGETARCH},target=/usr/local/cargo/registry \
    --mount=type=cache,id=servo-git-${TARGETARCH},target=/usr/local/cargo/git \
    if [ "${LICENSE_NOTICES}" = 1 ]; then \
        WEGA_SOURCE_COMMIT="${WEGA_SOURCE_COMMIT}" bash scripts/third-party-notices.sh /out/notices --features "${BUILD_FEATURES}"; \
    else \
        mkdir -p /out/notices && cp LICENSE NOTICE /out/notices/; \
    fi

FROM debian:trixie-slim AS desktop
ENV DEBIAN_FRONTEND=noninteractive
ENV BROWSER_METRICS_SCOPE=container
# CJK fonts add roughly 90 MB but are needed to read Chinese, Japanese and
# Korean sites; build with CJK_FONTS=0 to omit them.
ARG CJK_FONTS=1
# No gstreamer1.0-libav: it bundles GPL-licensed FFmpeg components. Without it,
# H.264/AAC media may not play; VP8/VP9/AV1/Opus/Vorbis use other plugins.
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl tini gosu supervisor xvfb x11vnc x11-utils openbox \
    novnc websockify nginx-light openssl python3-cryptography \
    libegl1 libgl1 libgl1-mesa-dri libgles2 libgbm1 libvulkan1 \
    libx11-6 libxcursor1 libxi6 libxrandr2 libxinerama1 libxkbcommon0 libxkbcommon-x11-0 \
    libdbus-1-3 libudev1 libfontconfig1 libfreetype6 libharfbuzz0b libunwind8 \
    gstreamer1.0-plugins-base gstreamer1.0-plugins-good gstreamer1.0-plugins-bad \
    gstreamer1.0-nice libgstreamer-gl1.0-0 \
    fonts-dejavu-core fonts-noto-core fonts-noto-color-emoji \
    $(if [ "${CJK_FONTS}" = 1 ]; then echo fonts-noto-cjk; fi) \
    && rm -rf /var/lib/apt/lists/* /etc/nginx/sites-enabled/default \
    && groupadd --gid 1000 browser && useradd --uid 1000 --gid browser --home-dir /config --shell /bin/sh browser \
    && groupadd --system --gid 1001 wega-auth \
    && useradd --system --uid 1001 --gid wega-auth --home-dir /nonexistent --no-create-home --shell /usr/sbin/nologin wega-auth \
    && groupadd --system --gid 1002 wega-ipc && usermod -aG wega-ipc browser && usermod -aG wega-ipc wega-auth \
    && mkdir -p /opt/wega/viewer /config /run/wega/browser \
    && chown browser:browser /config /run/wega/browser
COPY docker/viewer/ /opt/wega/viewer/
COPY docker/patch-novnc.py /opt/wega/patch-novnc.py
RUN python3 /opt/wega/patch-novnc.py /usr/share/novnc
COPY docker/auth-service.py docker/supervisor-watchdog.py /opt/wega/
COPY docker/nginx.conf /etc/nginx/nginx.conf
COPY docker/supervisord.conf /etc/wega-supervisord.conf
COPY docker/openbox.xml /etc/wega-openbox.xml
COPY --chmod=755 docker/entrypoint.sh docker/start-browser.sh /usr/local/bin/
ENV WEGA_FILE_HANDOFF_DIR=/run/wega/chooser \
    DISPLAY=:99 \
    XDG_RUNTIME_DIR=/run/wega/browser \
    XDG_CONFIG_HOME=/config/.config \
    XDG_CACHE_HOME=/config/.cache \
    XDG_DATA_HOME=/config/.local/share \
    WINIT_UNIX_BACKEND=x11 \
    BROWSER_RESOLUTION=1280x800 \
    BROWSER_START_URL=about:blank \
    BROWSER_DOWNLOAD_DIR=/config/Downloads \
    WEGA_RENDER_THREADS=2 \
    WEGA_GALLIUM_DRIVER=llvmpipe \
    WEGA_VNC_WAIT=50 \
    WEGA_UPLOAD_RETENTION_DAYS=7
WORKDIR /config
EXPOSE 7443
VOLUME ["/config"]
HEALTHCHECK --interval=30s --timeout=5s --start-period=45s --retries=3 \
    CMD /usr/local/bin/entrypoint.sh healthcheck
ENTRYPOINT ["/usr/bin/tini", "--", "/usr/local/bin/entrypoint.sh"]

LABEL org.opencontainers.image.title="Wega" \
      org.opencontainers.image.description="Native Rust browser powered by Servo, with a secure web viewer" \
      org.opencontainers.image.licenses="MPL-2.0" \
      org.opencontainers.image.source="https://github.com/wulfnb/wega"

# CI target: binaries compiled outside Docker by scripts/build-linux-binaries.sh
# in the same RUST_IMAGE, so the Cargo cache survives between workflow runs.
# Keep it before `runtime`, which stays the default (last) target.
FROM desktop AS runtime-prebuilt
COPY dist/docker/notices/ /usr/share/doc/wega/
COPY dist/docker/wega dist/docker/wega-engine /usr/local/bin/

FROM desktop AS runtime
COPY --from=builder /out/notices/ /usr/share/doc/wega/
COPY --from=builder /out/wega /out/wega-engine /usr/local/bin/
