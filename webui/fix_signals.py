#!/usr/bin/env python3
"""Fix Dioxus 0.6 signal API calls."""

import re
import sys

def fix_signals(content):
    """Replace old signal API with new Dioxus 0.6 API."""
    
    # Fix the most common pattern: *signal.read() for String types should be signal.read().clone()
    # These are String signals that shouldn't be dereferenced
    content = re.sub(r'\*(\w*sort\w*)\.read\(\)', r'\1.read().clone()', content)
    content = re.sub(r'\*(\w*order\w*)\.read\(\)', r'\1.read().clone()', content)
    content = re.sub(r'\*(\w*scope\w*)\.read\(\)', r'\1.read().clone()', content)
    content = re.sub(r'\*(\w*path\w*)\.read\(\)', r'\1.read().clone()', content)
    content = re.sub(r'\*(\w*filter\w*)\.read\(\)', r'\1.read().clone()', content)
    content = re.sub(r'\*(\w*query\w*)\.read\(\)', r'\1.read().clone()', content)
    
    # Fix tree_sort_view specifically
    content = re.sub(r'\*tree_sort_view\.read\(\)', r'tree_sort_view.read().clone()', content)
    
    return content

def main():
    file_path = r"c:\Users\rujbin1\Documents\speicherwald\webui\src\main.rs"
    
    with open(file_path, 'r', encoding='utf-8') as f:
        content = f.read()
    
    fixed_content = fix_signals(content)
    
    with open(file_path, 'w', encoding='utf-8') as f:
        f.write(fixed_content)
    
    print("Fixed signal API calls")

if __name__ == "__main__":
    main()
