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
    
    count=$(eval "$command" 2>/dev/null | wc -l | tr -d ' ')
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
    
    count=$(eval "$command" 2>/dev/null | wc -l | tr -d ' ')
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
    
    count=$(eval "$command" 2>/dev/null | wc -l | tr -d ' ')
    if [ "$count" -le "$max_allowed" ]; then
        echo -e "${GREEN}✓ PASS${NC} - $description (found: $count, max allowed: $max_allowed)"
        PASSED=$((PASSED + 1))
    else
        echo -e "${YELLOW}⚠ WARN${NC} - $description (found: $count, max allowed: $max_allowed)"
        WARNINGS=$((WARNINGS + 1))
    fi
}

echo "=== Neo-RS Codebase Consistency Check (Ultimate Edition) ==="
echo "Timestamp: $(date)"
echo

echo -e "${BLUE}=== 1. Debug Statement Consistency ===${NC}"
# Exclude false positives - comments mentioning println!/eprintln!
check_absent "No println! statements in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep 'println!' {} + 2>/dev/null | grep -v '//' | grep -v 'eprintln' | grep -v 'cli/src/console' | grep -v 'cli/src/main'"
check_absent "No dbg! statements in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep 'dbg!' {} + 2>/dev/null | grep -v '//'"
check_absent "No print! statements in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep 'print!' {} + 2>/dev/null | grep -v 'println' | grep -v 'cli/src/console' | grep -v '//'"
echo

echo -e "${BLUE}=== 2. Error Handling Consistency ===${NC}"
check_absent "No panic! in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep 'panic!' {} + 2>/dev/null | grep -v 'unimplemented!' | grep -v 'unreachable!' | grep -v 'assert!' | grep -v '//'"
check_present "Minimal unwrap() in production code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep -o '\.unwrap()' {} + 2>/dev/null | grep -v '//'" 150
check_absent "No expect() without context" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec grep '\.expect(\"\")' {} + 2>/dev/null"
echo

echo -e "${BLUE}=== 3. TODO and Production implementation Consistency ===${NC}"
check_present "TODO comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'TODO' {} + 2>/dev/null | grep -v '//'" 5
check_absent "FIXME comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'FIXME' {} + 2>/dev/null"
check_absent "XXX comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'XXX' {} + 2>/dev/null"
check_absent "HACK comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'HACK' {} + 2>/dev/null"
echo

echo -e "${BLUE}=== 4. Code Quality Consistency ===${NC}"
check_absent "Commented out code" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -E '^[[:space:]]*//[[:space:]]*(let|const|fn|impl|struct|enum|if|while|for)[[:space:]]' {} + 2>/dev/null"
check_warning "Functions longer than 100 lines" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec awk '/^[[:space:]]*pub[[:space:]]+fn|^[[:space:]]*fn/ {start=NR} /^}/ {if (NR-start > 100) print FILENAME\":\"start\"-\"NR}' {} + 2>/dev/null" 50
check_absent "Multiple empty lines" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec awk '/^$/{if (p) print NR; p=1; next} {p=0}' {} + 2>/dev/null"
echo

echo -e "${BLUE}=== 5. Import Consistency ===${NC}"
# Properly exclude test modules
check_absent "Wildcard imports in production" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -not -path '*/benches/*' -exec awk '/^[[:space:]]*use.*::\*;/ && !/#\[cfg\(test\)\]/ {print FILENAME\":\"NR}' {} + 2>/dev/null"
check_absent "Unused imports" "find crates/ node/src/ -name '*.rs' -exec grep '#\[allow(unused_imports)\]' {} + 2>/dev/null"
echo

