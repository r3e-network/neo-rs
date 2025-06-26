# Configuration Directory

This directory contains configuration files for Neo-RS.

## Structure

- `examples/` - Example configuration files
- `rustfmt.toml` - Symbolic link to root rustfmt.toml
- `clippy.toml` - Symbolic link to root clippy.toml

Note: Cargo expects rustfmt.toml and clippy.toml in the project root, so the actual files are there with symlinks here for organization.

## Usage

1. Copy configuration examples to the project root
2. Customize settings as needed
3. Configuration files in the root are git-ignored

## Example Configurations

- `examples/neo-config.toml.example` - Main Neo-RS node configuration template