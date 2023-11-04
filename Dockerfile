# First, we need an image to build the application
FROM rust:slim-bullseye as builder

WORKDIR /usr/src/hms-mqtt-publish

# We only copy the files we need. Otherwise the git folders caused an error in the arm/v7 build .
COPY src src
COPY build.rs .
COPY Cargo.toml .

# Compile the application and install it
RUN cargo install --path .

# Then we use a small base image and copy the compiled application into this image. This way we get a small image without overheating the build environment. 
FROM debian:bullseye-slim

# Copy the installed application from the build image to the smaller image.
COPY --from=builder /usr/local/cargo/bin/hms-mqtt-publish /usr/local/bin/hms-mqtt-publish

# Run the application with given env variables
CMD hms-mqtt-publish $INVERTER_HOST $MQTT_BROKER_HOST $MQTT_USERNAME $MQTT_PASSWORD
