# Development Directory

This directory contains development-related files that are not part of the main codebase.

## Structure

- `debug/` - Debug scripts and temporary debugging files
- `test-files/` - Test scripts and temporary test files  
- `scripts/` - Development and analysis scripts
- `logs/` - Log files from development testing

## Usage

Files in this directory are for development purposes only and should not be included in production builds. Most files here are git-ignored.

## Notes

- Test files here are for manual testing and debugging
- For proper unit and integration tests, see the `tests/` directories in each crate
- Log files are automatically ignored by git
- Debug files are temporary and should be cleaned up regularly