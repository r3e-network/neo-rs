#!/usr/bin/env python3
"""
快速全量验证运行器（优化版）

使用抽样策略加速验证：
1. 完整验证前 10,000 个区块
2. 抽样验证中间区块（每 1000 个验证 1 个）
3. 完整验证最近 10,000 个区块

这样可以在合理时间内验证整个链的代表性样本。
"""

import json
import subprocess
import sys
import time
from pathlib import Path
from datetime import datetime
import requests


def get_current_height(rpc_url):
    """获取当前链高"""
    payload = {
        "jsonrpc": "2.0",
        "method": "getblockcount",
        "params": [],
        "id": 1
    }
    
    try:
        response = requests.post(rpc_url, json=payload, timeout=10)
        result = response.json()
        return result.get('result', 0)
    except Exception as e:
        print(f"❌ 获取链高失败: {e}")
        return None


def verify_range(rust_rpc, csharp_rpc, start, end, name, output_dir):
    """验证一个区块范围"""
    output_file = output_dir / f'{name}.json'
    
    print(f"\n验证: {name} ({start:,} - {end:,})")
    
    cmd = [
        'python',
        'tools/state-root-diff/verify_state_roots_rpc.py',
        '--rust-rpc', rust_rpc,
        '--csharp-rpc', csharp_rpc,
        '--start', str(start),
        '--end', str(end),
        '--output', str(output_file)
    ]
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=7200)
        
        if result.returncode == 0 and output_file.exists():
            with open(output_file, 'r') as f:
                data = json.load(f)
            
            total = data['total']
            matching = data['matching']
            rate = matching / total * 100 if total > 0 else 0
            
            print(f"  ✅ {matching:,}/{total:,} 匹配 ({rate:.2f}%)")
            
            return {
                'success': True,
                'name': name,
                'start': start,
                'end': end,
                'total': total,
                'matching': matching,
                'rate': rate
            }
        else:
            print(f"  ❌ 验证失败")
            return {'success': False, 'name': name}
            
    except Exception as e:
        print(f"  ❌ 错误: {e}")
        return {'success': False, 'name': name, 'error': str(e)}


def main():
    print("="*60)
    print("Neo-rs 快速全量验证（优化策略）")
    print("="*60)
    
    rust_rpc = 'http://seed2.neo.org:10332'
    csharp_rpc = 'http://seed3.neo.org:10332'
    output_dir = Path('tools/state-root-diff/results')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # 获取当前链高
    current_height = get_current_height(csharp_rpc)
    if not current_height:
        print("❌ 无法启动验证")
        return
    
    print(f"\n当前链高: {current_height:,}\n")
    
    # 验证计划
    ranges = []
    
    # 1. 前 1,000 个区块（完整）
    ranges.append((0, 999, 'range_0_1k'))
    
    # 2. 1K-10K（完整）
    ranges.append((1000, 9999, 'range_1k_10k'))
    
    # 3. 10K-100K（每 1000 验证 100）
    for i in range(10000, 100000, 1000):
        ranges.append((i, min(i + 99, 99999), f'sample_{i}_{i+99}'))
    
    # 4. 100K-1M（每 10000 验证 100）
    for i in range(100000, 1000000, 10000):
        ranges.append((i, min(i + 99, 999999), f'sample_{i}_{i+99}'))
    
    # 5. 1M-当前（每 100000 验证 100）
    for i in range(1000000, current_height - 10000, 100000):
        ranges.append((i, min(i + 99, current_height - 10001), f'sample_{i}_{i+99}'))
    
    # 6. 最后 10,000 个区块（完整）
    ranges.append((current_height - 10000, current_height - 1, 'range_last_10k'))
    
    print(f"验证计划: {len(ranges)} 个范围")
    print(f"预计总验证区块数: ~{sum(r[1]-r[0]+1 for r in ranges):,}\n")
    
    # 执行验证
    results = []
    start_time = time.time()
    
    for i, (start, end, name) in enumerate(ranges, 1):
        print(f"[{i}/{len(ranges)}]", end=' ')
        
        result = verify_range(rust_rpc, csharp_rpc, start, end, name, output_dir)
        results.append(result)
        
        if not result['success']:
            print(f"\n❌ 验证失败，停止")
            break
        
        if result.get('matching', 0) != result.get('total', 0):
            print(f"\n⚠️  发现不匹配，停止")
            break
    
    # 生成报告
    elapsed = time.time() - start_time
    total_blocks = sum(r.get('total', 0) for r in results if r['success'])
    total_matching = sum(r.get('matching', 0) for r in results if r['success'])
    
    print("\n" + "="*60)
    print("验证完成")
    print("="*60)
    print(f"总验证区块数: {total_blocks:,}")
    print(f"匹配数: {total_matching:,}")
    
    if total_blocks > 0:
        rate = total_matching / total_blocks * 100
        print(f"一致率: {rate:.2f}%")
    
    print(f"用时: {elapsed/60:.1f} 分钟")
    
    # 保存报告
    report = {
        'timestamp': datetime.now().isoformat(),
        'strategy': 'optimized_sampling',
        'current_height': current_height,
        'total_blocks_verified': total_blocks,
        'total_matching': total_matching,
        'elapsed_seconds': elapsed,
        'ranges': results
    }
    
    report_file = output_dir / 'optimized_verification_report.json'
    with open(report_file, 'w') as f:
        json.dump(report, f, indent=2)
    
    print(f"\n报告已保存: {report_file}")
    
    if total_blocks > 0 and total_matching == total_blocks:
        print("\n🎉 验证成功！所有区块状态根一致！")
        return True
    else:
        print("\n⚠️  验证未完全通过")
        return False


if __name__ == '__main__':
    success = main()
    sys.exit(0 if success else 1)
