# syntax=docker/dockerfile:1
FROM rust:1.87-slim as builder

RUN apt-get update && apt-get install -y pkg-config libssl-dev && rm -rf /var/lib/apt/lists/*

WORKDIR /app
COPY Cargo.toml Cargo.lock ./

# Create dummy main to build dependencies
RUN mkdir src && echo "fn main() {}" > src/main.rs

# Build dependencies (cached layer)
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && rm src/main.rs

COPY src ./src/
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/app/target \
    cargo build --release && \
    cp target/release/nimbus /app/bootstrap

# Runtime stage
FROM public.ecr.aws/lambda/provided:al2023

RUN dnf install -y unzip && dnf clean all && \
    curl -fsSL https://bun.sh/install | bash && \
    mv /root/.bun/bin/bun /usr/local/bin/bun

# Copy the bootstrap binary to the location Lambda expects
COPY --from=builder /app/bootstrap /var/runtime/bootstrap
RUN chmod +x /var/runtime/bootstrap

# Set working directory and copy templates
WORKDIR /var/task
COPY templates ./templates/
RUN cd templates && bun install --frozen-lockfile

# Lambda will automatically run /var/runtime/bootstrap
CMD ["/var/runtime/bootstrap"]