# elemaudio-resources

## [elementary-rs](https://github.com/cristianvogel/elemaudio-rs)
## [elementary.audio](https://www.elementary.audio/)

Sibling repo for resource ownership, resource-backed playback, and VFS mirror demos.

## What lives here

- Resource manager-facing demos
- Native resource playback demo
- Browser mirror demo for `el.sample(...)`

## Current split state

This repo owns the optional resource layer and its demos.
It is a separate package from `elemaudio-rs`, and it is an optional extension to Elementary's vendor VFS model, not a replacement for the original lookup path.

## Run

```bash
cargo run --bin resource-manager-server
cargo run --bin resource-vfs-demo
```
