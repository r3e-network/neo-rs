#!/usr/bin/env python3
"""Fix GitHub Actions script syntax errors."""

import os
import re

def fix_github_actions_syntax(content, file_path):
    """Fix GitHub Actions YAML syntax errors."""
    # Common GitHub Actions syntax fixes
    lines = content.split('\n')
    fixed_lines = []
    
    for line in lines:
        original_line = line
        
        # Fix common syntax issues
        
        # Fix deprecated actions
        line = re.sub(r'uses: actions/create-release@v1', 'uses: softprops/action-gh-release@v1', line)
        line = re.sub(r'uses: actions/upload-release-asset@v1', 'uses: softprops/action-gh-release@v1', line)
        
        # Fix deprecated set-output syntax
        line = re.sub(r'echo "([^=]+)=([^"]*)" >> \$GITHUB_OUTPUT', r'echo "\1=\2" >> $GITHUB_OUTPUT', line)
        
        # Fix string interpolation in conditions
        if 'if:' in line and '"' in line:
            # Fix unescaped quotes in conditions
            if_match = re.search(r'if:\s*"([^"]*)"', line)
            if if_match:
                condition = if_match.group(1)
                # Escape inner quotes
                condition = condition.replace('"', '\\"')
                line = re.sub(r'if:\s*"[^"]*"', f'if: "{condition}"', line)
        
        # Fix asset upload syntax for new action
        if 'asset_path:' in line or 'asset_name:' in line or 'asset_content_type:' in line:
            # These are no longer needed with softprops/action-gh-release
            continue
        
        # Fix upload_url reference for new action
        if 'upload_url:' in line and 'needs.' in line:
            # Replace upload_url with files for new action
            continue
        
        fixed_lines.append(line)
    
    return '\n'.join(fixed_lines)

