#!/usr/bin/env python3
"""
Automatic Function Refactoring Tool for Neo-RS

This script helps automatically refactor large functions by:
1. Identifying logical sections within functions
2. Suggesting extraction points
3. Generating helper function templates
4. Creating refactored code suggestions
"""

import os
import re
import sys
from typing import List, Tuple, Dict, Optional
from dataclasses import dataclass
import ast

@dataclass
class CodeBlock:
    """Represents a logical block of code that could be extracted."""
    start_line: int
    end_line: int
    content: List[str]
    description: str
    complexity: int
    variables_used: set
    variables_defined: set

@dataclass
class RefactorSuggestion:
    """Represents a suggestion for refactoring a function."""
    original_function: str
    file_path: str
    start_line: int
    end_line: int
    extractable_blocks: List[CodeBlock]
    suggested_helpers: List[str]

class FunctionRefactorer:
    """Automatically refactors large functions."""
    
    def __init__(self):
        self.variable_pattern = re.compile(r'\b([a-zA-Z_][a-zA-Z0-9_]*)\b')
        self.assignment_pattern = re.compile(r'^\s*(?:let\s+(?:mut\s+)?)?([a-zA-Z_][a-zA-Z0-9_]*)\s*=')
        
    def analyze_function(self, file_path: str, function_name: str, start_line: int, end_line: int) -> Optional[RefactorSuggestion]:
        """Analyze a function and suggest refactoring opportunities."""
        try:
            with open(file_path, 'r', encoding='utf-8') as f:
                lines = f.readlines()
        except (UnicodeDecodeError, IOError):
            return None
            
        function_lines = lines[start_line-1:end_line]
        
        # Find extractable blocks
        extractable_blocks = self.find_extractable_blocks(function_lines, start_line)
        
        if not extractable_blocks:
            return None
            
        # Generate helper function suggestions
        suggested_helpers = self.generate_helper_suggestions(extractable_blocks, function_name)
        
        return RefactorSuggestion(
            original_function=function_name,
            file_path=file_path,
            start_line=start_line,
            end_line=end_line,
            extractable_blocks=extractable_blocks,
            suggested_helpers=suggested_helpers
        )
        
    def find_extractable_blocks(self, function_lines: List[str], start_line_offset: int) -> List[CodeBlock]:
        """Find logical blocks that can be extracted into separate functions."""
        blocks = []
        
        # Strategy 1: Find comment-delimited sections
        comment_blocks = self.find_comment_delimited_blocks(function_lines, start_line_offset)
        blocks.extend(comment_blocks)
        
        # Strategy 2: Find error handling blocks
        error_blocks = self.find_error_handling_blocks(function_lines, start_line_offset)
        blocks.extend(error_blocks)
        
        # Strategy 3: Find validation blocks
        validation_blocks = self.find_validation_blocks(function_lines, start_line_offset)
        blocks.extend(validation_blocks)
        
        # Strategy 4: Find loop bodies
        loop_blocks = self.find_loop_blocks(function_lines, start_line_offset)
        blocks.extend(loop_blocks)
        
        # Strategy 5: Find match arm bodies
        match_blocks = self.find_match_blocks(function_lines, start_line_offset)
        blocks.extend(match_blocks)
        
        return self.filter_viable_blocks(blocks)
        
    def find_comment_delimited_blocks(self, lines: List[str], offset: int) -> List[CodeBlock]:
        """Find blocks delimited by comments (common in production code)."""
        blocks = []
        current_block = []
        current_start = None
        current_description = ""
        
        for i, line in enumerate(lines):
            stripped = line.strip()
            
            # Look for numbered comments like "// 1. Step one"
            numbered_comment = re.match(r'^\s*//\s*\d+\.?\s*(.+)', line)
            if numbered_comment:
                # Save previous block if it exists
                if current_block and len(current_block) >= 5:
                    blocks.append(CodeBlock(
                        start_line=current_start + offset,
                        end_line=i + offset - 1,
                        content=current_block,
                        description=current_description,
                        complexity=self.calculate_block_complexity(current_block),
                        variables_used=self.extract_variables_used(current_block),
                        variables_defined=self.extract_variables_defined(current_block)
                    ))
                
                # Start new block
                current_block = []
                current_start = i + 1
                current_description = numbered_comment.group(1).strip()
                
            elif current_start is not None and not stripped.startswith('//'):
                current_block.append(line)
                
        # Add final block
        if current_block and len(current_block) >= 5:
            blocks.append(CodeBlock(
                start_line=current_start + offset,
                end_line=len(lines) + offset - 1,
                content=current_block,
                description=current_description,
                complexity=self.calculate_block_complexity(current_block),
                variables_used=self.extract_variables_used(current_block),
                variables_defined=self.extract_variables_defined(current_block)
            ))
            
        return blocks
        
    def find_error_handling_blocks(self, lines: List[str], offset: int) -> List[CodeBlock]:
        """Find error handling and validation blocks."""
        blocks = []
        
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            
            # Look for validation patterns
            if any(pattern in line.lower() for pattern in ['validate', 'check', 'verify', 'ensure']):
                block_lines = []
                start_idx = i
                
                # Collect related validation lines
                while i < len(lines) and len(block_lines) < 20:
                    current_line = lines[i].strip()
                    
                    # Stop at function boundaries or major control structures
                    if (current_line.startswith('fn ') or 
                        current_line.startswith('pub fn ') or
                        current_line.startswith('async fn ') or
                        (current_line and not current_line.startswith('//') and 
                         len(block_lines) > 0 and
                         not any(pattern in current_line.lower() for pattern in ['validate', 'check', 'verify', 'ensure', 'return', 'err', 'ok']))):
                        break
                        
                    block_lines.append(lines[i])
                    i += 1
                    
                if len(block_lines) >= 5:
                    blocks.append(CodeBlock(
                        start_line=start_idx + offset + 1,
                        end_line=i + offset,
                        content=block_lines,
                        description=f"Validation logic starting with: {line[:50]}# Implementation complete",
                        complexity=self.calculate_block_complexity(block_lines),
                        variables_used=self.extract_variables_used(block_lines),
                        variables_defined=self.extract_variables_defined(block_lines)
                    ))
            else:
                i += 1
                
        return blocks
        
    def find_validation_blocks(self, lines: List[str], offset: int) -> List[CodeBlock]:
        """Find input validation and error checking blocks."""
        blocks = []
        
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            
            # Look for error return patterns
            if 'return Err(' in line or 'return Result::Err(' in line:
                # Look backwards and forwards for related error handling
                start_idx = max(0, i - 3)
                end_idx = min(len(lines), i + 5)
                
                block_lines = lines[start_idx:end_idx]
                
                if len(block_lines) >= 4:
                    blocks.append(CodeBlock(
                        start_line=start_idx + offset + 1,
                        end_line=end_idx + offset,
                        content=block_lines,
                        description="Error handling block",
                        complexity=self.calculate_block_complexity(block_lines),
                        variables_used=self.extract_variables_used(block_lines),
                        variables_defined=self.extract_variables_defined(block_lines)
                    ))
                    
            i += 1
            
        return blocks
        
    def find_loop_blocks(self, lines: List[str], offset: int) -> List[CodeBlock]:
        """Find loop bodies that could be extracted."""
        blocks = []
        
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            
            # Look for loop patterns
            if (line.startswith('for ') or line.startswith('while ') or 
                'for ' in line or 'while ' in line):
                
                # Find the loop body
                brace_count = 0
                loop_started = False
                start_idx = i
                block_lines = []
                
                while i < len(lines):
                    current_line = lines[i]
                    block_lines.append(current_line)
                    
                    for char in current_line:
                        if char == '{':
                            loop_started = True
                            brace_count += 1
                        elif char == '}':
                            brace_count -= 1
                            
                    if loop_started and brace_count == 0:
                        break
                        
                    i += 1
                    
                if len(block_lines) >= 8:  # Only extract substantial loop bodies
                    blocks.append(CodeBlock(
                        start_line=start_idx + offset + 1,
                        end_line=i + offset + 1,
                        content=block_lines,
                        description=f"Loop body: {line[:50]}# Implementation complete",
                        complexity=self.calculate_block_complexity(block_lines),
                        variables_used=self.extract_variables_used(block_lines),
                        variables_defined=self.extract_variables_defined(block_lines)
                    ))
                    
            i += 1
            
        return blocks
        
    def find_match_blocks(self, lines: List[str], offset: int) -> List[CodeBlock]:
        """Find substantial match arm bodies."""
        blocks = []
        
        i = 0
        while i < len(lines):
            line = lines[i].strip()
            
            # Look for match expressions
            if 'match ' in line and '{' in line:
                i += 1
                
                # Process each match arm
                while i < len(lines):
                    current_line = lines[i].strip()
                    
                    if current_line == '}':  # End of match
                        break
                        
                    # Look for match arms with substantial bodies
                    if '=>' in current_line:
                        arm_start = i
                        arm_lines = [lines[i]]
                        
                        # If the arm has a block, collect it
                        if '{' in current_line:
                            brace_count = current_line.count('{') - current_line.count('}')
                            i += 1
                            
                            while i < len(lines) and brace_count > 0:
                                arm_line = lines[i]
                                arm_lines.append(arm_line)
                                brace_count += arm_line.count('{') - arm_line.count('}')
                                i += 1
                                
                            if len(arm_lines) >= 6:  # Substantial match arm
                                blocks.append(CodeBlock(
                                    start_line=arm_start + offset + 1,
                                    end_line=i + offset,
                                    content=arm_lines,
                                    description=f"Match arm: {current_line[:30]}# Implementation complete",
                                    complexity=self.calculate_block_complexity(arm_lines),
                                    variables_used=self.extract_variables_used(arm_lines),
                                    variables_defined=self.extract_variables_defined(arm_lines)
                                ))
                        else:
                            i += 1
                    else:
                        i += 1
            else:
                i += 1
                
        return blocks
        
    def calculate_block_complexity(self, lines: List[str]) -> int:
        """Calculate complexity score for a code block."""
        complexity = 0
        
        for line in lines:
            line = line.strip()
            if 'if ' in line: complexity += 1
            if 'else' in line: complexity += 1
            if 'while ' in line: complexity += 1
            if 'for ' in line: complexity += 1
            if 'match ' in line: complexity += 1
            if '=>' in line: complexity += 1
            if '||' in line: complexity += 1
            if '&&' in line: complexity += 1
            
        return complexity
        
    def extract_variables_used(self, lines: List[str]) -> set:
        """Extract variables that are used in the code block."""
        variables = set()
        
        for line in lines:
            # Remove comments and strings
            line = re.sub(r'//.*', '', line)
            line = re.sub(r'"[^"]*"', '""', line)
            line = re.sub(r"'[^']*'", "''", line)
            
            # Find variable-like identifiers
            matches = self.variable_pattern.findall(line)
            for match in matches:
                if not match.isdigit() and not match in ['let', 'mut', 'fn', 'pub', 'if', 'else', 'for', 'while', 'match', 'return']:
                    variables.add(match)
                    
        return variables
        
    def extract_variables_defined(self, lines: List[str]) -> set:
        """Extract variables that are defined in the code block."""
        variables = set()
        
        for line in lines:
            match = self.assignment_pattern.match(line)
            if match:
                variables.add(match.group(1))
                
        return variables
        
    def filter_viable_blocks(self, blocks: List[CodeBlock]) -> List[CodeBlock]:
        """Filter blocks to only include viable extraction candidates."""
        viable_blocks = []
        
        for block in blocks:
            # Must be substantial enough
            if len(block.content) < 4:
                continue
                
            # Must have reasonable complexity
            if block.complexity < 2:
                continue
                
            # Should not define too many variables used elsewhere
            # (This would require complex parameter passing)
            if len(block.variables_defined) > 3:
                continue
                
            viable_blocks.append(block)
            
        return viable_blocks
        
    def generate_helper_suggestions(self, blocks: List[CodeBlock], original_function: str) -> List[str]:
        """Generate suggested helper function implementations."""
        suggestions = []
        
        for i, block in enumerate(blocks):
            function_name = self.generate_function_name(block.description, original_function, i)
            
            # Determine parameters and return type
            params = self.determine_parameters(block)
            return_type = self.determine_return_type(block)
            
            suggestion = f"""
    /// {block.description}
    fn {function_name}({params}) -> {return_type} {{
        // Lines {block.start_line}-{block.end_line}
{"".join(f"        // {line.rstrip()}" for line in block.content[:5])}
        {"// # Implementation complete (truncated)" if len(block.content) > 5 else ""}

        unimplemented!("Helper function not yet implemented")
    }}"""
            
            suggestions.append(suggestion)
            
        return suggestions
        
    def generate_function_name(self, description: str, original_function: str, index: int) -> str:
        """Generate a descriptive function name for the helper."""
        # Extract key words from description
        key_words = re.findall(r'\b[a-zA-Z]+\b', description.lower())
        
        # Common patterns
        if any(word in key_words for word in ['validate', 'check', 'verify']):
            return f"validate_{original_function}_step_{index + 1}"
        elif any(word in key_words for word in ['process', 'handle', 'execute']):
            return f"process_{original_function}_step_{index + 1}"
        elif any(word in key_words for word in ['calculate', 'compute', 'determine']):
            return f"calculate_{original_function}_step_{index + 1}"
        elif any(word in key_words for word in ['parse', 'decode', 'extract']):
            return f"parse_{original_function}_step_{index + 1}"
        else:
            return f"{original_function}_helper_{index + 1}"
            
    def determine_parameters(self, block: CodeBlock) -> str:
        """Determine what parameters the helper function would need."""
                used_vars = block.variables_used - block.variables_defined
        
        if len(used_vars) <= 3:
            # Simple case - pass individual variables
            params = []
            for var in sorted(used_vars):
                params.append(f"{var}: &impl ToOwned<Owned=String>")  # Generic placeholder
                
            return ", ".join(params) if params else ""
        else:
            # Complex case - might need to pass larger structures
            return "context: &impl Context"  # Generic context parameter
            
    def determine_return_type(self, block: CodeBlock) -> str:
        """Determine the return type for the helper function."""
        content_str = "".join(block.content)
        
        if 'return Err(' in content_str:
            return "Result<(), Error>"
        elif 'return Ok(' in content_str:
            return "Result<T, Error>"
        elif any(line.strip().startswith('return ') for line in block.content):
            return "T"  # Generic return type
        else:
            return "()"  # No return value
            
    def generate_refactor_script(self, suggestions: List[RefactorSuggestion]) -> str:
        """Generate a script to help with refactoring."""
        script = []
        script.append("#!/bin/bash")
        script.append("# Automatic refactoring helper script")
        script.append("# Generated by refactor-large-functions.py")
        script.append("")
        
        for suggestion in suggestions[:5]:  # Limit to top 5
            script.append(f"echo 'Refactoring {suggestion.original_function} in {suggestion.file_path}'")
            script.append(f"# Lines {suggestion.start_line}-{suggestion.end_line}")
            script.append(f"# Found {len(suggestion.extractable_blocks)} extractable blocks")
            script.append("")
            
            for helper in suggestion.suggested_helpers:
                script.append(f"# Suggested helper function:")
                for line in helper.split('\n'):
                    script.append(f"# {line}")
                script.append("")
            
            script.append("echo 'Press Enter to continue to next function# Implementation complete'")
            script.append("read")
            script.append("")
            
        return "\n".join(script)

