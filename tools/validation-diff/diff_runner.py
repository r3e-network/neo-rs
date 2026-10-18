#!/usr/bin/env python3
"""
验证规则差分比对器

比对 Rust 和 C# 的验证结果，生成差分报告。
"""

import json
import sys
from pathlib import Path
from typing import Dict, List


class ValidationResult:
    def __init__(self, valid: bool, error_type: str = None, error_message: str = None):
        self.valid = valid
        self.error_type = error_type
        self.error_message = error_message


class DiffReport:
    def __init__(self):
        self.total_tests = 0
        self.matching = 0
        self.mismatching = 0
        self.mismatches = []

    def add_match(self, test_id: str):
        self.matching += 1

    def add_mismatch(self, test_id: str, rust_result: dict, csharp_result: dict):
        self.mismatching += 1
        self.mismatches.append({
            'test_id': test_id,
            'rust': rust_result,
            'csharp': csharp_result
        })

    def to_dict(self):
        return {
            'total_tests': self.total_tests,
            'matching': self.matching,
            'mismatching': self.mismatching,
            'match_rate': f"{(self.matching / self.total_tests * 100):.2f}%" if self.total_tests > 0 else "0%",
            'mismatches': self.mismatches
        }


def load_results(path: Path) -> Dict:
    """加载 JSON 结果文件"""
    with open(path, 'r', encoding='utf-8') as f:
        return json.load(f)


def compare_results(rust_results: Dict, csharp_results: Dict) -> DiffReport:
    """比对 Rust 和 C# 的验证结果"""
    report = DiffReport()
    
    # 获取所有测试 ID
    all_test_ids = set(rust_results.keys()) | set(csharp_results.keys())
    report.total_tests = len(all_test_ids)
    
    for test_id in sorted(all_test_ids):
        rust = rust_results.get(test_id)
        csharp = csharp_results.get(test_id)
        
        # 检查是否都存在
        if rust is None or csharp is None:
            report.add_mismatch(test_id, rust, csharp)
            continue
        
        # 比对 valid 字段
        if rust['valid'] != csharp['valid']:
            report.add_mismatch(test_id, rust, csharp)
            continue
        
        # 如果都是 invalid，比对 error_type
        if not rust['valid'] and not csharp['valid']:
            if rust.get('error_type') != csharp.get('error_type'):
                report.add_mismatch(test_id, rust, csharp)
                continue
        
        # 完全匹配
        report.add_match(test_id)
    
    return report


def generate_markdown_report(report: DiffReport, output_path: Path):
    """生成 Markdown 格式的差分报告"""
    with open(output_path, 'w', encoding='utf-8') as f:
        f.write("# 验证规则差分报告\n\n")
        f.write("## 摘要\n\n")
        f.write(f"- **总测试数**: {report.total_tests}\n")
        f.write(f"- **匹配数**: {report.matching}\n")
        f.write(f"- **不匹配数**: {report.mismatching}\n")
        f.write(f"- **匹配率**: {(report.matching / report.total_tests * 100):.2f}%\n\n")
        
        if report.mismatching == 0:
            f.write("## ✅ 完美匹配！\n\n")
            f.write("所有验证结果与 C# 3.10.1 完全一致。\n")
        else:
            f.write("## ⚠️ 发现不匹配\n\n")
            f.write(f"以下 {report.mismatching} 个测试结果不一致：\n\n")
            
            for i, mismatch in enumerate(report.mismatches, 1):
                f.write(f"### {i}. Test ID: `{mismatch['test_id']}`\n\n")
                
                f.write("**Rust 结果**:\n")
                rust = mismatch['rust']
                if rust:
                    f.write(f"- Valid: {rust['valid']}\n")
                    if not rust['valid']:
                        f.write(f"- Error Type: {rust.get('error_type', 'N/A')}\n")
                        f.write(f"- Error Message: {rust.get('error_message', 'N/A')}\n")
                else:
                    f.write("- 缺失\n")
                
                f.write("\n**C# 结果**:\n")
                csharp = mismatch['csharp']
                if csharp:
                    f.write(f"- Valid: {csharp['valid']}\n")
                    if not csharp['valid']:
                        f.write(f"- Error Type: {csharp.get('error_type', 'N/A')}\n")
                        f.write(f"- Error Message: {csharp.get('error_message', 'N/A')}\n")
                else:
                    f.write("- 缺失\n")
                
                f.write("\n---\n\n")


def main():
    if len(sys.argv) != 4:
        print("Usage: python diff_runner.py <rust_results.json> <csharp_results.json> <output_report.md>")
        sys.exit(1)
    
    rust_path = Path(sys.argv[1])
    csharp_path = Path(sys.argv[2])
    output_path = Path(sys.argv[3])
    
    print(f"Loading Rust results from {rust_path}...")
    rust_results = load_results(rust_path)
    
    print(f"Loading C# results from {csharp_path}...")
    csharp_results = load_results(csharp_path)
    
    print("Comparing results...")
    report = compare_results(rust_results, csharp_results)
    
    print(f"Generating report to {output_path}...")
    generate_markdown_report(report, output_path)
    
    # 打印摘要
    print(f"\n=== Diff Summary ===")
    print(f"Total tests: {report.total_tests}")
    print(f"Matching: {report.matching}")
    print(f"Mismatching: {report.mismatching}")
    print(f"Match rate: {(report.matching / report.total_tests * 100):.2f}%")
    
    # 返回退出码
    if report.mismatching == 0:
        print("\n✅ All validation results match C# 3.10.1!")
        sys.exit(0)
    else:
        print(f"\n⚠️ Found {report.mismatching} mismatches. See report for details.")
        sys.exit(1)


if __name__ == '__main__':
    main()