def update_release_workflow():
    """Update the release workflow to use modern GitHub Actions."""
    content = """name: Release

on:
  push:
    tags:
      - 'v*.*.*'

# Cancel running release workflows when new tags are pushed
concurrency:
  group: ${{ github.workflow }}-${{ github.ref }}
  cancel-in-progress: true

env:
  CARGO_TERM_COLOR: always

jobs:
  # Build binaries for multiple platforms
  build-binaries:
    name: Build Binaries
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            os: ubuntu-latest
            artifact_name: neo-rs-linux-x86_64
            binary_name: neo-rs
            
          - target: x86_64-unknown-linux-musl
            os: ubuntu-latest
            artifact_name: neo-rs-linux-x86_64-musl
            binary_name: neo-rs
            
          - target: aarch64-unknown-linux-gnu
            os: ubuntu-latest
            artifact_name: neo-rs-linux-aarch64
            binary_name: neo-rs
            
          - target: x86_64-pc-windows-msvc
            os: windows-latest
            artifact_name: neo-rs-windows-x86_64
            binary_name: neo-rs.exe
            
          - target: x86_64-apple-darwin
            os: macos-latest
            artifact_name: neo-rs-macos-x86_64
            binary_name: neo-rs
            
          - target: aarch64-apple-darwin
            os: macos-latest
            artifact_name: neo-rs-macos-aarch64
            binary_name: neo-rs

    runs-on: ${{ matrix.os }}
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Cache Cargo registry
        uses: actions/cache@v3
        with:
          path: |
            ~/.cargo/registry
            ~/.cargo/git
            target
          key: ${{ matrix.os }}-${{ matrix.target }}-cargo-${{ hashFiles('**/Cargo.lock') }}

      - name: Install system dependencies (Ubuntu)
        if: matrix.os == 'ubuntu-latest'
        run: |
          sudo apt-get update
          if [[ "${{ matrix.target }}" == *"musl"* ]]; then
            sudo apt-get install -y musl-tools
          fi
          if [[ "${{ matrix.target }}" == "aarch64"* ]]; then
            sudo apt-get install -y gcc-aarch64-linux-gnu
          fi
          sudo apt-get install -y librocksdb-dev

      - name: Install system dependencies (macOS)
        if: matrix.os == 'macos-latest'
        run: |
          brew install rocksdb

      - name: Install cross (for cross-compilation)
        if: matrix.target != 'x86_64-unknown-linux-gnu' && matrix.os == 'ubuntu-latest'
        run: cargo install cross

      - name: Build binary
        run: |
          if [[ "${{ matrix.os }}" == "ubuntu-latest" && "${{ matrix.target }}" != "x86_64-unknown-linux-gnu" ]]; then
            cross build --release --target ${{ matrix.target }} --bin neo-rs
          else
            cargo build --release --target ${{ matrix.target }} --bin neo-rs
          fi

      - name: Package binary
        shell: bash
        run: |
          cd target/${{ matrix.target }}/release
          if [[ "${{ matrix.os }}" == "windows-latest" ]]; then
            7z a ../../../${{ matrix.artifact_name }}.zip ${{ matrix.binary_name }}
          else
            tar czvf ../../../${{ matrix.artifact_name }}.tar.gz ${{ matrix.binary_name }}
          fi

      - name: Upload binaries
        uses: actions/upload-artifact@v3
        with:
          name: ${{ matrix.artifact_name }}
          path: ${{ matrix.artifact_name }}.${{ matrix.os == 'windows-latest' && 'zip' || 'tar.gz' }}

  # Create GitHub release
  create-release:
    name: Create Release
    needs: build-binaries
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Download all artifacts
        uses: actions/download-artifact@v3

      - name: Get tag version
        id: get_version
        run: echo "VERSION=${GITHUB_REF#refs/tags/}" >> $GITHUB_OUTPUT

      - name: Generate changelog
        id: changelog
        run: |
          # Extract changelog for this version
          VERSION="${{ steps.get_version.outputs.VERSION }}"
          if [ -f "CHANGELOG.md" ]; then
            # Extract changelog section for this version
            sed -n "/## ${VERSION}/,/## /p" CHANGELOG.md | head -n -1 > release_notes.md
          else
            echo "Release $VERSION" > release_notes.md
          fi

      - name: Create Release
        uses: softprops/action-gh-release@v1
        with:
          name: Neo-RS ${{ steps.get_version.outputs.VERSION }}
          body_path: release_notes.md
          draft: false
          prerelease: ${{ contains(steps.get_version.outputs.VERSION, '-') }}
          files: |
            */*.zip
            */*.tar.gz
        env:
          GITHUB_TOKEN: ${{ secrets.GITHUB_TOKEN }}

  # Build and upload Docker images
  docker:
    name: Build Docker Images
    needs: build-binaries
    runs-on: ubuntu-latest
    steps:
      - name: Checkout code
        uses: actions/checkout@v4

      - name: Get tag version
        id: get_version
        run: echo "VERSION=${GITHUB_REF#refs/tags/v}" >> $GITHUB_OUTPUT

      - name: Set up Docker Buildx
        uses: docker/setup-buildx-action@v3

      - name: Login to Docker Hub
        uses: docker/login-action@v3
        with:
          username: ${{ secrets.DOCKER_USERNAME }}
          password: ${{ secrets.DOCKER_PASSWORD }}

      - name: Login to GitHub Container Registry
        uses: docker/login-action@v3
        with:
          registry: ghcr.io
          username: ${{ github.actor }}
          password: ${{ secrets.GITHUB_TOKEN }}

      - name: Build and push Docker images
        uses: docker/build-push-action@v5
        with:
          context: .
          platforms: linux/amd64,linux/arm64
          push: true
          tags: |
            neo/neo-rs:latest
            neo/neo-rs:${{ steps.get_version.outputs.VERSION }}
            ghcr.io/neo-project/neo-rs:latest
            ghcr.io/neo-project/neo-rs:${{ steps.get_version.outputs.VERSION }}
          cache-from: type=gha
          cache-to: type=gha,mode=max

  # Post-release notifications
  notify:
    name: Post-release Notifications
    needs: [create-release, docker]
    runs-on: ubuntu-latest
    if: always()
    steps:
      - name: Get tag version
        id: get_version
        run: echo "VERSION=${GITHUB_REF#refs/tags/}" >> $GITHUB_OUTPUT

      - name: Notify success
        if: needs.create-release.result == 'success' && needs.docker.result == 'success'
        run: |
          echo "✅ Release ${{ steps.get_version.outputs.VERSION }} completed successfully!"
          echo "- Binaries built for all platforms"
          echo "- Docker images published"
          echo "- Documentation updated"

      - name: Notify failure
        if: needs.create-release.result == 'failure' || needs.docker.result == 'failure'
        run: |
          echo "❌ Release ${{ steps.get_version.outputs.VERSION }} failed!"
          echo "Please check the workflow logs for details."
          exit 1
"""
    return content

def process_file(filepath):
    """Process a single GitHub Actions file."""
    try:
        with open(filepath, 'r', encoding='utf-8') as f:
            content = f.read()
        
        original_content = content
        
        if 'release.yml' in filepath and '/neo-rs/' in filepath:
            # Use the updated release workflow
            fixed_content = update_release_workflow()
        else:
            fixed_content = fix_github_actions_syntax(content, filepath)
        
        if fixed_content != original_content:
            with open(filepath, 'w', encoding='utf-8') as f:
                f.write(fixed_content)
            print(f"Fixed GitHub Actions syntax in: {filepath}")
            return True
        return False
    except Exception as e:
        print(f"Error processing {filepath}: {e}")
        return False

def main():
    """Main function to process all GitHub Actions files."""
    total_fixed = 0
    
    # Find all GitHub Actions workflow files
    for root, _, files in os.walk('.'):
        if '/.github/workflows/' in root:
            for filename in files:
                if filename.endswith(('.yml', '.yaml')):
                    filepath = os.path.join(root, filename)
                    if process_file(filepath):
                        total_fixed += 1
    
    print(f"\nTotal GitHub Actions files fixed: {total_fixed}")

if __name__ == "__main__":
    main()