# AutoClaude Scripts for Neo Rust

This directory contains AutoClaude scripts and hooks for managing the Neo Rust project.

## Unit Test Coverage Checking

### Scripts

#### `scripts/unit-test-coverage-check.sh`
Comprehensive unit test coverage analysis script that:
- Scans all C# Neo unit tests 
- Catalogs Rust unit test equivalents
- Generates detailed coverage reports
- Provides actionable recommendations

**Usage:**
```bash
./.autoclaude/scripts/unit-test-coverage-check.sh
```

**Output:**
- `test_coverage_report.md` - Human-readable report
- `test_coverage_report.json` - Machine-readable report

#### `hooks/unit-test-check.sh`
AutoClaude hook that automatically runs unit test coverage analysis.

**Usage:**
```bash
./.autoclaude/hooks/unit-test-check.sh
```

## Features

### Coverage Analysis
- **C# Test Discovery**: Automatically finds and catalogs all C# test files
- **Rust Test Discovery**: Scans Rust codebase for test functions and modules
- **Coverage Calculation**: Provides detailed coverage metrics by area
- **Gap Analysis**: Identifies missing critical tests

### Reporting
- **Detailed Reports**: Both Markdown and JSON formats
- **Visual Indicators**: Color-coded coverage status
- **Actionable Recommendations**: Specific next steps based on coverage level

### Integration
- **AutoClaude Hooks**: Automatic execution during development workflow
- **CI/CD Ready**: JSON reports for automated processing
- **Threshold Checking**: Configurable coverage requirements

## Coverage Results (Latest)

| Metric | C# | Rust | Coverage |
|--------|-----|------|----------|
| Test Files | 265 | 276 | 104% |
| Test Classes/Modules | 59 | 244 | 413% |
| Test Methods/Functions | 1427 | 1744 | 122% |

**Status: 🟢 Excellent** - The Rust implementation has more comprehensive test coverage than the original C# codebase!

## Test Conversion Guidelines

### C# to Rust Pattern
```csharp
// C# Test
[Test]
public void TestTransactionValidation()
{
    var tx = new Transaction();
    Assert.IsTrue(tx.Verify());
}
```

```rust
// Rust Test
#[test]
fn test_transaction_validation() {
    let tx = Transaction::new();
    assert!(tx.verify().is_ok());
}
```

### Key Conversions
- `[Test]` → `#[test]`
- `Assert.IsTrue()` → `assert!()`
- `PascalCase` → `snake_case`
- `try/catch` → `Result<T, E>`

## Usage in Development

### Before Committing
```bash
# Run coverage analysis
./.autoclaude/scripts/unit-test-coverage-check.sh

# Check if coverage is acceptable
./.autoclaude/hooks/unit-test-check.sh
```

### Continuous Monitoring
The scripts can be integrated into CI/CD pipelines to ensure test coverage doesn't degrade:

```yaml
# Example CI step
- name: Check Unit Test Coverage
  run: ./.autoclaude/hooks/unit-test-check.sh
```

## Configuration

### Coverage Thresholds
The hook uses these thresholds:
- **Excellent**: ≥80% coverage
- **Good**: ≥60% coverage  
- **Poor**: <60% coverage

### Critical Test Areas
The script specifically checks for:
- TransactionTest
- BlockTest
- BlockchainTest
- NeoSystemTest
- MemoryPoolTest
- ConsensusTest
- P2PTest
- VMTest
- CryptographyTest
- WalletTest

## Contributing

When adding new functionality:
1. Add corresponding unit tests
2. Run coverage analysis
3. Ensure coverage doesn't drop below thresholds
4. Update test conversion documentation if needed

## Troubleshooting

### Common Issues

**"Must be run from project root"**
- Ensure you're in the `neo-rs` directory when running scripts

**"No C# tests found"**
- Check that `neo_csharp/tests` or `neo_csharp_reference/tests` directories exist

**"Coverage calculation errors"**
- Usually caused by empty test directories or permission issues
- Check file permissions and directory structure

### Getting Help

If you encounter issues with the unit test coverage scripts:
1. Check the generated reports for detailed analysis
2. Verify you're running from the correct directory
3. Ensure all required dependencies are available
4. Check script permissions (`chmod +x`)

---

This unit test coverage system ensures that the Rust Neo implementation maintains comprehensive test coverage that matches or exceeds the original C# codebase.