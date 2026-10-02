# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.4.0](https://github.com/DennisOSRM/hms-mqtt-publisher/releases/tag/v0.4.0) - 2026-10-02

### Fixed

- correct link to docker hub in README.md ([#67](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/67))

### Other

- add release-plz for one-click releases ([#144](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/144))
- Keep dependencies up to date with Dependabot ([#142](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/142))
- Fix RealData field mapping, skip stale readings and harden reply parsing ([#141](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/141))
- Update dependencies ([#136](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/136))
- Fix/docker read environment vars ([#70](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/70))
- Update dependencies ([#132](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/132))
- Update ha addons ([#121](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/121))
- Add hint to Ansible role/systemd deployment ([#118](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/118))
- Fix compilation ([#103](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/103))
- Update rumqttc requirement from 0.23.0 to 0.24.0 ([#102](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/102))
- Bump dependencies ([#99](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/99))
- Implement initial TLS support ([#93](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/93))
- Fix oder of magnitude reported for current in HA plugin ([#86](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/86))
- Add Rust action to check formatting ([#87](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/87))
- Update env_logger requirement from 0.10.1 to 0.11.0 ([#84](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/84))
- Create dependabot.yml ([#83](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/83))
- Create rust-clippy.yml ([#82](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/82))
- Create clippy formatting workflow for PRs ([#80](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/80))
- Implement an optional cooperative mode that will also update S-Miles Cloud ([#78](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/78))
- Reorder workspace to allow easy addition of further executables ([#79](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/79))
- Update dependencies ([#77](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/77))
- Load configuration from current working dir, or relative to executable ([#76](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/76))
- [MINOR] Upgrade dependencies ([#71](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/71))
- [MINOR] Fix typo in println(.) statement ([#72](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/72))
- Update config in Dockerfile ([#68](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/68))
- Update repository.json with correct repo url ([#65](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/65))
- Update base image home assistant addon Dockerfile ([#66](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/66))
- Fix order of magnitude for pv_port{1,2}_daily_yield ([#64](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/64))
- Improve error handling, Nightly build ([#44](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/44))
- Fix order of magnitude for pv_port{1,2}_energy ([#63](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/63))
- Downgrade error to warning ([#62](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/62))
- Support connecting to inverter by hostname ([#60](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/60))
- Unique client ids ([#59](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/59))
- Don't filter logging output ([#58](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/58))
- Implement read/write timeout ([#57](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/57))
- Set qos ([#54](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/54))
- Add initial integration test
- Print source code revision on startup ([#49](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/49))
- [SimpleMQTT] Publish time sent from inverter in human-readable form ([#47](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/47))
- Hide internal interfaces ([#48](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/48))
- Convert to workspace project ([#46](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/46))
- Hide actual MQTT implementation behind trait+newtype ([#42](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/42))
- Minor code adjustments
- Drop dependency on clap crate ([#43](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/43))
- Add lib.rs to bundle re-usable parts of the implementation ([#41](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/41))
- Rename mqtt*.rs to home_assistant*.rs ([#40](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/40))
- Prepare v0.2.0 release ([#39](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/39))
- Implement output channels ([#34](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/34))
- Feature/ha discovery ([#27](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/27))
- Create pull_request_template.md ([#35](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/35))
- Staged docker builds ([#32](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/32))
- Improved dev environment ([#31](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/31))
- Upgrade dependencies ([#23](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/23))
- Home Assistant Add-on  ([#20](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/20))
- Update README.md ([#19](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/19))
- Update README.md with information on Docker images ([#18](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/18))
- Remove unused dependency ([#16](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/16))
- Auto publish docker image to docker hub ([#13](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/13))
- Make MQTT Port configurable
- Refactor main.rs into a set of structs ([#10](https://github.com/DennisOSRM/hms-mqtt-publisher/pull/10))
- Update README.md
- Fix tool and project name
- Make tool less chatty in case inverter is unreachable
- Create rust.yml
- Update README.md
- initial commit
- Initial commit

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
