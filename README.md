# rackctl

> Monitor and control rack devices from the command line or a TUI.

[![CI](https://img.shields.io/github/actions/workflow/status/mb1986/rackctl/ci.yml?branch=main&style=for-the-badge&label=CI)](https://github.com/mb1986/rackctl/actions/workflows/ci.yml)

## What it does

rackctl manages the devices in a server rack: servers, disk shelves, switches, PDUs and UPSs.
Run without arguments, it opens a full-screen terminal UI with the rack drawn on the left,
every device shown with its own faceplate and live status LEDs, and panels with details and
controls on the right. With arguments, it works as a command-line tool, for example to switch
a PDU outlet.

## Status

Early development. `rackctl check` validates the configuration and the catalog. Device
control and the terminal UI are not available yet.

## Usage

```sh
rackctl check              # check ~/.config/rackctl/rack.kdl and summarize the rack
rackctl check -c rack.kdl  # check another rack file
rackctl rack               # draw the rack's front view
```

`check` exits with code 2 when the configuration has problems, and reports each of them
with its location in the file.

## Configuration

The rack is described in `~/.config/rackctl/rack.kdl`, written in [KDL](https://kdl.dev):

```kdl
rack "homelab" units=36 {
  device "router"   model="ubiquiti/er6p"     u=36
  device "switch"   model="cisco/sg350-28"    u=31
  device "server"   model="dell/r730-sff8"    u=28
  device "ups"      model="apc/sua2200rmi2u"  u=1
  device "pdu"      model="apc/ap7952"        mount="right" face="rear"
}
```

Each `model` refers to the device catalog in [`catalog/`](./catalog/), which describes the
hardware: its height, depth, parts and faceplate. Models in `~/.config/rackctl/catalog/`
add to the built-in ones or replace them.

## Building

rackctl needs Rust 1.98.1 or newer.

```sh
cargo build --release
cargo test
```

## Versioning

This project follows [Semantic Versioning](https://semver.org).
See [CHANGELOG.md](./CHANGELOG.md) for the history of changes.

## License

Licensed under either of [Apache License, Version 2.0](./LICENSE-APACHE) or
[MIT license](./LICENSE-MIT) at your option.
