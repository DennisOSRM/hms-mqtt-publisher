# hms-mqtt-publisher

This tool fetches the current telemetry from Hoymiles HMS-XXXXW-xT micro-inverters with built-in WiFi (and DTUs speaking the same protocol) and publishes it to an MQTT broker. It doesn't implement a DTU, but pulls the information off the DTU of these inverters over the local network.

It supports two output channels: a simple MQTT publisher without a particular schema, and one for [Home Assistant](https://www.home-assistant.io) with MQTT auto discovery.

## What is published

- Total AC power, daily yield and total energy, efficiency
- Per PV port: voltage, current, power, daily yield, energy total and status code
- Per inverter: grid voltage and frequency, AC current, reactive power, power factor, temperature, power limit (once one has been set), warning count and signal strength; three-phase inverters with voltage and current per phase
- The DTU's warnings, fetched about every 5 minutes

Readings the DTU marks as stale are skipped, see [Known limitations](#known-limitations).

## Availability

Each output publishes `online` to a retained availability topic whenever it (re)connects to the broker, and registers `offline` as its MQTT last will, which the broker publishes once the connection is lost. Home Assistant shows the entities as unavailable then.

| Output         | Availability topic                                                                  |
|----------------|-------------------------------------------------------------------------------------|
| Home Assistant | `solar/hms_<device id>/availability`, without a device id `solar/<client id>/availability` |
| Simple MQTT    | `hms800wt2/availability`, with a device id `<device id>/availability`               |

The topic is fixed before connecting, so without a device id the Home Assistant output uses its MQTT client id instead of the DTU serial number. The client id is distinct per instance, see [Multiple inverters](#multiple-inverters).

## Power limit

The active power limit of the inverters can be set in percent (2 to 100) of their rated power: in Home Assistant with the "Power Limit" number entity, or by publishing the percentage to `solar/hms_<device id>/power_limit/set` (Home Assistant output) or `hms800wt2/power_limit/set` (simple MQTT output, or `<device id>/power_limit/set`). The command takes the place of the next reading, so it is applied within one update interval. The limit applies to all inverters of the DTU.

## Encrypted local traffic

Newer firmware that encrypts local traffic is detected automatically. The publisher obtains the encryption parameters from the inverter and decrypts telemetry locally; no Hoymiles cloud connection is required.

## How to run

Use the [Docker image](#docker), the [Home Assistant add-on](#home-assistant-add-on), or build it from source:

```
$ git clone https://github.com/DennisOSRM/hms-mqtt-publisher.git
$ cd hms-mqtt-publisher
$ cargo r
```
![image](https://github.com/lumapu/ahoy/assets/1067895/32c0b9b6-5aea-41e3-b9f8-161ce82fb99a)

### Home Assistant add-on

Add this repository to the add-on store of Home Assistant (Settings, Add-ons, Add-on store, Repositories) and install "Hoymiles HMS Wifi Addon". A nightly variant built from the main branch is available as well.

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

Querying the inverter can make it skip uploads to the S-Miles cloud: it uploads about once a minute and skips an upload when it was queried shortly before. Setting `update_interval = 60500` (or `UPDATE_INTERVAL=60500`), as the Home Assistant add-on does by default, lets most uploads through, but depending on the timing some can still be missed.

### Ansible (systemd)

You can use the [bellackn.homelab.hms-mqtt-publisher](https://github.com/bellackn/ansible-collection-homelab/blob/main/roles/hms_mqtt_publisher/README.md)
role to deploy hms-mqtt-publisher as a systemd service to a remote host. Check the role's documentation to see configuration options and setup instructions.

## Note of caution
Please note: The tool does not come with any guarantees and if by chance you fry your inverter with a funny series of bits, you are on your own. That being said, no inverters have been harmed during development. 

## Known limitations
- Fresh data is available about every 30 seconds. A request within about 30 seconds of the previous one gets the previous reading and restarts the DTU's countdown; after a few such requests the DTU stops reading the inverter for a while. The default interval of 30.5 s stays below that limit, readings marked as stale are skipped, and the publisher pauses for a minute or longer after a stale reading. Other clients polling the same inverter count against the same limit.
- Developed and tested with an HMS-800W-2T. Other HMS models, DTUs and three-phase inverters use the same protocol but are untested; values of three-phase inverters assume the scaling of single-phase ones.
- Encrypted telemetry uses the A311/RealDataNew protocol; legacy firmware continues to use A303/RealData.
