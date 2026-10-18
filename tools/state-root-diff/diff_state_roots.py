#!/usr/bin/env python3
"""
状态根差分比对器

比对 Rust 和 C# 的状态根，找到第一个分歧点。
"""

import json
import sys
from pathlib import Path


def load_state_roots(path: Path) -> dict:
    """加载状态根文件"""
    with open(path, 'r') as f:
        return json.load(f)


def compare_state_roots(rust_path: Path, csharp_path: Path) -> dict:
    """比对 Rust 和 C# 的状态根"""
    
    # 加载数据
    rust_data = load_state_roots(rust_path)
    csharp_data = load_state_roots(csharp_path)
    
    rust_roots = {sr['height']: sr for sr in rust_data['state_roots']}
    csharp_roots = {sr['height']: sr for sr in csharp_data['state_roots']}
    
    # 分析
    total = len(rust_roots)
    matching = 0
    divergence_point = None
    mismatches = []
    
    for height in sorted(rust_roots.keys()):
        rust_sr = rust_roots.get(height)
        csharp_sr = csharp_roots.get(height)
        
        if not csharp_sr:
            print(f"⚠️  Height {height}: Missing in C# results")
            continue
        
        if rust_sr['state_root_hash'] == csharp_sr['state_root_hash']:
            matching += 1
        else:
            if divergence_point is None:
                divergence_point = height
            
            mismatches.append({
                'height': height,
                'block_hash': rust_sr['block_hash'],
                'rust_state_root': rust_sr['state_root_hash'],
                'csharp_state_root': csharp_sr['state_root_hash']
            })
    
    return {
        'total': total,
        'matching': matching,
        'mismatching': len(mismatches),
        'match_rate': matching / total * 100 if total > 0 else 0,
        'divergence_point': divergence_point,
        'mismatches': mismatches
    }


def generate_report(analysis: dict, output_path: Path):
    """生成差分报告"""
    
    with open(output_path, 'w', encoding='utf-8') as f:
        f.write("# Neo-rs 状态根差分报告\n\n")
        f.write("**生成时间**：2026-09-30\n\n")
        f.write("## 执行摘要\n\n")
        
        f.write(f"- **总区块数**：{analysis['total']}\n")
        f.write(f"- **状态根一致**：{analysis['matching']}\n")
        f.write(f"- **状态根不一致**：{analysis['mismatching']}\n")
        f.write(f"- **一致率**：{analysis['match_rate']:.2f}%\n\n")
        
        if analysis['divergence_point'] is not None:
            f.write(f"## ⚠️ 发现分歧点\n\n")
            f.write(f"**第一个分歧区块高度**：{analysis['divergence_point']}\n\n")
            f.write(f"从区块 {analysis['divergence_point']} 开始，Rust 和 C# 的状态根开始不一致。\n\n")
            
            f.write("### 详细不匹配列表\n\n")
            
            for i, mismatch in enumerate(analysis['mismatches'][:10], 1):  # 只显示前 10 个
                f.write(f"#### {i}. 区块高度：{mismatch['height']}\n\n")
                f.write(f"- **区块哈希**：`{mismatch['block_hash']}`\n")
                f.write(f"- **Rust 状态根**：`{mismatch['rust_state_root']}`\n")
                f.write(f"- **C# 状态根**：`{mismatch['csharp_state_root']}`\n\n")
            
            if len(analysis['mismatches']) > 10:
                f.write(f"... 还有 {len(analysis['mismatches']) - 10} 个不匹配（已省略）\n\n")
        else:
            f.write("## ✅ 完美匹配！\n\n")
            f.write("所有区块的状态根与 C# 完全一致。\n\n")
        
        f.write("## 下一步\n\n")
        
        if analysis['divergence_point'] is not None:
            f.write(f"1. 定位到区块 {analysis['divergence_point']}\n")
            f.write("2. 提取该区块的所有交易\n")
            f.write("3. 逐交易重放并比对状态变化\n")
            f.write("4. 找到导致分歧的具体交易和操作\n")
            f.write("5. 修复 Rust 实现\n")
            f.write("6. 重新验证\n\n")
        else:
            f.write("1. ✅ 当前范围验证完成\n")
            f.write("2. 扩大验证范围（下载更多区块）\n")
            f.write("3. 继续验证直到当前链高\n\n")


def main():
    if len(sys.argv) < 4:
        print("Usage: python diff_state_roots.py <rust_state_roots.json> <csharp_state_roots.json> <output_report.md>")
        sys.exit(1)
    
    rust_path = Path(sys.argv[1])
    csharp_path = Path(sys.argv[2])
    output_path = Path(sys.argv[3])
    
    if not rust_path.exists():
        print(f"Error: {rust_path} not found")
        sys.exit(1)
    
    if not csharp_path.exists():
        print(f"Error: {csharp_path} not found")
        sys.exit(1)
    
    print("Comparing state roots...")
    analysis = compare_state_roots(rust_path, csharp_path)
    
    print(f"\nResults:")
    print(f"  Total blocks: {analysis['total']}")
    print(f"  Matching: {analysis['matching']}")
    print(f"  Mismatching: {analysis['mismatching']}")
    print(f"  Match rate: {analysis['match_rate']:.2f}%")
    
    if analysis['divergence_point'] is not None:
        print(f"  Divergence point: Block {analysis['divergence_point']}")
    
    print(f"\nGenerating report to {output_path}...")
    generate_report(analysis, output_path)
    
    print("\n✅ Report generated!")
    
    if analysis['match_rate'] == 100.0:
        print("\n🎉 Perfect match! All state roots are consistent.")
        sys.exit(0)
    else:
        print(f"\n⚠️  Found divergence at block {analysis['divergence_point']}. See report for details.")
        sys.exit(1)


if __name__ == '__main__':
    main()
