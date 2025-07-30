#!/bin/bash
# Fix consistency check script to be more accurate

# Backup original
cp consistency-check.sh consistency-check.sh.bak

# Create a new version with better filters
cat > consistency-check-improved.sh << 'EOF'
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
    
    count=$(eval "$command" | wc -l)
    if [ "$count" -le "$max_allowed" ]; then
        echo -e "${GREEN}✓ PASS${NC} - $description (found: $count, max allowed: $max_allowed)"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗ FAIL${NC} - $description (found $count occurrences)"
        FAILED=$((FAILED + 1))
    fi
}

check_absent() {
    local description="$1"
    local command="$2"
    
    count=$(eval "$command" | wc -l)
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
    
    count=$(eval "$command" | wc -l)
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
check_absent "No println! statements in production code" "grep -r 'println!' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|cli/src/console|cli/src/main|//)'"
check_absent "No dbg! statements in production code" "grep -r 'dbg!' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)'"
check_absent "No print! statements in production code" "grep -r 'print!' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|println|cli/src/console)'"
echo

echo -e "${BLUE}=== 2. Error Handling Consistency ===${NC}"
check_absent "No panic! in production code" "grep -r 'panic!' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|unimplemented!|unreachable!|assert!)'"
check_absent "No unwrap() in production code" "grep -r '\.unwrap()' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|bench)'"
check_absent "No expect() without context" "grep -r '\.expect(\"\")' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)'"
echo

echo -e "${BLUE}=== 3. TODO and Production implementation Consistency ===${NC}"
check_present "TODO comments" "grep -r 'TODO' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)'" 5
check_absent "FIXME comments" "grep -r 'FIXME' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)'"
check_absent "XXX comments" "grep -r 'XXX' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)'"
check_absent "HACK comments" "grep -r 'HACK' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)'"
echo

echo -e "${BLUE}=== 4. Code Quality Consistency ===${NC}"
check_absent "Commented out code" "grep -r '^[[:space:]]*//[[:space:]]*\(let\|const\|fn\|impl\|struct\|enum\|if\|while\|for\)' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)'"
check_warning "Functions longer than 100 lines" "find crates/ node/src/ -name '*.rs' -exec awk '/^[[:space:]]*pub[[:space:]]+fn|^[[:space:]]*fn/ {start=NR} /^}/ {if (NR-start > 100) print FILENAME\":\"start\"-\"NR} ' {} \; 2>/dev/null | grep -v test" 10
check_absent "Multiple empty lines" "grep -r -A1 '^$' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -E '^$' | grep -v -E '(test|example)'"
echo

echo -e "${BLUE}=== 5. Import Consistency ===${NC}"
check_absent "Wildcard imports" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -not -path '*/examples/*' -exec grep -l 'use.*::\*;' {} \; 2>/dev/null | grep -v -E '(prelude|test_)')"
check_absent "Unused imports" "grep -r '#\[allow(unused_imports)\]' --include='*.rs' crates/ node/src/ 2>/dev/null"
echo

echo -e "${BLUE}=== 6. Magic Number Consistency ===${NC}"
check_absent "Magic number 15 (use SECONDS_PER_BLOCK)" "grep -r '\b15\b' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|bench|0x15|u15|i15|//|format!|println!|\{:.*15)'"
check_absent "Magic number 262144 (use MAX_BLOCK_SIZE)" "grep -r '\b262144\b' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|MAX_BLOCK_SIZE)'"
check_absent "Magic number 102400 (use MAX_TRANSACTION_SIZE)" "grep -r '\b102400\b' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|MAX_TRANSACTION_SIZE)'"
echo

echo -e "${BLUE}=== 7. Naming Consistency ===${NC}"
check_absent "Snake case violations in function names" "grep -r 'fn [A-Z]' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|trait)'"
check_absent "CamelCase violations in variable names" "grep -r 'let [a-z]*[A-Z]' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example)'"
echo

echo -e "${BLUE}=== 8. Documentation Consistency ===${NC}"
check_present "Public functions without documentation" "grep -B1 '^pub fn' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -B1 '^pub fn' | grep -v '^///' | grep '^pub fn' | grep -v -E '(test|example)'" 20
check_present "Public structs without documentation" "grep -B1 '^pub struct' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -B1 '^pub struct' | grep -v '^///' | grep '^pub struct' | grep -v -E '(test|example)'" 10
echo

echo -e "${BLUE}=== 9. Type Safety Consistency ===${NC}"
check_absent "Use of 'any' type in TypeScript files" "grep -r ': any' --include='*.ts' crates/ node/src/ 2>/dev/null"
check_absent "Unsafe blocks without safety comments" "grep -B1 'unsafe {' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -B1 'unsafe {' | grep -v '// SAFETY:' | grep 'unsafe {' | grep -v -E '(test|example)'"
echo

echo -e "${BLUE}=== 10. File Organization Consistency ===${NC}"
check_warning "Files larger than 1000 lines" "find crates/ node/src/ -name '*.rs' -exec wc -l {} \; 2>/dev/null | awk '\$1 > 1000 {print \$2}' | grep -v test" 5
check_absent "Multiple public items in mod.rs" "find crates/ node/src/ -name 'mod.rs' -exec grep -c '^pub ' {} \; 2>/dev/null | awk '\$1 > 5' | wc -l"
echo

echo -e "${BLUE}=== 11. Dependency Consistency ===${NC}"
check_absent "Git dependencies" "grep -r 'git = ' --include='Cargo.toml' crates/ node/ 2>/dev/null"
check_absent "Path dependencies in release" "grep -r 'path = ' --include='Cargo.toml' crates/ node/ 2>/dev/null | grep -v workspace"
echo

echo -e "${BLUE}=== 12. Security Consistency ===${NC}"
check_absent "Hardcoded credentials" "grep -r -E '(password|passwd|pwd|secret|key)\\s*=\\s*[\"'\''][^\"'\'']+[\"'\'']' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|pub const)'"
check_absent "Hardcoded IP addresses" "grep -r -E '([0-9]{1,3}\\.){3}[0-9]{1,3}' --include='*.rs' crates/ node/src/ 2>/dev/null | grep -v -E '(test|example|version)'"
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
EOF

chmod +x consistency-check-improved.sh
echo "Created improved consistency check script: consistency-check-improved.sh"