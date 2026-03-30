# mousemux

`mousemux` is a Linux daemon that grabs one physical mouse, remaps selected mouse button events to a virtual keyboard, and forwards all other mouse movement, wheel, and button events to a virtual mouse.

## Features

- Single-device mouse capture with `grab`
- YAML-based remap rules
- Separate virtual mouse and virtual keyboard outputs
- Hot reload with rollback on invalid config
- Foreground operation for systemd-managed deployments

## Usage

```bash
cargo run -- --config ./config.example.yaml
```

Validate a config without starting the daemon:

```bash
cargo run -- --check-config --config ./config.example.yaml
```

## Requirements

- Linux with `evdev` and `uinput`
- Root privileges
- A systemd-based environment for service deployment

## Service

An example unit file is available at `mousemux.service`.
