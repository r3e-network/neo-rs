#!/usr/bin/env python3
"""
Find Large Functions Script for Neo-RS

This script identifies functions that are too large and should be split
for better maintainability and code quality.
"""

import os
import re
import sys
from typing import List, Tuple, Dict
from dataclasses import dataclass

@dataclass
class FunctionInfo:
    """Information about a function."""
    name: str
    file_path: str
    start_line: int
    end_line: int
    line_count: int
    complexity_score: int

class LargeFunctionFinder:
    """Finds large functions in Rust code."""
    
    def __init__(self, max_lines: int = 50, max_complexity: int = 10):
        self.max_lines = max_lines
        self.max_complexity = max_complexity
        self.large_functions: List[FunctionInfo] = []
        
    def analyze_file(self, file_path: str) -> List[FunctionInfo]:
        """Analyze a single Rust file for large functions."""
        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                content = f.read()
        except (UnicodeDecodeError, IOError):
            return []
            
        functions = []
        lines = content.split('\n')
        
        # Find function definitions
        function_pattern = re.compile(r'^\s*(pub\s+)?(async\s+)?fn\s+(\w+)(?:<[^>]*>)?\s*\([^)]*\)', re.MULTILINE)
        
        i = 0
        while i < len(lines):
            line = lines[i]
            
            # Skip comments and empty lines
            if line.strip().startswith('//') or line.strip().startswith('/*') or not line.strip():
                i += 1
                continue
                
            # Look for function definitions
            match = function_pattern.match(line)
            if match:
                func_name = match.group(3)
                start_line = i + 1
                
                # Skip test functions
                if func_name.startswith('test_') or 'test' in func_name.lower():
                    i += 1
                    continue
                
                # Find the end of the function
                brace_count = 0
                func_started = False
                end_line = start_line
                
                j = i
                while j < len(lines):
                    current_line = lines[j]
                    
                    # Count braces to find function end
                    for char in current_line:
                        if char == '{':
                            func_started = True
                            brace_count += 1
                        elif char == '}':
                            brace_count -= 1
                            
                    if func_started and brace_count == 0:
                        end_line = j + 1
                        break
                        
                    j += 1
                
                # Calculate metrics
                line_count = end_line - start_line + 1
                complexity_score = self.calculate_complexity(lines[i:j+1])
                
                if line_count > self.max_lines or complexity_score > self.max_complexity:
                    func_info = FunctionInfo(
                        name=func_name,
                        file_path=file_path,
                        start_line=start_line,
                        end_line=end_line,
                        line_count=line_count,
                        complexity_score=complexity_score
                    )
                    functions.append(func_info)
                    
                i = j + 1
            else:
                i += 1
                
        return functions
        
    def calculate_complexity(self, function_lines: List[str]) -> int:
        """Calculate cyclomatic complexity of a function."""
        complexity = 1  # Base complexity
        
        for line in function_lines:
            line = line.strip()
            
            # Count decision points
            if re.search(r'\bif\b', line):
                complexity += 1
            if re.search(r'\belse\s+if\b', line):
                complexity += 1
            if re.search(r'\bwhile\b', line):
                complexity += 1
            if re.search(r'\bfor\b', line):
                complexity += 1
            if re.search(r'\bmatch\b', line):
                complexity += 1
            if re.search(r'=>', line) and not line.startswith('//'):
                complexity += 1
            if re.search(r'\|\|', line):
                complexity += 1
            if re.search(r'&&', line):
                complexity += 1
                
        return complexity
        
    def analyze_directory(self, directory: str) -> None:
        """Analyze all Rust files in a directory."""
        for root, dirs, files in os.walk(directory):
            # Skip target directory and hidden directories
            dirs[:] = [d for d in dirs if not d.startswith('.') and d != 'target']
            
            for file in files:
                if file.endswith('.rs'):
                    file_path = os.path.join(root, file)
                    functions = self.analyze_file(file_path)
                    self.large_functions.extend(functions)
                    
    def generate_report(self) -> str:
        """Generate a report of large functions."""
        if not self.large_functions:
            return "✅ No large functions found!"
            
        # Sort by line count (largest first)
        sorted_functions = sorted(self.large_functions, key=lambda f: f.line_count, reverse=True)
        
        report = []
        report.append("🔍 Large Functions Analysis Report")
        report.append("=" * 50)
        report.append(f"Found {len(sorted_functions)} large functions")
        report.append(f"Criteria: >{self.max_lines} lines OR >{self.max_complexity} complexity")
        report.append("")
        
        for i, func in enumerate(sorted_functions, 1):
            report.append(f"{i}. {func.name}")
            report.append(f"   File: {func.file_path}")
            report.append(f"   Lines: {func.line_count} (lines {func.start_line}-{func.end_line})")
            report.append(f"   Complexity: {func.complexity_score}")
            report.append("")
            
        return "\n".join(report)
        
    def generate_refactor_suggestions(self) -> str:
        """Generate refactoring suggestions for large functions."""
        if not self.large_functions:
            return ""
            
        suggestions = []
        suggestions.append("💡 Refactoring Suggestions")
        suggestions.append("=" * 30)
        suggestions.append("")
        
        for func in sorted(self.large_functions, key=lambda f: f.line_count, reverse=True)[:10]:
            suggestions.append(f"Function: {func.name} ({func.line_count} lines)")
            suggestions.append(f"File: {func.file_path}:{func.start_line}")
            suggestions.append("")
            suggestions.append("Suggested refactoring strategies:")
            
            if func.line_count > 100:
                suggestions.append("  - Extract multiple helper functions")
                suggestions.append("  - Consider splitting into a separate module")
            elif func.line_count > 75:
                suggestions.append("  - Extract 2-3 helper functions")
                suggestions.append("  - Group related logic into separate methods")
            else:
                suggestions.append("  - Extract 1-2 helper functions")
                suggestions.append("  - Move complex logic to separate methods")
                
            if func.complexity_score > 15:
                suggestions.append("  - Reduce nested conditions with early returns")
                suggestions.append("  - Consider using match expressions instead of if-else chains")
                suggestions.append("  - Extract decision logic into separate functions")
                
            suggestions.append("")
            suggestions.append("-" * 40)
            suggestions.append("")
            
        return "\n".join(suggestions)

def main():
    """Main function."""
    if len(sys.argv) > 1:
        directory = sys.argv[1]
    else:
        directory = "."
        
    if len(sys.argv) > 2:
        max_lines = int(sys.argv[2])
    else:
        max_lines = 50
        
    if len(sys.argv) > 3:
        max_complexity = int(sys.argv[3])
    else:
        max_complexity = 10
        
    print(f"🔍 Analyzing Rust code in: {directory}")
    print(f"📏 Max lines threshold: {max_lines}")
    print(f"🧠 Max complexity threshold: {max_complexity}")
    print()
    
    finder = LargeFunctionFinder(max_lines=max_lines, max_complexity=max_complexity)
    finder.analyze_directory(directory)
    
    # Generate and display report
    report = finder.generate_report()
    print(report)
    
    # Generate refactoring suggestions
    suggestions = finder.generate_refactor_suggestions()
    if suggestions:
        print()
        print(suggestions)
        
    # Save detailed report to file
    with open('large-functions-report.txt', 'w') as f:
        f.write(report)
        if suggestions:
            f.write("\n\n")
            f.write(suggestions)
            
    print(f"📄 Detailed report saved to: large-functions-report.txt")

if __name__ == "__main__":
    main()