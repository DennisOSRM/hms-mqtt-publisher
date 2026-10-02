# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- TLS support for MQTT connections ([#93](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/93))
- Publish AC current, reactive power, power factor, power limit, warning count, signal strength and per-port status codes ([#141](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/141))
- Hint on the Ansible role / systemd deployment ([#118](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/118))

### Fixed

- Correct field mapping of the inverter reply; skip stale readings when the DTU reports no inverter link ([#141](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/141))
- No more panics on short or malformed replies, single-port inverters or short serial numbers; replies are validated (length, command, sequence number, CRC) ([#141](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/141))
- Docker image reads its configuration from environment variables ([#70](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/70))
- Compilation fix ([#103](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/103))

### Changed

- Updated dependencies ([#99](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/99), [#102](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/102), [#132](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/132), [#136](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/136)) and Home Assistant add-ons ([#121](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/121))
- Dependabot keeps crates, GitHub Actions and Docker base images up to date ([#142](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/142))

## [0.4] - 2024-01-27

Coop mode and improved logging. See the [GitHub release](https://github.com/DennisOSRM/hms-mqtt-publisher/releases/tag/v0.4).
