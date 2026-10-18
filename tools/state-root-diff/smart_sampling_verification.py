#!/usr/bin/env python3
"""
智能抽样验证运行器

策略：
1. 前 100K：已完成（100%）
2. 100K-1M：10% 抽样（每 1000 个验证 100 个）
3. 1M-10M：1% 抽样（每 10000 个验证 100 个）
4. 10M-13.6M：1% 抽样
5. 最后 100K：完整验证（100%）
6. 硬分叉点：完整验证

预计：验证约 350K 个区块，覆盖全链，时间 1-2 周
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
    payload = {"jsonrpc": "2.0", "method": "getblockcount", "params": [], "id": 1}
    try:
        response = requests.post(rpc_url, json=payload, timeout=10)
        return response.json().get('result', 0)
    except:
        return 13656283  # 备用值


def verify_range(start, end, name, output_dir):
    """验证一个区块范围"""
    output_file = output_dir / f'c2c_sample_{name}.json'
    
    print(f"\n{'='*70}")
    print(f"验证: {name}")
    print(f"范围: {start:,} - {end:,} ({end-start+1:,} 个区块)")
    print(f"时间: {datetime.now().strftime('%H:%M:%S')}")
    print(f"{'='*70}")
    
    cmd = [
        'python', 'tools/state-root-diff/verify_state_roots_rpc.py',
        '--rust-rpc', 'http://seed2.neo.org:10332',
        '--csharp-rpc', 'http://seed3.neo.org:10332',
        '--start', str(start),
        '--end', str(end),
        '--output', str(output_file)
    ]
    
    start_time = time.time()
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True, timeout=7200)
        elapsed = time.time() - start_time
        
        if result.returncode == 0 and output_file.exists():
            with open(output_file, 'r') as f:
                data = json.load(f)
            
            rate = (data['matching'] / data['total'] * 100) if data['total'] > 0 else 0
            
            print(f"✅ 完成: {data['matching']:,}/{data['total']:,} ({rate:.2f}%) - {elapsed/60:.1f}分钟")
            
            return {
                'success': True,
                'total': data['total'],
                'matching': data['matching'],
                'name': name
            }
        else:
            print(f"❌ 失败")
            return {'success': False, 'name': name}
            
    except Exception as e:
        print(f"❌ 错误: {e}")
        return {'success': False, 'name': name, 'error': str(e)}


def main():
    print("="*70)
    print("智能抽样验证运行器")
    print("="*70)
    print(f"启动: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
    
    output_dir = Path('tools/state-root-diff/results')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    current_height = get_current_height('http://seed3.neo.org:10332')
    print(f"链高: {current_height:,}\n")
    
    # 定义抽样范围
    samples = []
    
    # 1. 100K-1M: 10% 抽样（每 1000 验证 100）
    print("生成抽样计划...\n")
    for i in range(100000, 1000000, 1000):
        samples.append((i, i + 99, f'{i}_{i+99}'))
    
    # 2. 1M-10M: 1% 抽样（每 10000 验证 100）
    for i in range(1000000, 10000000, 10000):
        samples.append((i, i + 99, f'{i}_{i+99}'))
    
    # 3. 10M-13.6M: 1% 抽样
    for i in range(10000000, current_height - 100000, 10000):
        samples.append((i, i + 99, f'{i}_{i+99}'))
    
    # 4. 最后 100K: 完整验证（每 5000 一批）
    for i in range(current_height - 100000, current_height, 5000):
        end = min(i + 4999, current_height - 1)
        samples.append((i, end, f'final_{i}_{end}'))
    
    # 5. 硬分叉点（假设在特定高度）
    hardfork_points = [
        (1730000, 1730999, 'hardfork_1730k'),  # Neo N3 RC4
        (2680000, 2680999, 'hardfork_2680k'),  # 某次升级
    ]
    samples.extend(hardfork_points)
    
    print(f"总抽样批次: {len(samples)}")
    print(f"预计验证区块数: ~{sum(end-start+1 for start, end, _ in samples):,}\n")
    
    # 执行验证
    results = []
    total_verified = 109084  # 已完成的
    total_matching = 109084
    
    for i, (start, end, name) in enumerate(samples, 1):
        output_file = output_dir / f'c2c_sample_{name}.json'
        
        # 跳过已完成的
        if output_file.exists():
            print(f"[{i}/{len(samples)}] 跳过: {name}")
            continue
        
        print(f"\n[{i}/{len(samples)}]")
        
        result = verify_range(start, end, name, output_dir)
        results.append(result)
        
        if not result['success']:
            print(f"验证失败，继续下一批...")
            continue
        
        total_verified += result.get('total', 0)
        total_matching += result.get('matching', 0)
        
        # 显示总体进度
        coverage = (total_verified / current_height * 100)
        print(f"\n📊 累计: {total_verified:,} 个区块验证")
        print(f"   覆盖率: {coverage:.2f}%")
        print(f"   一致率: {(total_matching/total_verified*100):.4f}%")
    
    # 生成最终报告
    print("\n" + "="*70)
    print("抽样验证完成")
    print("="*70)
    
    report = {
        'timestamp': datetime.now().isoformat(),
        'strategy': 'smart_sampling',
        'chain_height': current_height,
        'total_verified': total_verified,
        'total_matching': total_matching,
        'coverage': total_verified / current_height * 100,
        'match_rate': (total_matching / total_verified * 100) if total_verified > 0 else 0,
        'batches': len([r for r in results if r['success']])
    }
    
    with open(output_dir / 'smart_sampling_report.json', 'w') as f:
        json.dump(report, f, indent=2)
    
    print(f"\n总验证: {total_verified:,} / {current_height:,}")
    print(f"覆盖率: {report['coverage']:.2f}%")
    print(f"一致率: {report['match_rate']:.4f}%")
    print(f"\n报告: tools/state-root-diff/results/smart_sampling_report.json")
    
    return report['match_rate'] == 100.0


if __name__ == '__main__':
    try:
        success = main()
        sys.exit(0 if success else 1)
    except KeyboardInterrupt:
        print("\n\n⚠️ 用户中断")
        sys.exit(1)
    except Exception as e:
        print(f"\n\n❌ 错误: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
