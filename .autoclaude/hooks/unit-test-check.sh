#!/bin/bash

# Unit Test Coverage Check Hook for AutoClaude
# This hook automatically runs unit test coverage analysis when triggered

# Run the unit test coverage check
echo "🧪 Running unit test coverage analysis..."
./.autoclaude/scripts/unit-test-coverage-check.sh

# Check if coverage is acceptable
if [[ -f "test_coverage_report.json" ]]; then
    coverage=$(grep -o '"overall_coverage_percent": [0-9]*' test_coverage_report.json | grep -o '[0-9]*')
    
    if [[ $coverage -ge 80 ]]; then
        echo "✅ Unit test coverage is excellent ($coverage%)"
        exit 0
    elif [[ $coverage -ge 60 ]]; then
        echo "⚠️  Unit test coverage is good but can be improved ($coverage%)"
        exit 0
    else
        echo "❌ Unit test coverage is below acceptable threshold ($coverage%)"
        echo "   Please convert more C# tests to Rust before proceeding"
        exit 1
    fi
else
    echo "❌ Failed to generate coverage report"
    exit 1
fi