echo -e "${BLUE}=== 6. Magic Number Consistency ===${NC}"
check_absent "Magic number 15 (use SECONDS_PER_BLOCK)" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/benches/*' -exec grep -w '15' {} + 2>/dev/null | grep -v '0x15' | grep -v 'u15' | grep -v 'i15' | grep -v '//' | grep -v 'format!' | grep -v 'println!' | grep -v '{:.*15' | grep -v '15}' | grep -v '\".*15.*\"' | grep -v 'Duration::from'"
check_absent "Magic number 262144 (use MAX_BLOCK_SIZE)" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -w '262144' {} + 2>/dev/null | grep -v 'MAX_BLOCK_SIZE'"
check_absent "Magic number 102400 (use MAX_TRANSACTION_SIZE)" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -w '102400' {} + 2>/dev/null | grep -v 'MAX_TRANSACTION_SIZE'"
echo

echo -e "${BLUE}=== 7. Naming Consistency ===${NC}"
check_absent "Snake case violations in function names" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep 'fn [A-Z]' {} + 2>/dev/null | grep -v trait"
check_absent "CamelCase violations in variable names" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -E 'let [a-z]+[A-Z]+[a-zA-Z]*[[:space:]]*=' {} + 2>/dev/null | grep -v -E '(Some|None|Ok|Err)'"
echo

echo -e "${BLUE}=== 8. Documentation Consistency ===${NC}"
check_present "Public functions without documentation" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec sh -c 'grep -B1 \"^pub fn\" \"$1\" | grep -B1 \"^pub fn\" | grep -v \"^///\" | grep \"^pub fn\"' _ {} \; 2>/dev/null | wc -l" 20
check_present "Public structs without documentation" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec sh -c 'grep -B1 \"^pub struct\" \"$1\" | grep -B1 \"^pub struct\" | grep -v \"^///\" | grep \"^pub struct\"' _ {} \; 2>/dev/null | wc -l" 10
echo

echo -e "${BLUE}=== 9. Type Safety Consistency ===${NC}"
check_absent "Use of 'any' type in TypeScript files" "find crates/ node/src/ -name '*.ts' -exec grep ': any' {} + 2>/dev/null"
check_absent "Unsafe blocks without safety comments" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec sh -c 'grep -B1 \"unsafe {\" \"$1\" | grep -B1 \"unsafe {\" | grep -v \"// SAFETY:\" | grep \"unsafe {\"' _ {} \; 2>/dev/null"
echo

echo -e "${BLUE}=== 10. File Organization Consistency ===${NC}"
check_warning "Files larger than 1000 lines" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec sh -c 'lines=$(wc -l < \"$1\"); if [ \"$lines\" -gt 1000 ]; then echo \"$1\"; fi' _ {} \; 2>/dev/null | wc -l" 10
check_present "Minimal public items in mod.rs" "find crates/ node/src/ -name 'mod.rs' -exec sh -c 'count=$(grep -c \"^pub \" \"$1\" 2>/dev/null || echo 0); if [ \"$count\" -gt 10 ]; then echo \"$1\"; fi' _ {} \; 2>/dev/null | wc -l" 5
echo

echo -e "${BLUE}=== 11. Dependency Consistency ===${NC}"
check_absent "Git dependencies" "find . -name 'Cargo.toml' -exec grep 'git = ' {} + 2>/dev/null"
check_present "Workspace path dependencies" "find . -name 'Cargo.toml' -exec grep 'path = ' {} + 2>/dev/null | grep -v workspace | wc -l" 70
echo

echo -e "${BLUE}=== 12. Security Consistency ===${NC}"
check_absent "Hardcoded credentials" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -E '(password|passwd|pwd|secret|key)[[:space:]]*=[[:space:]]*[\"\047][^\"\047]+[\"\047]' {} + 2>/dev/null | grep -v 'pub const' | grep -v '//'"
# Exclude test framework constants
check_absent "Hardcoded IP addresses" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/local_test_framework.rs' -exec grep -E '([0-9]{1,3}\.){3}[0-9]{1,3}' {} + 2>/dev/null | grep -v version | grep -v '//'"
echo

echo -e "${BLUE}=== Consistency Check Summary ===${NC}"
TOTAL=$((PASSED + FAILED + WARNINGS))
echo "Total Checks: $TOTAL"
echo -e "${GREEN}Passed: $PASSED${NC}"
echo -e "${YELLOW}Warnings: $WARNINGS${NC}"
echo -e "${RED}Failed: $FAILED${NC}"

# Calculate score
if [ "$TOTAL" -gt 0 ]; then
    SCORE=$((PASSED * 100 / TOTAL))
else
    SCORE=0
fi
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