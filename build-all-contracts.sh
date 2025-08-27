#!/bin/bash

# Build script for all Membrane contracts with rust-optimizer
set -e

echo "Building all Membrane contracts with rust-optimizer..."

# Set Rust target for Wasm with proper features
export RUSTFLAGS="-C target-feature=+bulk-memory,+mutable-globals"

# Create artifacts directory
mkdir -p artifacts

# Define contracts to exclude
EXCLUDED_CONTRACTS=(
    "managed-market"
    "market-manager"
    "mm-swap"
    "mm_osmosis_oracle"
)

# Get rust-optimizer version from Makefile.toml
RUST_OPTIMIZER_VERSION=$(grep 'RUST_OPTIMIZER_VERSION' Makefile.toml | sed 's/.*= "\([^"]*\)".*/\1/')
echo "Using rust-optimizer version: $RUST_OPTIMIZER_VERSION"

# Build all contracts using rust-optimizer
for contract_dir in contracts/*/; do
    if [ -f "$contract_dir/Cargo.toml" ]; then
        contract_name=$(basename "$contract_dir")
        
        # Check if contract should be excluded
        if [[ " ${EXCLUDED_CONTRACTS[@]} " =~ " ${contract_name} " ]]; then
            echo "Skipping $contract_name (excluded)..."
            continue
        fi
        
        echo "Building $contract_name with rust-optimizer..."
        
        cd "$contract_dir"
        
        # Clean previous builds
        cargo clean
        
        # Use rust-optimizer for this specific contract
        if [[ $(arch) == "arm64" ]]; then
            image="cosmwasm/workspace-optimizer-arm64:$RUST_OPTIMIZER_VERSION"
        else
            image="cosmwasm/workspace-optimizer:$RUST_OPTIMIZER_VERSION"
        fi
        
        echo "Using Docker image: $image"
        
        # Create a temporary workspace for this single contract
        cp Cargo.toml Cargo.toml.original
        cat > Cargo.toml << EOF
[workspace]
members = ["."]

[profile.release]
codegen-units = 1
debug = false
debug-assertions = false
incremental = false
lto = true
opt-level = 'z'
overflow-checks = true
panic = 'abort'
rpath = false
EOF
        
        # Run rust-optimizer on this single contract
        docker run --rm -v "$(pwd)":/code \
            --mount type=volume,source="$(basename "$(pwd)")_cache",target=/code/target \
            --mount type=volume,source=registry_cache,target=/usr/local/cargo/registry \
            "$image"
        
        # Restore original Cargo.toml
        mv Cargo.toml.original Cargo.toml
        
        # Copy the generated wasm file
        if [ -f "../../target/wasm32-unknown-unknown/release/${contract_name//-/_}.wasm" ]; then
            cp "../../target/wasm32-unknown-unknown/release/${contract_name//-/_}.wasm" "../../artifacts/${contract_name//-/_}.wasm"
            echo "✓ Built and optimized $contract_name"
        else
            echo "✗ Failed to build $contract_name"
        fi
        
        cd ../..
    fi
done

echo "Build complete! All contracts have been built and optimized with rust-optimizer."
echo "Generated files:"
ls -la artifacts/*.wasm 