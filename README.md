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

The container is configured with environment variables (see [Configuration](#configuration)), e.g.

```
docker run -e INVERTER_HOST=192.168.4.182 -e MQTT_BROKER_HOST=192.168.1.10 dennisosrm/hms-mqtt-publisher
```

Alternatively, mount a `config.toml` to `/config/config.toml`.

### Configuration

Settings are read from `config.toml` in the current directory (or next to the executable) and can be set or overridden by environment variables, for every kind of deployment:

| Variable           | `config.toml` setting                 | Notes                                             |
|--------------------|---------------------------------------|---------------------------------------------------|
| `INVERTER_HOST`    | `inverter_host`                       | required                                          |
| `UPDATE_INTERVAL`  | `update_interval`                     | milliseconds, minimum and default 30500           |
| `MQTT_BROKER_HOST` | `host` of the MQTT outputs            | required unless set in `config.toml`              |
| `MQTT_PORT`        | `port`                                | optional, default 1883 (8883 with TLS)            |
| `MQTT_USERNAME`    | `username`                            | optional                                          |
| `MQTT_PASSWORD`    | `password`                            | optional                                          |
| `MQTT_TLS`         | `tls`                                 | optional, `true` or `false`                       |
| `MQTT_CLIENT_ID`   | `client_id`                           | optional, used as is; with both outputs enabled `-ha` and `-sm` are appended. Default derived from the device id or inverter host |
| `DEVICE_ID`        | `device_id`                           | optional, see [Multiple inverters](#multiple-inverters) |

Environment variables take precedence over `config.toml`. The `MQTT_*` variables apply to the MQTT outputs configured in `config.toml` (`[home_assistant]`, `[simple_mqtt]`); without any, they enable both. Empty variables count as not set.

### Multiple inverters

Run one instance per inverter and give each a distinct `DEVICE_ID` (or `device_id` in `config.toml`), for example the last digits of its serial number. It may contain letters, digits, `_` and `-`. The device id names the inverter in the Home Assistant entities (`hms_<device id>`) and replaces the `hms800wt2` prefix of the simple MQTT topics. Without it, Home Assistant uses the first 8 characters of the DTU serial number, which are the model and production week and can be the same for several inverters. Leave it unset for a single inverter to keep the existing entity ids.

Each instance connects with its own MQTT client id, derived from the device id or the inverter host, so the instances don't disconnect each other.

### S-Miles cloud

The inverter only uploads to the S-Miles cloud when it is not queried more often than about once a minute. To keep the cloud up to date, set `update_interval = 60500` (or `UPDATE_INTERVAL=60500`), as the Home Assistant add-on does by default.

### Ansible (systemd)

You can use the [bellackn.homelab.hms-mqtt-publisher](https://github.com/bellackn/ansible-collection-homelab/blob/main/roles/hms_mqtt_publisher/README.md)
role to deploy hms-mqtt-publisher as a systemd service to a remote host. Check the role's documentation to see configuration options and setup instructions.

## Note of caution
Please note: The tool does not come with any guarantees and if by chance you fry your inverter with a funny series of bits, you are on your own. That being said, no inverters have been harmed during development. 

## Known limitations
- One can only fetch updates approximately twice per minute. The inverter firmware seems to implement a mandatory wait period of a little more than 30 seconds. If one makes a request within 30 seconds of the previous one, then the inverter will reply with the previous reading and restart the countdown. It will also not send updated values to S-Miles Cloud if this happens. 
- The tool is a CLI tool and not a background service. 
- The tools was developed for (and with an) HMS-800W-2T. It may work with the other inverters from the series, but is untested at the time of writing

