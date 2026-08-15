#!/usr/bin/env bash
# SPDX-License-Identifier: Apache-2.0
# Feature-driven build matrix: every Cargo feature combination must
# compile, and a representative set is executed as tests.
set -euo pipefail
cd "$(dirname "$0")/.."

cargo fmt --check

features=(dsstore appledouble maczip plist xattr volumetrace)
n=${#features[@]}
total=$((1 << n))

for ((mask = 0; mask < total; mask++)); do
    list=""
    for ((i = 0; i < n; i++)); do
        if (( mask & (1 << i) )); then
            [ -n "$list" ] && list+=","
            list+="${features[i]}"
        fi
    done
    if [ -n "$list" ]; then
        echo "==> [$mask/$((total-1))] check --no-default-features --features $list"
        cargo check --quiet --no-default-features --features "$list"
    else
        echo "==> [0] check --no-default-features"
        cargo check --quiet --no-default-features
    fi
done

echo
echo "==> test default features"
cargo test --quiet
echo "==> test --no-default-features"
cargo test --quiet --no-default-features --lib --tests
for f in "${features[@]}"; do
    echo "==> test --no-default-features --features $f"
    cargo test --quiet --no-default-features --features "$f" --lib --tests
done

echo
echo "All feature combinations compile and the selected test matrix passes."
