# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.5.1](https://github.com/DennisOSRM/hms-mqtt-publisher/compare/v0.5.0...v0.5.1) - 2026-10-08

### Added

- configure performance mode and startup power limit ([#167](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/167))
- support DTUs with encrypted local traffic ([#165](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/165))
- publish MQTT availability with a last will ([#164](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/164))
- set the power limit from Home Assistant and MQTT ([#163](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/163))
- back off after stale readings ([#160](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/160))
- publish total energy and DTU warnings ([#158](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/158))
- publish three-phase inverters ([#156](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/156))
- request real-time data like the vendor app ([#155](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/155))

### Fixed

- don't publish a power limit of 0 % before one is set ([#161](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/161))
- *(addon)* use this repository's nightly image in the nightly add-on ([#159](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/159))
- *(ci)* lowercase nightly image names and skip release branches ([#152](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/152))

### Other

- remove cruft, fix outdated comments and extend test coverage ([#168](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/168))
- cover the polling loop ([#166](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/166))
- refresh the README ([#162](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/162))
- limit releases to code changes and pin the stable add-on to v0.5.0 ([#154](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/154))

## [0.5.0](https://github.com/DennisOSRM/hms-mqtt-publisher/compare/v0.4...v0.5.0) - 2026-10-04

### Added

- Support multiple inverters on one broker: an optional device id names each inverter in topics and Home Assistant entities, and every instance gets its own MQTT client id ([#150](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/150))
- Configure the publisher from environment variables for every deployment, merged with an optional `config.toml` ([#149](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/149))
- Publish AC current, reactive power, power factor, power limit, warning count, signal strength and per-port status codes ([#141](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/141))
- TLS support for MQTT connections ([#93](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/93))
- Hint on the Ansible role / systemd deployment ([#118](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/118))

### Fixed

- Correct field mapping of the inverter reply; skip stale readings when the DTU reports no inverter link ([#141](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/141))
- No more panics on short or malformed replies, single-port inverters or short serial numbers; replies are validated (length, command, sequence number, CRC) ([#141](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/141))
- Invalid configuration from unset Docker variables, empty MQTT credentials and slow container shutdown ([#149](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/149))
- Docker image reads its configuration from environment variables ([#70](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/70))
- Compilation fix ([#103](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/103))

### Changed

- The default MQTT client ids are now `hms-mqtt-publish-<device id or inverter host>-ha` / `-sm` instead of `hms800wt2-mqtt-publisher-ha` / `-sm`; this only matters for brokers with client id based ACLs ([#150](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/150))
- The Docker image starts the publisher directly; a `config.toml` can be mounted at `/config/config.toml` ([#149](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/149))
- Updated dependencies, including toml 1.1 and rumqttc 0.25 ([#99](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/99), [#102](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/102), [#132](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/132), [#136](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/136), [#145](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/145), [#146](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/146)), and the Home Assistant add-ons ([#121](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/121))
- Docker images are built on native ARM runners ([#143](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/143)); Dependabot keeps crates, GitHub Actions and Docker base images up to date ([#142](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/142), [#148](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/148))

## [0.4] - 2024-01-27

Coop mode and improved logging. See the [GitHub release](https://github.com/DennisOSRM/hms-mqtt-publisher/releases/tag/v0.4).
