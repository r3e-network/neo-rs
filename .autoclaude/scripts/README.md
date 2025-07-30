# AutoClaude Scripts

This folder contains quality check scripts that ensure your code meets production standards.

## Built-in Scripts

1. **production-readiness.js** - Scans for TODO, FIXME, placeholders, and incomplete implementations
2. **build-check.js** - Verifies that your project builds successfully  
3. **test-check.js** - Runs your test suite and ensures all tests pass
4. **format-check.js** - Checks code formatting using project-specific tools
5. **github-actions.js** - Validates GitHub Actions workflow files
6. **tdd-automation.js** - Ensures test coverage and test-driven development
7. **ai-code-review.js** - AI-powered code quality, security, and performance review
8. **doc-generator.js** - Checks for missing documentation and generates suggestions

## Custom Scripts

You can add your own validation scripts to this folder. Scripts should:

1. Be executable (chmod +x)
2. Return JSON output with this format:
   ```json
   {
     "passed": true/false,
     "errors": ["error1", "error2"],
     "warnings": ["warning1", "warning2"]
   }
   ```
3. Exit with code 0 on success, non-zero on failure

## Configuration

Scripts are configured in the `config.json` file in the parent directory.
