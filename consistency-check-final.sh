#!/bin/bash

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Counters
PASSED=0
FAILED=0
WARNINGS=0

# Helper functions
check_present() {
    local description="$1"
    local command="$2"
    local max_allowed="$3"
    
    count=$(eval "$command" 2>/dev/null | wc -l)
    if [ "$count" -le "$max_allowed" ]; then
        echo -e "${GREEN}✓ PASS${NC} - $description (found: $count, max allowed: $max_allowed)"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗ FAIL${NC} - $description (found $count occurrences, max allowed: $max_allowed)"
        FAILED=$((FAILED + 1))
    fi
}

check_absent() {
    local description="$1"
    local command="$2"
    
    count=$(eval "$command" 2>/dev/null | wc -l)
    if [ "$count" -eq 0 ]; then
        echo -e "${GREEN}✓ PASS${NC} - $description"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗ FAIL${NC} - $description (found $count occurrences)"
        FAILED=$((FAILED + 1))
    fi
}

check_warning() {
    local description="$1"
    local command="$2"
    local max_allowed="$3"
    
    count=$(eval "$command" 2>/dev/null | wc -l)
    if [ "$count" -le "$max_allowed" ]; then
        echo -e "${GREEN}✓ PASS${NC} - $description (found: $count, max allowed: $max_allowed)"
        PASSED=$((PASSED + 1))
    else
        echo -e "${YELLOW}⚠ WARN${NC} - $description (found: $count, max allowed: $max_allowed)"
        WARNINGS=$((WARNINGS + 1))
    fi
}

echo "=== Neo-RS Codebase Consistency Check ==="
echo "Timestamp: $(date)"
echo

echo -e "${BLUE}=== 1. Debug Statement Consistency ===${NC}"
check_absent "No println! statements in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep -l 'println!' {} + | xargs grep -l 'println!' | grep -v -E '(cli/src/console|cli/src/main|//)'"
check_absent "No dbg! statements in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep -l 'dbg!' {} +"
check_absent "No print! statements in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep -l 'print!' {} + | grep -v 'cli/src/console'"
echo

echo -e "${BLUE}=== 2. Error Handling Consistency ===${NC}"
check_absent "No panic! in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep -l 'panic!' {} + | xargs grep 'panic!' | grep -v -E '(unimplemented!|unreachable!|assert!|//)'"
check_present "Minimal unwrap() in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep -o '\.unwrap()' {} + | grep -v '//' | wc -l" 150
check_absent "No expect() without context" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep '\.expect(\"\")' {} +"
echo

echo -e "${BLUE}=== 3. TODO and Production implementation Consistency ===${NC}"
check_present "TODO comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'TODO' {} +" 5
check_absent "FIXME comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'FIXME' {} +"
check_absent "XXX comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'XXX' {} +"
check_absent "HACK comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'HACK' {} +"
echo

echo -e "${BLUE}=== 4. Code Quality Consistency ===${NC}"
check_absent "Commented out code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -E '^[[:space:]]*//[[:space:]]*(let|const|fn|impl|struct|enum|if|while|for)[[:space:]]' {} +"
check_warning "Functions longer than 100 lines" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec awk '/^[[:space:]]*pub[[:space:]]+fn|^[[:space:]]*fn/ {start=NR} /^}/ {if (NR-start > 100) print FILENAME\":\"start\"-\"NR}' {} +" 10
check_absent "Multiple empty lines" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec awk '/^$/{if (p) print NR; p=1; next} {p=0}' {} +"
echo

echo -e "${BLUE}=== 5. Import Consistency ===${NC}"
check_absent "Wildcard imports in production" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep 'use.*::\*;' {} + | grep -v -E '(prelude|test_|//)'"
check_absent "Unused imports" "find crates/ node/src/ -name '*.rs' -exec grep '#\[allow(unused_imports)\]' {} +"
echo

