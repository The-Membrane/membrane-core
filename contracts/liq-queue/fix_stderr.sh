#!/bin/bash
# Quick fix for StdError::GenericErr compatibility with cosmwasm-std 2.0

echo "Fixing StdError::GenericErr issues in liq-queue contract..."

# Fix in query.rs
sed -i '' 's/StdError::GenericErr {$/StdError::generic_err(/g' src/query.rs
sed -i '' 's/msg: "Queue for this asset doesn'\''t exist".to_string(),$/    "Queue for this asset doesn'\''t exist"/g' src/query.rs
sed -i '' 's/}$/)/g' src/query.rs

# Fix pattern: msg: format!(...),
find src -name "*.rs" -exec sed -i '' 's/StdError::GenericErr {$/StdError::generic_err(/g' {} \;
find src -name "*.rs" -exec sed -i '' 's/msg: \(.*\)\.to_string(),$/\1/g' {} \;

echo "Done! Now run: cargo test"
