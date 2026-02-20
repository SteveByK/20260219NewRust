FROM rust:1.85-bookworm AS builder
WORKDIR /app

RUN cargo install cargo-leptos --locked
COPY . .
RUN rustup target add wasm32-unknown-unknown
RUN cargo leptos build --release

FROM gcr.io/distroless/cc-debian12
WORKDIR /app
COPY --from=builder /app/target/release/platform-server /app/platform-server
COPY --from=builder /app/target/site /app/site
EXPOSE 3000
CMD ["/app/platform-server"]
