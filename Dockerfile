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
# Remove the hms2mqtt dependency from the Cargo.toml file (compiled in second stage)
RUN sed -i s/hms2mqtt.*//g Cargo.toml 
RUN mkdir src && \
    echo "fn main() {println!(\"hello from dependency build\")}" > src/main.rs && \
    cargo build --release


# Stage 2: Build the application
COPY ./Cargo.toml ./
COPY ./hms2mqtt ./hms2mqtt
COPY ./src ./src
RUN cargo install --path .

# Copy the installed application from the build image to the smaller image.
RUN cp /usr/local/cargo/bin/hms-mqtt-publish /usr/local/bin/hms-mqtt-publish

# Then we use a small base image and copy the compiled application into this image. This way we get a small image without overheating the build environment. 
FROM debian:bullseye-slim

# Copy the installed application from the build image to the smaller image.
COPY --from=builder /usr/local/cargo/bin/hms-mqtt-publish /usr/local/bin/hms-mqtt-publish

# Generate the config file from given environment variables
RUN echo "inverter_host = \"$INVERTER_HOST\" \n\n[home_assistent] \nhost = \"$MQTT_BROKER_HOST\"\nusername = \"$MQTT_USERNAME\"\npassword = \"$MQTT_PASSWORD\"\nport = $MQTT_PORT\n" > config.toml

# Run the application
CMD hms-mqtt-publish
