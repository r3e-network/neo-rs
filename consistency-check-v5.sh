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

# Helper to count production code unwraps using Python for accuracy
count_production_unwraps() {
    python3 -c "
import os
import re

total = 0
for root, dirs, files in os.walk('crates'):
    # Skip test directories
    dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches']]
    
    for file in files:
        if file.endswith('.rs') and not file.endswith('_test.rs') and not file.endswith('_tests.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                    # Skip if entire file is a test module
                    if '#[cfg(test)]' in content[:100]:
                        continue
                    # Remove test modules from content
                    content = re.sub(r'#\[cfg\(test\)\].*?mod\s+tests?\s*\{.*?\n\}', '', content, flags=re.DOTALL)
                    # Count unwraps not in comments
                    lines = content.split('\n')
                    for line in lines:
                        if '.unwrap()' in line and not line.strip().startswith('//'):
                            total += line.count('.unwrap()')
            except:
                pass

for root, dirs, files in os.walk('node'):
    dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches']]
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                    content = re.sub(r'#\[cfg\(test\)\].*?mod\s+tests?\s*\{.*?\n\}', '', content, flags=re.DOTALL)
                    lines = content.split('\n')
                    for line in lines:
                        if '.unwrap()' in line and not line.strip().startswith('//'):
                            total += line.count('.unwrap()')
            except:
                pass

print(total)
"
}

# Count wildcard imports excluding test modules
count_production_wildcards() {
    python3 -c "
import os
import re

total = 0
for root, dirs, files in os.walk('crates'):
    dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
    
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                    
                # Remove all test modules before checking
                content = re.sub(r'#\[cfg\(test\)\]\s*mod\s+tests?\s*\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}', '', content, flags=re.DOTALL)
                
                # Now check for wildcard imports
                lines = content.split('\n')
                for line in lines:
                    if re.search(r'use\s+.*::\*;', line) and not line.strip().startswith('//'):
                        total += 1
            except:
                pass

for root, dirs, files in os.walk('node'):
    dirs[:] = [d for d in dirs if d not in ['tests', 'test', 'examples', 'benches', '.git', 'target']]
    for file in files:
        if file.endswith('.rs'):
            filepath = os.path.join(root, file)
            try:
                with open(filepath, 'r') as f:
                    content = f.read()
                content = re.sub(r'#\[cfg\(test\)\]\s*mod\s+tests?\s*\{[^{}]*(?:\{[^{}]*\}[^{}]*)*\}', '', content, flags=re.DOTALL)
                lines = content.split('\n')
                for line in lines:
                    if re.search(r'use\s+.*::\*;', line) and not line.strip().startswith('//'):
                        total += 1
            except:
                pass

print(total)
"
}

# Helper functions
check_present() {
    local description="$1"
    local command="$2"
    local max_allowed="$3"
    
    count=$(eval "$command" 2>/dev/null | tr -d ' ')
    if [ -z "$count" ]; then
        count=0
    fi
    
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

echo "=== Neo-RS Codebase Consistency Check v5 (with Duplication Detection) ==="
echo "Timestamp: $(date)"
echo

echo -e "${BLUE}=== 1. Debug Statement Consistency ===${NC}"
check_absent "No println! statements in production code" "find crates/ node/src/ -name '*.rs' -type f | xargs grep -l 'println!' 2>/dev/null | xargs grep 'println!' | grep -v '//' | grep -v 'eprintln' | grep -v -E '(test|example|cli/src/console|cli/src/main)'"
check_absent "No dbg! statements in production code" "find crates/ node/src/ -name '*.rs' -type f | xargs grep 'dbg!' 2>/dev/null | grep -v '//' | grep -v -E '(test|example)'"
check_absent "No print! statements in production code" "find crates/ node/src/ -name '*.rs' -type f | xargs grep 'print!' 2>/dev/null | grep -v 'println' | grep -v '//' | grep -v -E '(test|example|cli/src/console)'"
echo

echo -e "${BLUE}=== 2. Error Handling Consistency ===${NC}"
check_absent "No panic! in production code" "find crates/ node/src/ -name '*.rs' -type f | xargs grep 'panic!' 2>/dev/null | grep -v -E '(unimplemented!|unreachable!|assert!|test|example|//|todo!())'"
check_present "Minimal unwrap() in production code" "count_production_unwraps" 150
check_absent "No expect(\"\") without context" "find crates/ node/src/ -name '*.rs' -type f | xargs grep '\.expect(\"\")' 2>/dev/null | grep -v -E '(test|example)'"
echo

echo -e "${BLUE}=== 3. TODO and Production implementation Consistency ===${NC}"
check_present "TODO comments" "find crates/ node/src/ -name '*.rs' -type f | xargs grep 'TODO' 2>/dev/null | grep -v -E '(test|example)' | wc -l" 5
check_absent "FIXME comments" "find crates/ node/src/ -name '*.rs' -type f | xargs grep 'FIXME' 2>/dev/null | grep -v -E '(test|example)'"
check_absent "XXX comments" "find crates/ node/src/ -name '*.rs' -type f | xargs grep 'XXX' 2>/dev/null | grep -v -E '(test|example)'"
check_absent "HACK comments" "find crates/ node/src/ -name '*.rs' -type f | xargs grep 'HACK' 2>/dev/null | grep -v -E '(test|example)'"
echo

echo -e "${BLUE}=== 4. Code Quality Consistency ===${NC}"
check_absent "Commented out code" "find crates/ node/src/ -name '*.rs' -type f | xargs grep -E '^[[:space:]]*//[[:space:]]*(let|const|fn|impl|struct|enum|if|while|for)[[:space:]]' 2>/dev/null | grep -v -E '(test|example)'"
check_warning "Functions longer than 100 lines" "find crates/ node/src/ -name '*.rs' -type f -exec awk '/^[[:space:]]*pub[[:space:]]+fn|^[[:space:]]*fn/ {start=NR; name=\$0} /^}/ {if (NR-start > 100) print FILENAME\":\"start\"-\"NR}' {} \; 2>/dev/null | grep -v test" 50
check_absent "Multiple empty lines" "find crates/ node/src/ -name '*.rs' -type f -not -path '*/tests/*' -not -path '*/test/*' -exec awk '/^$/{if(p){print FILENAME\":\"NR} p=1; next} {p=0}' {} \; 2>/dev/null"
echo

echo -e "${BLUE}=== 5. Import Consistency ===${NC}"
# Check for wildcard imports NOT in test modules  
check_present "Wildcard imports in production" "count_production_wildcards" 0
check_absent "Unused imports" "find crates/ node/src/ -name '*.rs' -type f | xargs grep '#\[allow(unused_imports)\]' 2>/dev/null"
echo

echo -e "${BLUE}=== 6. Magic Number Consistency ===${NC}"
check_absent "Magic number 15" "find crates/ node/src/ -name '*.rs' -type f | xargs grep -w '15' 2>/dev/null | grep -v -E '(0x15|u15|i15|//|format!|println!|{:.*15|15}|\".*15.*\"|test|Duration::from)'"
check_absent "Magic number 262144" "find crates/ node/src/ -name '*.rs' -type f | xargs grep -w '262144' 2>/dev/null | grep -v -E '(MAX_BLOCK_SIZE|test)'"
check_absent "Magic number 102400" "find crates/ node/src/ -name '*.rs' -type f | xargs grep -w '102400' 2>/dev/null | grep -v -E '(MAX_TRANSACTION_SIZE|test)'"
echo

echo -e "${BLUE}=== 7. Naming Consistency ===${NC}"
check_absent "Snake case violations in function names" "find crates/ node/src/ -name '*.rs' -type f | xargs grep 'fn [A-Z]' 2>/dev/null | grep -v -E '(trait|test)'"
check_absent "CamelCase violations in variable names" "find crates/ node/src/ -name '*.rs' -type f | xargs grep -E 'let [a-z]+[A-Z]+[a-zA-Z]*[[:space:]]*=' 2>/dev/null | grep -v -E '(Some|None|Ok|Err|test)'"
echo

echo -e "${BLUE}=== 8. Documentation Consistency ===${NC}"
# Reasonable thresholds for a large codebase
check_warning "Public functions without documentation" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -B1 '^pub fn' {} \; 2>/dev/null | grep -B1 '^pub fn' | grep -v '^///' | grep '^pub fn' | wc -l" 100
check_warning "Public structs without documentation" "find crates/ node/src/ -name '*.rs' -not -path '*/tests/*' -not -path '*/test/*' -exec grep -B1 '^pub struct' {} \; 2>/dev/null | grep -B1 '^pub struct' | grep -v '^///' | grep '^pub struct' | wc -l" 50
echo

echo -e "${BLUE}=== 9. Type Safety Consistency ===${NC}"
check_absent "Use of 'any' type" "find crates/ node/src/ -name '*.ts' -o -name '*.tsx' | xargs grep ': any' 2>/dev/null"
check_absent "Unsafe blocks without safety comments" "find crates/ node/src/ -name '*.rs' -type f -not -path '*/tests/*' -not -path '*/test/*' -exec awk '/unsafe[[:space:]]*\{/ {if (prev !~ /SAFETY/) print FILENAME\":\"NR\": \"$0} {prev=$0}' {} \; 2>/dev/null"
echo

echo -e "${BLUE}=== 10. File Organization Consistency ===${NC}"
check_warning "Files larger than 1000 lines" "find crates/ node/src/ -name '*.rs' -type f -exec sh -c 'lines=\$(wc -l < \"\$1\"); if [ \"\$lines\" -gt 1000 ]; then echo \"\$1\"; fi' _ {} \; 2>/dev/null | grep -v test | wc -l" 10
check_present "Minimal public items in mod.rs" "find crates/ node/src/ -name 'mod.rs' -exec sh -c 'count=\$(grep -c \"^[[:space:]]*pub[[:space:]]\" \"\$1\" 2>/dev/null || echo 0); if [ \"\$count\" -gt 10 ]; then echo \"\$1\"; fi' _ {} \; 2>/dev/null | wc -l" 5
echo

echo -e "${BLUE}=== 11. Dependency Consistency ===${NC}"
check_absent "Git dependencies" "grep -r 'git[[:space:]]*=' Cargo.toml */Cargo.toml 2>/dev/null"
check_present "Workspace path dependencies" "grep -r 'path[[:space:]]*=' Cargo.toml */Cargo.toml 2>/dev/null | grep -v workspace | wc -l" 70
echo

echo -e "${BLUE}=== 12. Security Consistency ===${NC}"
check_absent "Hardcoded credentials" "find crates/ node/src/ -name '*.rs' -type f | xargs grep -E '(password|passwd|pwd|secret|key)[[:space:]]*=[[:space:]]*[\"\\047][^\"\\047]+[\"\\047]' 2>/dev/null | grep -v -E '(pub const|test|//|example)'"
check_absent "Hardcoded IP addresses" "find crates/ node/src/ -name '*.rs' -type f | xargs grep -E '([0-9]{1,3}\.){3}[0-9]{1,3}' 2>/dev/null | grep -v -E '(version|test|example|//|local_test_framework)'"
echo

echo -e "${BLUE}=== 13. Code Duplication ===${NC}"
check_absent "Duplicate imports" "find crates/ node/src/ -name '*.rs' -type f -exec sh -c 'grep \"^use \" \"\$1\" 2>/dev/null | sort | uniq -d' _ {} \;"
check_warning "Duplicate constants across files" "grep -r '^const\\|^pub const' crates/ node/src/ --include='*.rs' 2>/dev/null | cut -d':' -f2- | sort | uniq -d | wc -l" 10
check_warning "Common function names (potential duplication)" "find crates/ node/src/ -name '*.rs' -type f -exec grep -E '^[[:space:]]*(pub[[:space:]]+)?fn[[:space:]]+' {} \; 2>/dev/null | sed 's/.*fn[[:space:]]*\\([a-zA-Z0-9_]*\\).*/\\1/' | sort | uniq -c | awk '\$1 > 20 {print}' | wc -l" 20
echo

echo -e "${BLUE}=== Consistency Check Summary ===${NC}"
TOTAL=$((PASSED + FAILED + WARNINGS))
echo "Total Checks: 33"
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