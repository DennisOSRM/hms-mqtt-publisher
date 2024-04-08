# hms-mqtt-publisher

This tool fetches the current telemetry information from the HMS-XXXXW-2T series of micro-inverters and publishes the information into an MQTT broker. Please note that it doesn’t implement a DTU, but pulls the information off the internal DTU of these inverters. 

## How to run
The tool is distributed as source only — for now. You’ll have to download, compile and run it yourself. Please note that configuration of hosts, and passwords is done via `config.toml` from the current directory. It supports two different output channels. One is a simple MQTT publisher that doesn't follow a particular schema, and the other is made for [Home Assistant](https://www.home-assistant.io). It supports auto discovery of devices.

```
$ git clone https://github.com/DennisOSRM/hms-mqtt-publisher.git
$ cd hms-mqtt-publisher
$ cargo r
```
![image](https://github.com/lumapu/ahoy/assets/1067895/32c0b9b6-5aea-41e3-b9f8-161ce82fb99a)

### Docker

The latest release is directly deployable via a docker image from [DockerHub](https://hub.docker.com/r/dennisosrm/hms-mqtt-publisher). It is built automatically for the following Linux platforms: 
 - amd64,
 - arm/v7,
 - and arm64.

The parameters to access the inverter and MQTT instance are pulled from environment variables:
- `$INVERTER_HOST`
- `$MQTT_BROKER_HOST`
- `$MQTT_USERNAME` (optional)
- `$MQTT_PASSWORD` (optional)
- `$MQTT_PORT` (optional)

### Ansible (systemd)

You can use the [bellackn.homelab.hms-mqtt-publisher](https://github.com/bellackn/ansible-collection-homelab/blob/main/roles/hms_mqtt_publisher/README.md)
role to deploy hms-mqtt-publisher as a systemd service to a remote host. Check the role's documentation to see configuration options and setup instructions.

## Note of caution
Please note: The tool does not come with any guarantees and if by chance you fry your inverter with a funny series of bits, you are on your own. That being said, no inverters have been harmed during development. 

## Known limitations
- One can only fetch updates approximately twice per minute. The inverter firmware seems to implement a mandatory wait period of a little more than 30 seconds. If one makes a request within 30 seconds of the previous one, then the inverter will reply with the previous reading and restart the countdown. It will also not send updated values to S-Miles Cloud if this happens. 
- The tool is a CLI tool and not a background service. 
- The tools was developed for (and with an) HMS-800W-2T. It may work with the other inverters from the series, but is untested at the time of writing

