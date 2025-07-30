#!/bin/bash

# Unit Test Coverage Check Script for Neo Rust
# Verifies that all C# Neo unit tests have been converted to Rust equivalents

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
PURPLE='\033[0;35m'
CYAN='\033[0;36m'
NC='\033[0m'

# Configuration
RUST_TEST_DIR="crates"
REPORT_FILE="test_coverage_report.md"
JSON_REPORT="test_coverage_report.json"

echo -e "${CYAN}╔══════════════════════════════════════════════════════════════════╗${NC}"
echo -e "${CYAN}║                🧪 NEO UNIT TEST COVERAGE CHECKER 🧪            ║${NC}"
echo -e "${CYAN}║                                                                  ║${NC}"
echo -e "${CYAN}║     Analyzing C# to Rust Unit Test Conversion Coverage          ║${NC}"
echo -e "${CYAN}╚══════════════════════════════════════════════════════════════════╝${NC}"
echo

# Function to find all C# test files
find_csharp_tests() {
    local temp_file="csharp_tests.tmp"
    > "$temp_file"
    
    # Search for C# test files in both directories
    if [[ -d "neo_csharp/tests" ]]; then
        find neo_csharp/tests -name "*.cs" >> "$temp_file" 2>/dev/null || true
    fi
    
    if [[ -d "neo_csharp_reference/tests" ]]; then
        find neo_csharp_reference/tests -name "*.cs" >> "$temp_file" 2>/dev/null || true
    fi
    
    cat "$temp_file"
    rm -f "$temp_file"
}

# Function to extract C# tests
extract_csharp_tests() {
    echo -e "${BLUE}🔍 Scanning C# test files...${NC}"
    
    local test_methods=0
    local test_classes=0
    local test_files=0
    
    while IFS= read -r file; do
        if [[ -f "$file" ]]; then
            ((test_files++))
            
            # Count test classes
            local classes=$(grep -c "class.*Test\|Test.*class" "$file" 2>/dev/null || echo 0)
            ((test_classes += classes))
            
            # Count test methods
            local methods=$(grep -c "\[Test\]\|public.*Test.*(" "$file" 2>/dev/null || echo 0)
            ((test_methods += methods))
        fi
    done < <(find_csharp_tests)
    
    echo -e "${GREEN}   Found $test_files C# test files${NC}"
    echo -e "${GREEN}   Found $test_classes test classes${NC}"
    echo -e "${GREEN}   Found $test_methods test methods${NC}"
    echo
    
    # Store results in global variables
    CSHARP_TEST_FILES=$test_files
    CSHARP_TEST_CLASSES=$test_classes
    CSHARP_TEST_METHODS=$test_methods
}

# Function to extract Rust tests
extract_rust_tests() {
    echo -e "${BLUE}🦀 Scanning Rust test files...${NC}"
    
    local test_files=0
    local test_modules=0
    local test_functions=0
    
    if [[ -d "$RUST_TEST_DIR" ]]; then
        # Count files with tests
        test_files=$(find "$RUST_TEST_DIR" -name "*.rs" -exec grep -l "#\[test\]" {} \; 2>/dev/null | wc -l)
        
        # Count test modules
        test_modules=$(find "$RUST_TEST_DIR" -name "*.rs" -exec grep -c "#\[cfg(test)\]" {} \; 2>/dev/null | awk '{sum += $1} END {print sum+0}')
        
        # Count test functions
        test_functions=$(find "$RUST_TEST_DIR" -name "*.rs" -exec grep -c "#\[test\]" {} \; 2>/dev/null | awk '{sum += $1} END {print sum+0}')
    fi
    
    echo -e "${GREEN}   Found $test_files Rust test files${NC}"
    echo -e "${GREEN}   Found $test_modules test modules${NC}"
    echo -e "${GREEN}   Found $test_functions test functions${NC}"
    echo
    
    # Store results in global variables
    RUST_TEST_FILES=$test_files
    RUST_TEST_MODULES=$test_modules
    RUST_TEST_FUNCTIONS=$test_functions
}

