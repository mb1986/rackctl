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
rackctl check              # check the configuration in ~/.config/rackctl and summarize each rack
rackctl check -c lab/      # check the configuration in another directory
rackctl check -c lab.kdl   # check a configuration kept in a single file
rackctl rack               # draw the front view of each rack
rackctl rack homelab       # draw one rack
rackctl trace server:nic1  # follow the cables from an endpoint
```

`check` exits with code 2 when the configuration has problems, and reports each of them
with its location in the file.

## Configuration

The configuration is written in [KDL](https://kdl.dev). rackctl reads every `.kdl` file in
`~/.config/rackctl/`, subdirectories included, and each top-level node says what it holds,
so the configuration can be split into files in any way, or kept in one. A `rack` node
describes a rack, and there can be several:

```kdl
rack "homelab" units=36 {
  device "router"   model="ubiquiti/er6p"     u=36
  device "patch"    model="generic/patchpanel-24" u=33
  device "switch"   model="cisco/sg350-28"    u=31
  device "server"   model="dell/r730-sff8"    u=28
  device "ups"      model="apc/sua2200rmi2u"  u=1
  device "pdu"      model="apc/ap7952"        mount="right" face="rear"
}
```

Each `model` refers to the device catalog in [`catalog/`](./catalog/), which describes the
hardware: its height, depth, parts and faceplate. Models in `~/.config/rackctl/catalog/`
add to the built-in ones or replace them.

The cables go in a `wiring` node. Each line is one path, and a patch panel port names its
side: `b` (back) or `f` (front), or both when a path passes through it.

```kdl
wiring {
  power pdu:8 server:psu1
  net   server:nic1 patch:b-f15 switch:10
}
```

With several racks, a device is written with its rack, as `office.switch:28`. Its name alone
is enough when only one rack has a device of that name, or inside a wiring that names its
rack:

```kdl
wiring rack="homelab" {
  power pdu:7 server:psu2
  net   switch:28 office.switch:28
}
```

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
