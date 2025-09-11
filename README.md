# hacamegui-rs

A GUI app written in Rust using egui for the Huawei EnVizion 360° camera. Uses the [hacam-lib-rs](https://github.com/timleg002/hacam-lib-rs) library.

## Features

Supports camera live view, recording (to MP4) and taking pictures (as JPG). The app also features an integrated spherical video renderer, which means you can preview the live video, recording or a picture in spherical 360° view. The GUI also supports recording to a MP4 file with integrated metadata injecting, which means the video plays in a spherical view if using VLC, for example.

## Platforms

Works on Windows (x86_64) & macOS (aarch64). Untested on Linux and x86_64 macOS, but should work also. Support for web is also planned.