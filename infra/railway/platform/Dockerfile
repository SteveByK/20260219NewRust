## syntax=docker/dockerfile:1.7

FROM rust:1.91-bookworm AS toolchain
WORKDIR /app

ENV CARGO_BUILD_JOBS=2 \
	CARGO_NET_RETRY=5 \
	CARGO_HTTP_TIMEOUT=600 \
	LEPTOS_TAILWIND_VERSION=4.1.10 \
	LEPTOS_WASM_OPT_VERSION=version_123 \
	CARGO_PROFILE_RELEASE_LTO=thin \
	CARGO_PROFILE_RELEASE_CODEGEN_UNITS=16

RUN set -eux; \
	for i in 1 2 3 4 5; do \
		cargo install cargo-chef --locked && cargo install cargo-leptos --locked && break; \
		echo "tool install failed (attempt ${i}), retrying..."; \
		sleep $((i * 5)); \
		if [ "$i" -eq 5 ]; then exit 1; fi; \
	done

RUN rustup target add wasm32-unknown-unknown

FROM toolchain AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM toolchain AS cacher
COPY --from=planner /app/recipe.json /app/recipe.json
RUN cargo chef cook --release --workspace --recipe-path recipe.json --locked

FROM toolchain AS builder
COPY --from=cacher /app/target /app/target
COPY . .
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
COPY --from=builder /app/target/site /app/site
EXPOSE 3000
CMD ["/app/platform-server"]
