# First, we need an image to build the application
FROM rust:slim-bullseye as builder

WORKDIR /usr/src/hms-mqtt-publish

# The following builds the rust application in three stages:
#
# 1. Build the dependencies
# 2. Build the protobuf files
# 3. Build the application
# 
# This way, each stage is cached and rebuilding the application is faster.


# Stage 1: Build the dependencies

COPY ./Cargo.toml ./
RUN mkdir src && \
    echo "fn main() {println!(\"hello from dependency build\")}" > src/main.rs && \
    cargo build --release


# Stage 2: Build the protobuf files

RUN mkdir src/protos
# Copy build.rs and Protobuf files
COPY ./build.rs ./
COPY ./src/protos/*.proto ./src/protos/

# Run the build script to generate code from Protobuf files
RUN echo "fn main() {println!(\"hello from protbuf build\")}" > src/main.rs && \
    cargo build --release


# Stage 3: Compile the application and install it

# This touch command is important to ensure that the build.rs is considered changed
RUN touch build.rs
# Now copy the actual source code and re-compile
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
