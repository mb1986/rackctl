# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added

- `rackctl check`: validates the rack file and every catalog model, then summarizes the rack:
  devices by kind, used and free units, and side strips. The rack file is taken from `-c`,
  `$RACKCTL_CONFIG` or `$XDG_CONFIG_HOME/rackctl/rack.kdl` (by default
  `~/.config/rackctl/rack.kdl`); user models are read from `catalog/` next to it.
- `rackctl catalog show <id>`: draws a model's face in a slice of rack, or a side strip over
  its span, with a sample status (`--state normal|off`) or each element's number
  (`--numbers`).
- `rackctl rack`: draws the rack's front view with a sample status: each device's face in
  its units, empty units, unit numbers, and side strips, with a rear strip mirrored.
- Wiring in `wiring.kdl` next to the rack file: power, network and management cables written
  as paths such as `net srv01:nic1 patch-32:b-f14 sw:11`. `rackctl check` reports unknown
  devices and endpoints, cables between the wrong kinds of endpoint, and endpoints used
  twice.
- Device faces: catalog models describe their front panel as a picture with a legend, with
  checked numbering of bays, PSUs, NICs, ports and outlets.
- Device catalog with selected models of Dell, Cisco, APC, HPE and Ubiquiti hardware, plus
  generic models. Models are read from the built-in catalog and the user's catalog, and each model
  file is loaded only when it is first used.
- Rack layout loading from `rack.kdl`: devices in the rack's unit slots and vertical strips
  beside the rack, on the front or the rear.
- Checks with precise error messages: unknown or misspelled nodes, properties and models,
  devices that do not fit in the rack, and overlapping devices. Two half-depth devices may
  share a unit on the front and the rear.
- Rack names, device ids and model file names use lowercase letters, digits and `-`, and
  do not start or end with `-`. Hidden files and links to directories in a catalog
  directory are ignored.

[Unreleased]: https://github.com/mb1986/rackctl/commits/main
