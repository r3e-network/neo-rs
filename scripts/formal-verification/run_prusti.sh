#!/usr/bin/env bash
# Reproducible Prusti pilot verification of 6 pure functions.
# Prusti 0.2.2 (2024-03-26) + nightly-2023-09-15 toolchain (installed via rustup),
# Java 21 (Viper backend).
set -u
export PATH="/c/Program Files/Eclipse Adoptium/jre-21.0.12.101-hotspot/bin:$PATH"
export JAVA_HOME="$(dirname "$(dirname "$(which java)")")"
BASE="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$BASE"
echo "== running Prusti on formal/prusti/pilot_standalone.rs =="
"$BASE/.tools/prusti-rustc.exe" --edition 2021 formal/prusti/pilot_standalone.rs
echo "exit=$?  (0 = all 14 items successfully verified)"