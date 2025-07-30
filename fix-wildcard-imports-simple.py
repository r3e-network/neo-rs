#!/usr/bin/env python3
import os
import re
import glob

def fix_wildcards(file_path):
    try:
        with open(file_path, 'r') as f:
            content = f.read()
        
        original = content
        
        # Skip test files
        if 'test' in file_path:
            return 0
        
        # Fix common wildcards
        replacements = [
            ('use std::collections::*;', 'use std::collections::{HashMap, HashSet};'),
            ('use std::io::*;', 'use std::io::{Read, Write};'),
            ('use std::fmt::*;', 'use std::fmt::{self, Display, Debug};'),
            ('use serde::*;', 'use serde::{Serialize, Deserialize};'),
        ]
        
        changes = 0
        for old, new in replacements:
            if old in content:
                content = content.replace(old, new)
                changes += 1
        
        if content != original:
            with open(file_path, 'w') as f:
                f.write(content)
            print(f"Fixed {changes} wildcards in {file_path}")
            return changes
        
        return 0
    except Exception as e:
        print(f"Error: {e}")
        return 0

total = 0
for pattern in ['crates/**/*.rs', 'node/**/*.rs']:
    for file_path in glob.glob(pattern, recursive=True):
        total += fix_wildcards(file_path)

print(f"Total fixed: {total}")