# Function to analyze test coverage by area
analyze_test_coverage() {
    echo -e "${BLUE}📊 Analyzing test coverage by area...${NC}"
    
    local areas=("Core" "Transaction" "Block" "Blockchain" "Consensus" "Cryptography" "Network" "SmartContract" "VM" "Wallet" "Persistence" "Ledger" "IO" "Json")
    
    echo -e "${CYAN}Test Coverage by Area:${NC}"
    printf "%-15s %-10s %-10s %-10s\n" "Area" "C# Tests" "Rust Tests" "Coverage"
    printf "%-15s %-10s %-10s %-10s\n" "----" "---------" "----------" "--------"
    
    for area in "${areas[@]}"; do
        # Count C# tests for this area
        local csharp_count=0
        while IFS= read -r file; do
            if [[ -f "$file" ]] && grep -qi "$area" "$file" 2>/dev/null; then
                ((csharp_count++))
            fi
        done < <(find_csharp_tests)
        
        # Count Rust tests for this area
        local rust_count=0
        if [[ -d "$RUST_TEST_DIR" ]]; then
            rust_count=$(find "$RUST_TEST_DIR" -name "*.rs" -exec grep -l "test.*$area\|$area.*test" {} \; 2>/dev/null | wc -l)
        fi
        
        # Calculate coverage
        local coverage=0
        if [[ $csharp_count -gt 0 ]]; then
            coverage=$((rust_count * 100 / csharp_count))
        fi
        
        # Color code the result
        local color_code=""
        if [[ $coverage -ge 80 ]]; then
            color_code="${GREEN}"
        elif [[ $coverage -ge 50 ]]; then
            color_code="${YELLOW}"
        else
            color_code="${RED}"
        fi
        
        printf "%-15s %-10s %-10s ${color_code}%-10s${NC}\n" "$area" "$csharp_count" "$rust_count" "${coverage}%"
    done
    
    echo
}

# Function to find missing critical tests
find_missing_tests() {
    echo -e "${BLUE}❌ Checking for missing critical tests...${NC}"
    
    local critical_tests=("TransactionTest" "BlockTest" "BlockchainTest" "NeoSystemTest" "MemoryPoolTest" "ConsensusTest" "P2PTest" "VMTest" "CryptographyTest" "WalletTest")
    
    for test in "${critical_tests[@]}"; do
        # Check if exists in C#
        local csharp_exists=false
        while IFS= read -r file; do
            if [[ -f "$file" ]] && grep -qi "$test" "$file" 2>/dev/null; then
                csharp_exists=true
                break
            fi
        done < <(find_csharp_tests)
        
        if $csharp_exists; then
            # Check if exists in Rust
            local rust_exists=false
            if [[ -d "$RUST_TEST_DIR" ]]; then
                if find "$RUST_TEST_DIR" -name "*.rs" -exec grep -qi "$test\|$(echo $test | sed 's/Test$//')" {} \; 2>/dev/null; then
                    rust_exists=true
                fi
            fi
            
            if $rust_exists; then
                echo -e "${GREEN}   ✅ $test - Rust equivalent found${NC}"
            else
                echo -e "${RED}   ❌ $test - Missing Rust equivalent${NC}"
            fi
        fi
    done
    
    echo
}

