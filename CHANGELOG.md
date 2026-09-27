# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- Device catalog with selected models of Dell, Cisco, APC, HPE and Ubiquiti hardware, plus
  generic models. Models are read from the built-in catalog and the user's catalog, and each model
  file is loaded only when it is first used.
- Rack layout loading from `rack.kdl`: devices in the rack's unit slots and vertical strips
  beside the rack, on the front or the rear.
- Checks with precise error messages: unknown or misspelled nodes, properties and models,
  devices that do not fit in the rack, and overlapping devices. Two half-depth devices may
  share a unit on the front and the rear.

[Unreleased]: https://github.com/mb1986/rackctl/commits/main
