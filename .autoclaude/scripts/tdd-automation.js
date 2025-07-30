#!/usr/bin/env node
const fs = require('fs');
const path = require('path');
const { execSync } = require('child_process');

async function check() {
    const errors = [];
    const warnings = [];
    
    try {
        // Check for test coverage
        if (fs.existsSync('coverage/coverage-summary.json')) {
            const coverageData = fs.readFileSync('coverage/coverage-summary.json', 'utf8');
            const summary = JSON.parse(coverageData);
            const coverage = summary.total.lines.pct;
            
            if (coverage < 80) {
                errors.push(`Test coverage (${coverage}%) is below minimum (80%)`);
            }
        } else {
            warnings.push('No test coverage data found - run tests with coverage first');
        }
        
        // Basic check for test files
        const hasTests = fs.existsSync('test') || fs.existsSync('tests') || 
                        fs.existsSync('__tests__') || fs.existsSync('src/__tests__');
        
        if (!hasTests) {
            errors.push('No test directory found');
        }
        
    } catch (error) {
        errors.push(`TDD check failed: ${error.message}`);
    }
    
    const result = {
        passed: errors.length === 0,
        errors: errors,
        warnings: warnings
    };
    
    console.log(JSON.stringify(result, null, 2));
}

check().catch(error => {
    console.error(JSON.stringify({
        passed: false,
        errors: [`TDD automation error: ${error.message}`]
    }, null, 2));
    process.exit(1);
});