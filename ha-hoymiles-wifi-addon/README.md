# Home Assistant Add-on

This tool can directly run inside Home Assistant OS as an addon. To get started, add the URL of this repository to the add-on store in Home Assistant and install the add-on. Alternatively, you can also run it stand-alone on any machine that supports rust or docker (see section below).

## Configuration

To set up the add-on, fill in the following configuration parameters:

- `inverter_host`: The hostname or IP address of your inverter.
- `mqtt_broker_host`: The hostname or IP address of your MQTT broker.
- `mqtt_username`: The username for your MQTT broker.
- `mqtt_password`: The password for your MQTT broker. Keep this secret!
- `mqtt_port`: The port of your MQTT broker (default is 1883 for unencrypted MQTT).

## Example configuration

```yaml
inverter_host: "192.168.1.100"
mqtt_broker_host: "core-mosquitto"
mqtt_username: "yourusername"
mqtt_password: "yourpassword"
mqtt_port: 1883
```

## Note of caution
Please note: The tool does not come with any guarantees and if by chance you fry your inverter with a funny series of bits, you are on your own. That being said, no inverters have been harmed during development. 

## Known limitations
- One can only fetch updates approximately twice per minute. The inverter firmware seems to implement a mandatory wait period of a little more than 30 seconds. If one makes a request within 30 seconds of the previous one, then the inverter will reply with the previous reading and restart the countdown. It will also not send updated values to S-Miles Cloud if this happens. 
- The tools was developed for (and with an) HMS-800W-T2. It may work with the other inverters from the series, but is untested at the time of writing

