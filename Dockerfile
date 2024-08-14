ARG BOT_PROFILE=prod
ARG BOT_TARGET=prod

# Setup.
# FROM rust:slim AS rust
FROM rustlang/rust:nightly-bookworm-slim AS rust

# Update OS and setup deps.
RUN --mount=type=cache,target=/var/cache/apt,sharing=locked \
    apt update && apt upgrade -y


# Compile.
FROM rust AS builder

# Use sparse registry.
ARG CARGO_REGISTRIES_CRATES_IO_PROTOCOL=sparse
ARG BOT_PROFILE BOT_TARGET

# Create a new empty shell project.
RUN cargo new --bin app
WORKDIR /app

# Create a temporary lib.rs to match the Cargo.toml config.
ARG TEMPLIB=./src/lib/lib.rs
RUN mkdir -p $(dirname $TEMPLIB) && touch $TEMPLIB

# Copy manifests.
# COPY ./.cargo ./.cargo
COPY ./Cargo.lock ./Cargo.toml ./

# Build only the dependencies to cache them.
RUN --mount=type=cache,target=~/.cargo,sharing=locked \
    cargo build --profile=$BOT_PROFILE

# Remove default code from deps build.
RUN rm ./src/*.rs \
    && rm ./target/$BOT_TARGET/deps/riveting_bot* \
    && rm ./target/$BOT_TARGET/deps/libriveting_bot*

# Copy the source code.
COPY ./src ./src

# Build with profile.
RUN cargo build --profile=$BOT_PROFILE && strip --strip-all ./target/$BOT_TARGET/riveting-bot


# Final image.
FROM gcr.io/distroless/cc-debian12 AS final

ARG BOT_TARGET

# Copy the build artifact from the build stage.
COPY --from=builder /app/target/$BOT_TARGET/riveting-bot /app/riveting-bot

# Run as non-root.
# USER 1000:1000

# Set the startup command.
ENTRYPOINT ["/app/riveting-bot"]