# Function to generate reports
generate_reports() {
    echo -e "${BLUE}📝 Generating reports...${NC}"
    
    # Calculate overall coverage
    local overall_coverage=0
    if [[ $CSHARP_TEST_METHODS -gt 0 ]]; then
        overall_coverage=$((RUST_TEST_FUNCTIONS * 100 / CSHARP_TEST_METHODS))
    fi
    
    # Generate Markdown report
    cat > "$REPORT_FILE" << EOF
# Neo Rust Unit Test Coverage Report

Generated on: $(date '+%Y-%m-%d %H:%M:%S')

## Summary

| Metric | C# | Rust | Coverage |
|--------|-------|------|----------|
| Test Files | $CSHARP_TEST_FILES | $RUST_TEST_FILES | $((RUST_TEST_FILES * 100 / CSHARP_TEST_FILES))% |
| Test Classes/Modules | $CSHARP_TEST_CLASSES | $RUST_TEST_MODULES | $((RUST_TEST_MODULES * 100 / CSHARP_TEST_CLASSES))% |
| Test Methods/Functions | $CSHARP_TEST_METHODS | $RUST_TEST_FUNCTIONS | ${overall_coverage}% |

## Overall Status

EOF

    if [[ $overall_coverage -ge 80 ]]; then
        echo "🟢 **Excellent** - Test coverage is very good" >> "$REPORT_FILE"
    elif [[ $overall_coverage -ge 60 ]]; then
        echo "🟡 **Good** - Test coverage is acceptable" >> "$REPORT_FILE"
    elif [[ $overall_coverage -ge 40 ]]; then
        echo "🟠 **Fair** - Test coverage needs improvement" >> "$REPORT_FILE"
    else
        echo "🔴 **Poor** - Test coverage is insufficient" >> "$REPORT_FILE"
    fi
    
    cat >> "$REPORT_FILE" << EOF

## Recommendations

1. **Priority 1**: Convert missing critical test classes
2. **Priority 2**: Achieve 80%+ test coverage
3. **Priority 3**: Add integration and end-to-end tests

## Test Conversion Guidelines

### C# to Rust Conversion Examples

\`\`\`csharp
// C# Test
[Test]
public void TestTransactionValidation()
{
    var tx = new Transaction();
    Assert.IsTrue(tx.Verify());
}
\`\`\`

\`\`\`rust
// Rust Test
#[test]
fn test_transaction_validation() {
    let tx = Transaction::new();
    assert!(tx.verify().is_ok());
}
\`\`\`

EOF
    
    # Generate JSON report
    cat > "$JSON_REPORT" << EOF
{
  "generated_at": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "summary": {
    "csharp_test_files": $CSHARP_TEST_FILES,
    "rust_test_files": $RUST_TEST_FILES,
    "csharp_test_methods": $CSHARP_TEST_METHODS,
    "rust_test_functions": $RUST_TEST_FUNCTIONS,
    "overall_coverage_percent": $overall_coverage
  },
  "status": "$(if [[ $overall_coverage -ge 80 ]]; then echo "excellent"; elif [[ $overall_coverage -ge 60 ]]; then echo "good"; elif [[ $overall_coverage -ge 40 ]]; then echo "fair"; else echo "poor"; fi)"
}
EOF
    
    echo -e "${GREEN}   ✅ Reports generated:${NC}"
    echo -e "${GREEN}      • $REPORT_FILE${NC}"
    echo -e "${GREEN}      • $JSON_REPORT${NC}"
    echo
}

# Function to suggest next steps
suggest_next_steps() {
    echo -e "${BLUE}🎯 Recommended Actions:${NC}"
    echo
    
    local overall_coverage=$((RUST_TEST_FUNCTIONS * 100 / CSHARP_TEST_METHODS))
    
    if [[ $overall_coverage -lt 50 ]]; then
        echo -e "${RED}🚨 URGENT: Test coverage is very low${NC}"
        echo "   1. Focus on core functionality tests first"
        echo "   2. Convert Transaction, Block, and Blockchain tests"
        echo "   3. Add basic unit tests for all public APIs"
    elif [[ $overall_coverage -lt 80 ]]; then
        echo -e "${YELLOW}⚠️  MODERATE: Test coverage needs improvement${NC}"
        echo "   1. Convert remaining critical test classes"
        echo "   2. Add edge case and error condition tests"
        echo "   3. Implement integration tests"
    else
        echo -e "${GREEN}✅ GOOD: Test coverage is adequate${NC}"
        echo "   1. Add property-based testing"
        echo "   2. Implement performance benchmarks"
        echo "   3. Add fuzz testing for security"
    fi
    
    echo
    echo -e "${CYAN}General Recommendations:${NC}"
    echo "   • Set up automated test coverage monitoring"
    echo "   • Add CI/CD test result comparison"
    echo "   • Implement regression testing"
    echo "   • Create test data generators"
    echo
}

# Main execution
main() {
    # Check if we're in the right directory
    if [[ ! -d "$RUST_TEST_DIR" ]]; then
        echo -e "${RED}Error: Must be run from the neo-rs project root directory${NC}"
        echo -e "${RED}Expected directory: $RUST_TEST_DIR${NC}"
        exit 1
    fi
    
    echo -e "${CYAN}Starting comprehensive unit test coverage analysis...${NC}"
    echo
    
    # Run analysis
    extract_csharp_tests
    extract_rust_tests
    analyze_test_coverage
    find_missing_tests
    generate_reports
    suggest_next_steps
    
    # Final summary
    local overall_coverage=$((RUST_TEST_FUNCTIONS * 100 / CSHARP_TEST_METHODS))
    
    echo -e "${CYAN}╔══════════════════════════════════════════════════════════════════╗${NC}"
    echo -e "${CYAN}║                    📊 FINAL SUMMARY 📊                         ║${NC}"
    echo -e "${CYAN}║                                                                  ║${NC}"
    echo -e "${CYAN}║     Overall Test Coverage: ${overall_coverage}%                                ║${NC}"
    echo -e "${CYAN}║     C# Tests: $CSHARP_TEST_METHODS | Rust Tests: $RUST_TEST_FUNCTIONS                            ║${NC}"
    echo -e "${CYAN}║                                                                  ║${NC}"
    echo -e "${CYAN}║     Status: $(if [[ $overall_coverage -ge 80 ]]; then echo "🟢 Excellent"; elif [[ $overall_coverage -ge 60 ]]; then echo "🟡 Good     "; elif [[ $overall_coverage -ge 40 ]]; then echo "🟠 Fair     "; else echo "🔴 Poor     "; fi)                                   ║${NC}"
    echo -e "${CYAN}╚══════════════════════════════════════════════════════════════════╝${NC}"
}

# Initialize global variables
CSHARP_TEST_FILES=0
CSHARP_TEST_CLASSES=0
CSHARP_TEST_METHODS=0
RUST_TEST_FILES=0
RUST_TEST_MODULES=0
RUST_TEST_FUNCTIONS=0

# Run main function
main "$@"