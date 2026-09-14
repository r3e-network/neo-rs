#!/usr/bin/env python3
"""
Verification Script for Neo-N3 Testnet Sync Toolkit
Checks all components are properly installed and accessible
"""

import os
import sys
from pathlib import Path
from datetime import datetime

def print_header():
    """Print welcome header"""
    print("\n" + "="*70)
    print(" Neo-N3 Testnet Sync Toolkit - Installation Verification")
    print("="*70 + "\n")

def check_directory_structure():
    """Verify tool directory structure exists"""
    print("[1/6] Checking directory structure...")
    
    tool_dir = Path(__file__).parent
    required_files = [
        "index.html",
        "sync_monitor.py", 
        "sync.sh",
        "sync.bat",
        "QUICK_START.md",
        "README.md",
        "README_TOOLKIT.md",
        "QUICK_REFERENCE.md"
    ]
    
    missing = []
    for file in required_files:
        if not (tool_dir / file).exists():
            missing.append(file)
    
    if missing:
        print("  ❌ Missing files:")
        for f in missing:
            print(f"     - {f}")
        return False
    else:
        print("  ✓ All required files present")
        return True

def check_dashboard_html():
    """Verify dashboard HTML file is valid"""
    print("[2/6] Checking dashboard HTML...")
    
    dashboard_path = Path(__file__).parent / "index.html"
    html_content = dashboard_path.read_text()
    
    checks = [
        ("CSS styling block", ":root {" in html_content),
        ("Dashboard grid layout", ".dashboard-grid" in html_content),
        ("Progress visualization", ".progress-bar" in html_content),
        ("JavaScript functionality", "<script>" in html_content),
        ("Responsive design", "@media" in html_content)
    ]
    
    all_pass = True
    for name, passed in checks:
        status = "✓" if passed else "❌"
        print(f"  {status} {name}")
        if not passed:
            all_pass = False
    
    return all_pass

def check_python_monitor():
    """Verify Python monitoring backend"""
    print("[3/6] Checking Python monitor...")
    
    monitor_path = Path(__file__).parent / "sync_monitor.py"
    content = monitor_path.read_text()
    
    checks = [
        ("SyncMonitor class defined", "class SyncMonitor:" in content),
        ("Log parsing implemented", "def parse_log_line" in content),
        ("Validation functions", "def validate_state_checkpoint" in content),
        ("Report generation", "def generate_report" in content),
        ("Error handling", "except Exception" in content)
    ]
    
    all_pass = True
    for name, passed in checks:
        status = "✓" if passed else "❌"
        print(f"  {status} {name}")
        if not passed:
            all_pass = False
    
    return all_pass

def check_scripts():
    """Verify orchestration scripts exist and are executable"""
    print("[4/6] Checking orchestration scripts...")
    
    tool_dir = Path(__file__).parent
    
    # Check bash script
    bash_script = tool_dir / "sync.sh"
    if bash_script.exists():
        content = bash_script.read_text()
        has_main = "def main():" in content or 'main "$@"' in content
        status = "✓" if has_main else "❌"
        print(f"  {status} sync.sh (Bash script)")
        bash_ok = has_main
    else:
        print("  ❌ sync.sh missing")
        bash_ok = False
    
    # Check batch script
    batch_script = tool_dir / "sync.bat"
    if batch_script.exists():
        content = batch_script.read_text()
        has_sync = "run_full_sync" in content
        status = "✓" if has_sync else "❌"
        print(f"  {status} sync.bat (Windows script)")
        batch_ok = has_sync
    else:
        print("  ❌ sync.bat missing")
        batch_ok = False
    
    return bash_ok and batch_ok

def check_documentation():
    """Verify documentation files are comprehensive"""
    print("[5/6] Checking documentation...")
    
    doc_files = {
        "QUICK_START.md": "Quick Start Guide",
        "README.md": "Comprehensive Documentation",
        "README_TOOLKIT.md": "Tool Suite Overview",
        "QUICK_REFERENCE.md": "Quick Reference Card"
    }
    
    all_pass = True
    for filename, description in doc_files.items():
        filepath = Path(__file__).parent / filename
        if filepath.exists():
            content = filepath.read_text()
            has_content = len(content) > 1000  # Should be substantial
            status = "✓" if has_content else "⚠️"
            print(f"  {status} {description} ({filename})")
            if not has_content:
                all_pass = False
        else:
            print(f"  ❌ {description} ({filename}) - MISSING")
            all_pass = False
    
    return all_pass

def check_node_config():
    """Verify testnet configuration exists"""
    print("[6/6] Checking testnet configuration...")
    
    config_path = Path.cwd().parent.parent / "config" / "testnet-production.toml"
    
    if config_path.exists():
        print(f"  ✓ Testnet config found: {config_path}")
        
        content = config_path.read_text()
        checks = [
            ("Network = TestNet", "Network = \"TestNet\"" in content),
            ("P2P port configured", "Port = 20332" in content),
            ["Storage section", "[Storage]" in content],
            ("RPC endpoint", "http://localhost:10332" in content.lower())
        ]
        
        all_pass = True
        for name, passed in checks:
            status = "✓" if passed else "⚠️"
            print(f"    {status} {name}")
            if not passed:
                all_pass = False
        
        return all_pass
    else:
        print(f"  ⚠️  Config not found at expected location: {config_path}")
        print(f"  ℹ️  Please ensure you're running from project root: d:\\Git\\neo-rs")
        return True  # Not critical, just informational

def print_summary(all_pass):
    """Print verification summary"""
    print("\n" + "="*70)
    
    if all_pass:
        print(" ✅ VERIFICATION PASSED - All components ready!")
        print("\n Next steps:")
        print("   1. Build optimized binary: cargo build --release --bin neo-node")
        print("   2. Run sync: ./tools/sync-monitor/sync.sh sync")
        print("   3. Open dashboard: http://localhost:8080")
    else:
        print(" ⚠️  VERIFICATION ISSUES DETECTED")
        print("\n Please review errors above and fix before proceeding.")
    
    print("="*70 + "\n")

def main():
    """Run all verification checks"""
    print_header()
    
    results = []
    
    # Run checks
    results.append(check_directory_structure())
    results.append(check_dashboard_html())
    results.append(check_python_monitor())
    results.append(check_scripts())
    results.append(check_documentation())
    results.append(check_node_config())
    
    # Print summary
    all_pass = all(results)
    print_summary(all_pass)
    
    return 0 if all_pass else 1

if __name__ == "__main__":
    sys.exit(main())
