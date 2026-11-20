set shell := ["bash", "-euo", "pipefail", "-c"]

# f3d flags verified against f3d 3.5.0 on Fedora. No --rendering-backend flag is needed:
# the default backend renders headlessly (works with DISPLAY/WAYLAND_DISPLAY unset), while
# explicit egl/osmesa/glx fail here with "Cannot find ... library".
# --no-config ignores ~/.config/f3d so renders are reproducible. --camera-direction (unlike
# --camera-position) auto-frames the model; a position outside the model renders a blank PNG.
f3d_common := "--no-config --verbose=warning --up=+Z --edges --grid --axis --camera-orthographic --resolution=1200,900"

# List recipes
default:
    @just --list

# Format all code
fmt:
    cargo fmt --all

# Verify formatting without modifying files
fmt-check:
    cargo fmt --all -- --check

# Clippy on all targets; any warning is an error
lint:
    cargo clippy --all-targets --all-features -- -D warnings

# Run tests (nextest if installed, else cargo test); doctests always via cargo test
test:
    #!/usr/bin/env bash
    set -euo pipefail
    if cargo nextest --version >/dev/null 2>&1; then
        cargo nextest run --all-targets --all-features
    else
        cargo test --all-targets --all-features
    fi
    cargo test --doc --all-features

# Single verification entrypoint: fmt-check, lint, test
check: fmt-check lint test

# Render an STL to PNG, isometric view (Z up), headless
preview file out="preview.png":
    f3d "{{file}}" {{f3d_common}} --camera-direction=-1,1,-1 --output="{{out}}"
    @test -s "{{out}}" && echo "wrote {{out}}"

# Render an STL to PNG, top-down view (looking along -Z; X axis points up, Y points left)
preview-top file out="preview-top.png":
    f3d "{{file}}" {{f3d_common}} --camera-direction=0,0,-1 --output="{{out}}"
    @test -s "{{out}}" && echo "wrote {{out}}"
