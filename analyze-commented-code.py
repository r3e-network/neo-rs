#!/usr/bin/env python3
"""Analyze commented code to identify what can be safely removed."""

import os
import re
import glob
from collections import defaultdict

def analyze_comments(file_path):
    """Analyze comments in a file."""
    try:
        with open(file_path, 'r', encoding='utf-8') as f:
            lines = f.readlines()
        
        comment_types = defaultdict(int)
        total_comments = 0
        
        for i, line in enumerate(lines):
            stripped = line.strip()
            
            # Skip empty lines
            if not stripped:
                continue
            
            # Documentation comments (keep these)
            if stripped.startswith('///') or stripped.startswith('//!'):
                comment_types['documentation'] += 1
                continue
            
            # Regular comments
            if stripped.startswith('//'):
                total_comments += 1
                comment_content = stripped[2:].strip()
                
                # Categorize comment
                if not comment_content:
                    comment_types['empty'] += 1
                elif any(keyword in comment_content.lower() for keyword in ['todo', 'fixme', 'hack', 'xxx']):
                    comment_types['task_markers'] += 1
                elif any(keyword in comment_content.lower() for keyword in ['note:', 'important:', 'warning:']):
                    comment_types['important_notes'] += 1
                elif re.match(r'^(Step |[0-9]+\.|[0-9]+\))', comment_content):
                    comment_types['numbered_steps'] += 1
                elif len(comment_content) > 80:
                    comment_types['long_explanations'] += 1
                elif re.match(r'^(let|const|mut|fn|pub|use|impl|struct|enum|trait|if|else|while|for|match|return)', comment_content):
                    comment_types['likely_code'] += 1
                elif comment_content.endswith(';') or comment_content.endswith('{') or comment_content.endswith('}'):
                    comment_types['likely_code'] += 1
                else:
                    comment_types['short_notes'] += 1
        
        return total_comments, comment_types
    
    except Exception as e:
        print(f"Error analyzing {file_path}: {e}")
        return 0, {}

def main():
    """Main function to analyze comments."""
    total_files = 0
    total_comments = 0
    all_comment_types = defaultdict(int)
    
    # Skip test files
    for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
        for file_path in glob.glob(pattern, recursive=True):
            if os.path.isfile(file_path) and not any(skip in file_path for skip in ['test', '/tests/', '/examples/', 'bench']):
                comments, types = analyze_comments(file_path)
                if comments > 0:
                    total_files += 1
                    total_comments += comments
                    for comment_type, count in types.items():
                        all_comment_types[comment_type] += count
    
    print("=== Comment Analysis Report ===\n")
    print(f"Total files with comments: {total_files}")
    print(f"Total comment lines: {total_comments}")
    print("\nComment breakdown:")
    
    for comment_type, count in sorted(all_comment_types.items(), key=lambda x: x[1], reverse=True):
        percentage = (count / total_comments * 100) if total_comments > 0 else 0
        print(f"  {comment_type:20s}: {count:5d} ({percentage:5.1f}%)")
    
    print("\nRecommendations:")
    if all_comment_types['likely_code'] > 0:
        print(f"- {all_comment_types['likely_code']} comments appear to be commented-out code (can be removed)")
    if all_comment_types['empty'] > 0:
        print(f"- {all_comment_types['empty']} empty comment lines (can be removed)")
    
    removable = all_comment_types['likely_code'] + all_comment_types['empty']
    if removable > 0:
        print(f"\nTotal safely removable: {removable} comment lines")
        print(f"Would reduce comment count from {total_comments} to {total_comments - removable}")

if __name__ == '__main__':
    main()