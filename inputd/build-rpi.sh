#!/bin/bash
echo "Building for Raspberry Pi (ARMv7 with musl)..."
echo ""
cargo build --release --target=arm-unknown-linux-musleabihf
