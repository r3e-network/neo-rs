#!/usr/bin/env python3
"""
简化的验证报告生成器

基于 Rust 验证器的结果生成报告，分析通过/失败的模式。
"""

import json
import sys
from pathlib import Path


def analyze_rust_results(rust_results_path: Path, test_vectors_path: Path) -> dict:
    """分析 Rust 验证结果"""
    
    # 加载结果
    with open(rust_results_path, 'r') as f:
        rust_results = json.load(f)
    
    # 加载测试向量以获取预期结果
    with open(test_vectors_path, 'r') as f:
        test_vectors = json.load(f)
    
    # 创建测试向量的查找表
    expected = {}
    for test in test_vectors:
        expected[test['test_id']] = test['expected_valid']
    
    # 分析结果
    total = len(rust_results)
    correct = 0
    incorrect = 0
    mismatches = []
    
    valid_blocks_correct = 0
    valid_blocks_total = 0
    invalid_blocks_correct = 0
    invalid_blocks_total = 0
    
    for test_id, result in rust_results.items():
        expected_valid = expected.get(test_id, None)
        actual_valid = result['valid']
        
        if expected_valid is None:
            continue
        
        if expected_valid:
            valid_blocks_total += 1
            if actual_valid:
                valid_blocks_correct += 1
        else:
            invalid_blocks_total += 1
            if not actual_valid:
                invalid_blocks_correct += 1
        
        if expected_valid == actual_valid:
            correct += 1
        else:
            incorrect += 1
            mismatches.append({
                'test_id': test_id,
                'expected_valid': expected_valid,
                'actual_valid': actual_valid,
                'error_type': result.get('error_type'),
                'error_message': result.get('error_message')
            })
    
    return {
        'total': total,
        'correct': correct,
        'incorrect': incorrect,
        'accuracy': correct / total * 100 if total > 0 else 0,
        'valid_blocks': {
            'total': valid_blocks_total,
            'correct': valid_blocks_correct,
            'accuracy': valid_blocks_correct / valid_blocks_total * 100 if valid_blocks_total > 0 else 0
        },
        'invalid_blocks': {
            'total': invalid_blocks_total,
            'correct': invalid_blocks_correct,
            'accuracy': invalid_blocks_correct / invalid_blocks_total * 100 if invalid_blocks_total > 0 else 0
        },
        'mismatches': mismatches
    }


def generate_report(analysis: dict, output_path: Path):
    """生成 Markdown 报告"""
    
    with open(output_path, 'w', encoding='utf-8') as f:
        f.write("# Neo-rs 区块验证规则验证报告\n\n")
        f.write("**生成时间**：2026-09-30\n\n")
        f.write("## 执行摘要\n\n")
        
        f.write(f"- **总测试数**：{analysis['total']}\n")
        f.write(f"- **正确数**：{analysis['correct']}\n")
        f.write(f"- **错误数**：{analysis['incorrect']}\n")
        f.write(f"- **准确率**：{analysis['accuracy']:.2f}%\n\n")
        
        f.write("## 分类统计\n\n")
        
        f.write("### 有效区块验证\n\n")
        vb = analysis['valid_blocks']
        f.write(f"- 总数：{vb['total']}\n")
        f.write(f"- 正确识别：{vb['correct']}\n")
        f.write(f"- 准确率：{vb['accuracy']:.2f}%\n\n")
        
        f.write("### 无效区块验证\n\n")
        ib = analysis['invalid_blocks']
        f.write(f"- 总数：{ib['total']}\n")
        f.write(f"- 正确拒绝：{ib['correct']}\n")
        f.write(f"- 准确率：{ib['accuracy']:.2f}%\n\n")
        
        if analysis['incorrect'] == 0:
            f.write("## ✅ 完美匹配！\n\n")
            f.write("所有验证结果与预期完全一致。neo-rs 的区块验证规则实现正确。\n\n")
        else:
            f.write("## ⚠️ 发现不匹配\n\n")
            f.write(f"以下 {analysis['incorrect']} 个测试结果与预期不一致：\n\n")
            
            for i, mismatch in enumerate(analysis['mismatches'], 1):
                f.write(f"### {i}. Test ID: `{mismatch['test_id']}`\n\n")
                f.write(f"- **预期**：{'有效' if mismatch['expected_valid'] else '无效'}\n")
                f.write(f"- **实际**：{'有效' if mismatch['actual_valid'] else '无效'}\n")
                
                if mismatch['error_type']:
                    f.write(f"- **错误类型**：{mismatch['error_type']}\n")
                if mismatch['error_message']:
                    f.write(f"- **错误消息**：{mismatch['error_message']}\n")
                
                f.write("\n---\n\n")
        
        f.write("## 结论\n\n")
        
        if analysis['accuracy'] == 100.0:
            f.write("✅ **neo-rs 的区块验证规则与预期 100% 一致**\n\n")
            f.write("这证明了 neo-rs 正确实现了 Neo N3 的区块验证规则。\n\n")
        elif analysis['accuracy'] >= 90.0:
            f.write(f"⚠️ **neo-rs 的区块验证规则准确率为 {analysis['accuracy']:.2f}%**\n\n")
            f.write("大部分验证规则已正确实现，但仍有少数不匹配需要修复。\n\n")
        else:
            f.write(f"❌ **neo-rs 的区块验证规则准确率仅为 {analysis['accuracy']:.2f}%**\n\n")
            f.write("需要进一步修复验证规则实现。\n\n")
        
        f.write("## 下一步\n\n")
        
        if analysis['incorrect'] > 0:
            f.write("1. 分析上述不匹配的测试用例\n")
            f.write("2. 修复 neo-rs 的验证规则实现\n")
            f.write("3. 重新运行验证并确认修复\n")
            f.write("4. 继续原生合约调用输出差分\n\n")
        else:
            f.write("1. ✅ 区块验证规则验证完成\n")
            f.write("2. 继续原生合约调用输出差分\n")
            f.write("3. 生成层级 1 总结报告\n")
            f.write("4. 启动层级 2（端到端状态根差分）\n\n")


def main():
    # 输入路径
    rust_results = Path('tools/validation-diff/results/rust_results.json')
    test_vectors = Path('tools/validation-diff/test_vectors/all_blocks.json')
    output_report = Path('reports/formal/layer1-block-validation-report.md')
    
    if not rust_results.exists():
        print(f"Error: {rust_results} not found")
        print("Please run the Rust validator first")
        sys.exit(1)
    
    if not test_vectors.exists():
        print(f"Error: {test_vectors} not found")
        sys.exit(1)
    
    # 确保输出目录存在
    output_report.parent.mkdir(parents=True, exist_ok=True)
    
    print("Analyzing Rust validation results...")
    analysis = analyze_rust_results(rust_results, test_vectors)
    
    print(f"\nResults:")
    print(f"  Total: {analysis['total']}")
    print(f"  Correct: {analysis['correct']}")
    print(f"  Incorrect: {analysis['incorrect']}")
    print(f"  Accuracy: {analysis['accuracy']:.2f}%")
    
    print(f"\nGenerating report to {output_report}...")
    generate_report(analysis, output_report)
    
    print("\n✅ Report generated successfully!")
    
    if analysis['accuracy'] == 100.0:
        print("\n🎉 Perfect match! All validation rules are correct.")
        sys.exit(0)
    else:
        print(f"\n⚠️  Found {analysis['incorrect']} mismatches. See report for details.")
        sys.exit(1)


if __name__ == '__main__':
    main()
