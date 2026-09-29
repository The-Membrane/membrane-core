#!/bin/bash

# Build script for Membrane contracts with proper Wasm features
set -e

echo "Building Membrane contracts with bulk memory support..."

# Set Rust target for Wasm
export RUSTFLAGS="-C target-feature=+bulk-memory,+mutable-globals"

# Build all contracts
for contract_dir in contracts/*/; do
    if [ -f "$contract_dir/Cargo.toml" ]; then
        contract_name=$(basename "$contract_dir")
        echo "Building $contract_name..."
        
        cd "$contract_dir"
        
        # Clean previous builds
        cargo clean
        
        # Build with wasm-pack for proper Wasm generation
        wasm-pack build --target web --out-dir ../../artifacts/temp_$contract_name
        
        # Move the generated wasm file
        if [ -f "../../artifacts/temp_$contract_name/${contract_name//-/_}_bg.wasm" ]; then
            mv "../../artifacts/temp_$contract_name/${contract_name//-/_}_bg.wasm" "../../artifacts/${contract_name//-/_}.wasm"
            echo "✓ Built $contract_name"
        else
            echo "✗ Failed to build $contract_name"
        fi
        
        # Clean up temp directory
        rm -rf "../../artifacts/temp_$contract_name"
        
        cd ../..
    fi
done

echo "Build complete! All contracts have been built with bulk memory support." 