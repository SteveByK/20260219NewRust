## syntax=docker/dockerfile:1.7

FROM rust:1.91-bookworm AS toolchain
WORKDIR /app

RUN apt-get update \
	&& apt-get install -y --no-install-recommends pkg-config libasound2-dev ca-certificates curl binaryen \
	&& rm -rf /var/lib/apt/lists/*

ARG TAILWIND_VERSION=v4.1.10
ARG CARGO_LEPTOS_VERSION=0.3.4

RUN set -eux; \
	arch="$(dpkg --print-architecture)"; \
	case "$arch" in \
		amd64) tw_arch="x64" ;; \
		arm64) tw_arch="arm64" ;; \
		*) echo "unsupported architecture: $arch"; exit 1 ;; \
	esac; \
	url="https://github.com/tailwindlabs/tailwindcss/releases/download/${TAILWIND_VERSION}/tailwindcss-linux-${tw_arch}"; \
	fallback_url="https://github.com/tailwindlabs/tailwindcss/releases/latest/download/tailwindcss-linux-${tw_arch}"; \
	(curl -fL --retry 5 --retry-delay 2 --retry-all-errors "$url" -o /usr/local/bin/tailwindcss \
		|| curl -fL --retry 5 --retry-delay 2 --retry-all-errors "$fallback_url" -o /usr/local/bin/tailwindcss); \
	chmod +x /usr/local/bin/tailwindcss

ENV CARGO_BUILD_JOBS=2 \
	CARGO_NET_RETRY=5 \
	CARGO_HTTP_TIMEOUT=600 \
	LEPTOS_TAILWIND_VERSION=v4.1.10 \
	CARGO_PROFILE_RELEASE_LTO=thin \
	CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16

RUN set -eux; \
	for i in 1 2 3 4 5; do \
		cargo install cargo-leptos --locked --version ${CARGO_LEPTOS_VERSION} && break; \
		echo "tool install failed (attempt ${i}), retrying..."; \
		sleep $((i * 5)); \
		if [ "$i" -eq 5 ]; then exit 1; fi; \
	done

RUN set -eux; \
	for i in 1 2 3 4 5; do \
		cargo install wasm-bindgen-cli --locked --version 0.2.100 && break; \
		echo "wasm-bindgen install failed (attempt ${i}), retrying..."; \
		sleep $((i * 5)); \
		if [ "$i" -eq 5 ]; then exit 1; fi; \
	done

RUN rustup target add wasm32-unknown-unknown

FROM toolchain AS builder
COPY . .
RUN rm -f rust-toolchain.toml
RUN set -eux; \
	for i in 1 2 3 4 5; do \
		cd /app/apps/platform; \
		cargo leptos build --release && break; \
		echo "cargo leptos build failed (attempt ${i}), retrying..."; \
		sleep $((i * 10)); \
		if [ "$i" -eq 5 ]; then exit 1; fi; \
	done
RUN set -eux; \
	if [ -f /app/target/release/platform-server ]; then \
		cp /app/target/release/platform-server /app/platform-server; \
	elif [ -f /app/target/release/platform ]; then \
		cp /app/target/release/platform /app/platform-server; \
	else \
		echo "server binary not found under /app/target/release"; \
		ls -la /app/target/release || true; \
		exit 1; \
	fi
RUN set -eux; \
	if [ -f /app/apps/platform/target/site/index.html ]; then \
		cp -a /app/apps/platform/target/site /app/site; \
	elif [ -f /app/target/site/index.html ]; then \
		cp -a /app/target/site /app/site; \
	else \
		echo "site index not found in expected paths; generating site via hydrate wasm fallback"; \
		find /app -maxdepth 6 -name index.html -print || true; \
		cd /app/apps/platform; \
		tailwindcss -i ./style/tailwind.css -o ./style/output.css --minify; \
		cd /app; \
		cargo build -p platform --release --no-default-features --features hydrate --target wasm32-unknown-unknown; \
		mkdir -p /app/site/pkg /app/site/style; \
		wasm-bindgen --target web --out-dir /app/site/pkg /app/target/wasm32-unknown-unknown/release/platform.wasm; \
		cp /app/apps/platform/style/output.css /app/site/style/output.css; \
		printf '%s\n' '<!doctype html>' '<html lang="en">' '<head>' '<meta charset="utf-8">' '<meta name="viewport" content="width=device-width, initial-scale=1">' '<title>Social Map Platform</title>' '<link rel="stylesheet" href="/style/output.css">' '</head>' '<body>' '<script type="module">' 'import init, { hydrate } from "/pkg/platform.js";' 'await init();' 'hydrate();' '</script>' '</body>' '</html>' > /app/site/index.html; \
	fi

RUN set -eux; \
	test -f /app/site/index.html; \
	test -f /app/site/style/output.css; \
	test -f /app/site/pkg/platform.js; \
	test -f /app/site/pkg/platform_bg.wasm

FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY --from=builder /app/platform-server /app/platform-server
COPY --from=builder /app/site /app/site
EXPOSE 3000
CMD ["/app/platform-server"]
