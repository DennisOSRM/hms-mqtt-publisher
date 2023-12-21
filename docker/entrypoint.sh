#!/bin/sh

# Generate the config file from given environment variables

cat << EOF > config.toml

inverter_host = "$INVERTER_HOST" 

[home_assistant]
host = "$MQTT_BROKER_HOST"
username = "$MQTT_USERNAME"
password = "$MQTT_PASSWORD"
port = $MQTT_PORT

EOF

# start mqtt publisher
hms-mqtt-publish