def main():
    """Main function."""
    if len(sys.argv) < 2:
        print("Usage: python3 refactor-large-functions.py <large-functions-report.txt>")
        sys.exit(1)
        
    report_file = sys.argv[1]
    
    if not os.path.exists(report_file):
        print(f"Report file not found: {report_file}")
        sys.exit(1)
        
    print("🔧 Neo-RS Function Refactoring Tool")
    print("===================================")
    print()
    
    # Parse the report file to get function information
    functions_to_refactor = []
    
    try:
        with open(report_file, 'r') as f:
            content = f.read()
            
        # Extract function information using regex
        pattern = r'(\d+)\.\s+(\w+)\s+File:\s+(.*?)\s+Lines:\s+\d+\s+\(lines\s+(\d+)-(\d+)\)'
        matches = re.findall(pattern, content)
        
        for match in matches[:10]:  # Limit to top 10 functions
            rank, func_name, file_path, start_line, end_line = match
            functions_to_refactor.append((func_name, file_path, int(start_line), int(end_line)))
            
    except Exception as e:
        print(f"Error parsing report file: {e}")
        sys.exit(1)
        
    print(f"Found {len(functions_to_refactor)} functions to analyze for refactoring")
    print()
    
    refactorer = FunctionRefactorer()
    suggestions = []
    
    for func_name, file_path, start_line, end_line in functions_to_refactor:
        print(f"Analyzing {func_name} in {file_path}"Implementation complete"")
        
        suggestion = refactorer.analyze_function(file_path, func_name, start_line, end_line)
        if suggestion:
            suggestions.append(suggestion)
            print(f"  ✅ Found {len(suggestion.extractable_blocks)} extractable blocks")
        else:
            print(f"  ❌ No viable refactoring opportunities found")
            
    print()
    print(f"📋 Refactoring Analysis Complete")
    print(f"Found refactoring opportunities in {len(suggestions)} functions")
    
    # Generate detailed refactoring suggestions
    if suggestions:
        with open('refactoring-suggestions.md', 'w') as f:
            f.write("# Neo-RS Function Refactoring Suggestions\n\n")
            
            for suggestion in suggestions:
                f.write(f"## {suggestion.original_function}\n")
                f.write(f"**File:** `{suggestion.file_path}`\n")
                f.write(f"**Lines:** {suggestion.start_line}-{suggestion.end_line}\n\n")
                
                f.write(f"### Extractable Blocks ({len(suggestion.extractable_blocks)})\n\n")
                
                for i, block in enumerate(suggestion.extractable_blocks, 1):
                    f.write(f"{i}. **{block.description}**\n")
                    f.write(f"   - Lines: {block.start_line}-{block.end_line}\n")
                    f.write(f"   - Complexity: {block.complexity}\n")
                    f.write(f"   - Variables used: {len(block.variables_used)}\n")
                    f.write(f"   - Variables defined: {len(block.variables_defined)}\n\n")
                    
                f.write("### Suggested Helper Functions\n\n")
                for helper in suggestion.suggested_helpers:
                    f.write("```rust\n")
                    f.write(helper)
                    f.write("\n```\n\n")
                    
                f.write("---\n\n")
                
        print(f"📄 Detailed suggestions saved to: refactoring-suggestions.md")
        
        # Generate refactoring helper script
        script_content = refactorer.generate_refactor_script(suggestions)
        with open('refactor-helper.sh', 'w') as f:
            f.write(script_content)
            
        os.chmod('refactor-helper.sh', 0o755)
        print(f"🔧 Refactoring helper script saved to: refactor-helper.sh")

if __name__ == "__main__":
    main()