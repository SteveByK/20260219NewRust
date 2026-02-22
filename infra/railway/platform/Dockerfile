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

RUN rustup target add wasm32-unknown-unknown

FROM toolchain AS builder
COPY . .
RUN rm -f rust-toolchain.toml
RUN set -eux; \
	for i in 1 2 3 4 5; do \
		cargo leptos build --release && break; \
		echo "cargo leptos build failed (attempt ${i}), retrying..."; \
		sleep $((i * 10)); \
		if [ "$i" -eq 5 ]; then exit 1; fi; \
	done

FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY --from=builder /app/target/release/platform-server /app/platform-server
COPY --from=builder /app/apps/platform/target/site /app/site
EXPOSE 3000
CMD ["/app/platform-server"]
