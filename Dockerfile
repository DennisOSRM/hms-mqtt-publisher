# First, we need an image to build the application
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

# Then we use a small base image and copy the compiled application into this image. This way we get a small image without overheating the build environment. 
FROM debian:bullseye-slim

# Copy the installed application from the build image to the smaller image.
COPY --from=builder /usr/local/cargo/bin/hms-mqtt-publish /usr/local/bin/hms-mqtt-publish

# Run the application with given env variables
CMD hms-mqtt-publish $INVERTER_HOST $MQTT_BROKER_HOST $MQTT_USERNAME $MQTT_PASSWORD $MQTT_PORT
