

<img src="https://github.com/cristianvogel/elemaudio-resources/blob/main/src/resource-server-demo.jpg" width="400" />


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

Resource ids are derived from the source filename, mono inputs stay mono, and multichannel inputs remain multichannel so the browser demo can use `el.sample(...)` or `el.mc.sample(...)` as appropriate.

The HTTP API also exposes resource metadata by id, currently `duration_ms` and `channels`, so demos can show buffer stats without downloading the audio again.

Metadata requests are currently read-only and keyed by the derived resource id.

Browser uploads confirm before overwriting an existing derived resource id.

## Run

```bash
cargo run --bin resource-manager-server
cargo run --bin resource-vfs-demo
```
