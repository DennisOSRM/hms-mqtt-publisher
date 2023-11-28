ARG BUILD_FROM

# Use pre-built image to copy the binary from
FROM rust:slim-bullseye as builder

WORKDIR /usr/src/hms-mqtt-publish

# The following builds the rust application in two stages:
#
# 1. Build the dependencies
# 2. Build the application
# 
# This way, each stage is cached and rebuilding the application is faster.


# Stage 1: Build the dependencies

COPY ./Cargo.toml ./
RUN mkdir src && \
    echo "fn main() {println!(\"hello from dependency build\")}" > src/main.rs && \
    cargo build --release


# Stage 2: Build the protobuf files

COPY ./build.rs ./
COPY ./src ./src
RUN cargo install --path .

# Copy the installed application from the build image to the smaller image.
RUN cp /usr/local/cargo/bin/hms-mqtt-publish /usr/local/bin/hms-mqtt-publish

FROM $BUILD_FROM

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/local/bin/hms-mqtt-publish /usr/local/bin/hms-mqtt-publish

# Copy the run script into the container
COPY run.sh /run.sh
RUN chmod a+x /run.sh

# When the container starts, run the script
CMD [ "/run.sh" ]
