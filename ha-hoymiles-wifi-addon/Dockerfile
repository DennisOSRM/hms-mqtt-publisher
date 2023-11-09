ARG BUILD_FROM

# Use pre-built image to copy the binary from
FROM dennisosrm/hms-mqtt-publisher:v0.1 AS builder

FROM $BUILD_FROM

# Copy the compiled binary from the builder stage
COPY --from=builder /usr/local/bin/hms-mqtt-publish /usr/local/bin/hms-mqtt-publish

# Copy the run script into the container
COPY run.sh /run.sh
RUN chmod a+x /run.sh

# When the container starts, run the script
CMD [ "/run.sh" ]
