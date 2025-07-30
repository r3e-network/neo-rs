#!/usr/bin/env node
const fs = require('fs');
const path = require('path');

async function check() {
    const errors = [];
    const warnings = [];
    
    try {
        // Find code files
        const files = findCodeFiles(process.cwd());
        
        for (const file of files) {
            const content = fs.readFileSync(file, 'utf8');
            const relativePath = path.relative(process.cwd(), file);
            
            // Security checks
            if (/eval\s*\(/.test(content)) {
                errors.push(`${relativePath}: Dangerous use of eval()`);
            }
            if (/password.*=.*["'][^"']+["']/i.test(content)) {
                errors.push(`${relativePath}: Hardcoded password detected`);
            }
            
            // Performance checks
            if (/for\s*\([^)]*\)\s*{[^}]*for\s*\([^)]*\)\s*{/.test(content)) {
                warnings.push(`${relativePath}: Nested loops detected - potential O(n²) complexity`);
            }
            
            // Best practices
            if (/console\.(log|debug)/.test(content)) {
                warnings.push(`${relativePath}: Console statements found`);
            }
        }
        
    } catch (error) {
        errors.push(`AI code review failed: ${error.message}`);
    }
    
    const result = {
        passed: errors.length === 0,
        errors: errors,
        warnings: warnings
    };
    
    console.log(JSON.stringify(result, null, 2));
}

function findCodeFiles(dir) {
    const files = [];
    const extensions = ['.js', '.ts', '.jsx', '.tsx'];
    const skipDirs = ['.git', 'node_modules', 'dist', 'build', 'coverage', '.autoclaude'];
    
    function scan(currentDir) {
        const items = fs.readdirSync(currentDir);
        
        for (const item of items) {
            const fullPath = path.join(currentDir, item);
            const stat = fs.statSync(fullPath);
            
            if (stat.isDirectory()) {
                if (!skipDirs.includes(item)) {
                    scan(fullPath);
                }
            } else if (stat.isFile()) {
                if (extensions.includes(path.extname(item))) {
                    files.push(fullPath);
                }
            }
        }
    }
    
    scan(dir);
    return files;
}

check().catch(error => {
    console.error(JSON.stringify({
        passed: false,
        errors: [`AI code review error: ${error.message}`]
    }, null, 2));
    process.exit(1);
});