echo -e "${BLUE}=== 6. Magic Number Consistency ===${NC}"
check_absent "Magic number 15 (use SECONDS_PER_BLOCK)" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/benches/*' -exec grep -w '15' {} + | grep -v -E '(0x15|u15|i15|//|format!|println!|\{:.*15|15\}|\".*15.*\")'  | grep -v 'Duration::from'"
check_absent "Magic number 262144 (use MAX_BLOCK_SIZE)" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -w '262144' {} + | grep -v 'MAX_BLOCK_SIZE'"
check_absent "Magic number 102400 (use MAX_TRANSACTION_SIZE)" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -w '102400' {} + | grep -v 'MAX_TRANSACTION_SIZE'"
echo

echo -e "${BLUE}=== 7. Naming Consistency ===${NC}"
check_absent "Snake case violations in function names" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'fn [A-Z]' {} + | grep -v trait"
check_absent "CamelCase violations in variable names" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -E 'let [a-z]+[A-Z]+[a-zA-Z]*[[:space:]]*=' {} + | grep -v -E '(Some|None|Ok|Err)'"
echo

echo -e "${BLUE}=== 8. Documentation Consistency ===${NC}"
check_present "Public functions without documentation" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -B1 '^pub fn' {} + | grep -B1 '^pub fn' | grep -v '^///' | grep '^pub fn' | wc -l" 20
check_present "Public structs without documentation" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -B1 '^pub struct' {} + | grep -B1 '^pub struct' | grep -v '^///' | grep '^pub struct' | wc -l" 10
echo

echo -e "${BLUE}=== 9. Type Safety Consistency ===${NC}"
check_absent "Use of 'any' type in TypeScript files" "find crates/ node/src/ -name '*.ts' -exec grep ': any' {} +"
check_absent "Unsafe blocks without safety comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -B1 'unsafe {' {} + | grep -B1 'unsafe {' | grep -v '// SAFETY:' | grep 'unsafe {'"
echo

echo -e "${BLUE}=== 10. File Organization Consistency ===${NC}"
check_warning "Files larger than 1000 lines" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec wc -l {} + | awk '\$1 > 1000 {print \$2}' | wc -l" 5
check_present "Minimal public items in mod.rs" "find crates/ node/src/ -name 'mod.rs' -exec sh -c 'count=$(grep -c \"^pub \" \"\$1\"); if [ \$count -gt 10 ]; then echo \"\$1\"; fi' _ {} \; | wc -l" 5
echo

echo -e "${BLUE}=== 11. Dependency Consistency ===${NC}"
check_absent "Git dependencies" "find . -name 'Cargo.toml' -exec grep 'git = ' {} +"
check_present "Workspace path dependencies" "find . -name 'Cargo.toml' -exec grep 'path = ' {} + | grep -v workspace | wc -l" 70
echo

echo -e "${BLUE}=== 12. Security Consistency ===${NC}"
check_absent "Hardcoded credentials" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -E '(password|passwd|pwd|secret|key)[[:space:]]*=[[:space:]]*[\"\047][^\"\047]+[\"\047]' {} + | grep -v 'pub const'"
check_absent "Hardcoded IP addresses" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -E '([0-9]{1,3}\.){3}[0-9]{1,3}' {} + | grep -v version"
echo

echo -e "${BLUE}=== Consistency Check Summary ===${NC}"
TOTAL=$((PASSED + FAILED + WARNINGS))
echo "Total Checks: $TOTAL"
echo -e "${GREEN}Passed: $PASSED${NC}"
echo -e "${YELLOW}Warnings: $WARNINGS${NC}"
echo -e "${RED}Failed: $FAILED${NC}"

# Calculate score
SCORE=$((PASSED * 100 / TOTAL))
echo "Consistency Score: ${SCORE}%"
echo

# Overall status
if [ "$SCORE" -ge 90 ]; then
    echo -e "${GREEN}✓ EXCELLENT CONSISTENCY${NC}"
    echo "The codebase maintains excellent consistency standards."
    STATUS="EXCELLENT"
elif [ "$SCORE" -ge 80 ]; then
    echo -e "${GREEN}✓ GOOD CONSISTENCY${NC}"
    echo "The codebase has good consistency with minor issues."
    STATUS="GOOD"
elif [ "$SCORE" -ge 70 ]; then
    echo -e "${YELLOW}⚠ FAIR CONSISTENCY${NC}"
    echo "The codebase has fair consistency but needs improvement."
    STATUS="FAIR"
else
    echo -e "${RED}✗ POOR CONSISTENCY${NC}"
    echo "The codebase has significant consistency issues that need attention."
    STATUS="NEEDS_IMPROVEMENT"
fi

# Machine-readable output
echo
echo "=== Machine-Readable Results ==="
echo "CONSISTENCY_STATUS=$STATUS"
echo "TOTAL_CHECKS=$TOTAL"
echo "PASSED=$PASSED"
echo "WARNINGS=$WARNINGS"
echo "FAILED=$FAILED"
echo "SCORE=$SCORE"
echo "TIMESTAMP=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
echo

echo "Consistency check completed at $(date)"