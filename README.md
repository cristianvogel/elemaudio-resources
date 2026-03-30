# elemaudio-resources
## [elementary-rs](https://github.com/cristianvogel/elemaudio-rs) 
## [elementary.audio](https://www.elementary.audio/)

Sibling repo for resource ownership, resource-backed playback, and VFS mirror demos.

## What lives here

- Resource manager-facing demos
- Native resource playback demo
- Browser mirror demo for `el.sample(...)`

## Current split state

This repo currently reuses the shared resource-manager source from `elemaudio-rs` via wrapper binaries.
That lets us split the package boundary first, then move code ownership over incrementally.
The resource layer is an optional extension to Elementary's vendor VFS model, not a replacement for the original lookup path.

## Run

```bash
cargo run --bin resource-manager-server
cargo run --bin resource-vfs-demo
```
