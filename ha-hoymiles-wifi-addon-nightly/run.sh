#!/usr/bin/with-contenv bashio

# Enable strict mode for bash (exit on error, error on undefined variable, error if any pipeline element fails)
set -euo pipefail
# Fetch values from the add-on configuration by extracting it from /data/options.json

HA_MQTT_BROKER_HOST=$(bashio::services mqtt "host")
HA_MQTT_USERNAME=$(bashio::services mqtt "username")
HA_MQTT_PASSWORD=$(bashio::services mqtt "password")

INVERTER_HOST=$(bashio::config 'inverter_host')
MQTT_BROKER_HOST=$(bashio::config 'mqtt_broker_host')
MQTT_USERNAME=$(bashio::config 'mqtt_username')
MQTT_PASSWORD=$(bashio::config 'mqtt_password')
MQTT_PORT=$(bashio::config 'mqtt_port')
UPDATE_INTERVAL=$(bashio::config 'update_interval')
DEBUG_LOGGING=$(bashio::config 'debug_logging')

# Use bashio::config values if they are defined, otherwise fall back to bashio::services values
MQTT_BROKER_HOST=${MQTT_BROKER_HOST:-$HA_MQTT_BROKER_HOST}
MQTT_USERNAME=${MQTT_USERNAME:-$HA_MQTT_USERNAME}
MQTT_PASSWORD=${MQTT_PASSWORD:-$HA_MQTT_PASSWORD}
DEBUG_LOGGING=${DEBUG_LOGGING:-'false'}


# Check if the required configs are provided
if [[ -z "$INVERTER_HOST" ]]; then
    echo "The inverter_host is not configured."
    exit 1
fi

if [[ -z "$MQTT_BROKER_HOST" ]]; then
    echo "The mqtt_broker_host is not configured."
    exit 1
fi

# print a warning if update interval is < 60500
if [[ "$UPDATE_INTERVAL" -lt 60500 ]]; then
    echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
    echo "The update_interval is set to a value lower than 60500."
    echo "This is not recommended and may cause the inverter to become unresponsive."
    echo "!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!!"
fi

# Create the configuration file
cat <<EOF > ./config.toml
inverter_host = "$INVERTER_HOST"
update_interval = $UPDATE_INTERVAL

[home_assistant]
host = "$MQTT_BROKER_HOST"
username = "$MQTT_USERNAME"
password = "$MQTT_PASSWORD"
port = $MQTT_PORT
EOF

# if DEBUG_LOGGING is true, set the RUST_LOG=debug environment variable to enable debug logging
if [[ "$DEBUG_LOGGING" == "true" ]]; then
    export RUST_LOG=debug
fi

# Execute the application
/usr/local/bin/hms-mqtt-publish
