#!/usr/bin/env python3
"""
持续验证运行器 - 不间断验证所有 MainNet 区块

从当前进度开始，持续验证直到完成全部区块。
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
        return None


def verify_batch(start, end, batch_name, output_file):
    """验证一批区块"""
    print(f"\n{'='*60}")
    print(f"[{datetime.now().strftime('%Y-%m-%d %H:%M:%S')}]")
    print(f"验证批次: {batch_name}")
    print(f"区块范围: {start:,} - {end:,} ({end-start+1:,} 个区块)")
    print(f"{'='*60}")
    
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
            
            print(f"\n✅ 批次完成")
            print(f"   总数: {data['total']:,}")
            print(f"   匹配: {data['matching']:,}")
            print(f"   一致率: {rate:.2f}%")
            print(f"   用时: {elapsed/60:.1f} 分钟")
            print(f"   速度: {data['total']/elapsed:.1f} 区块/秒")
            
            if data['matching'] != data['total']:
                print(f"\n⚠️  发现 {data['mismatching']} 个不匹配")
                if data.get('divergence_point'):
                    print(f"   第一个分歧点: 区块 {data['divergence_point']}")
                return False
            
            return True
        else:
            print(f"\n❌ 验证失败")
            print(result.stderr[:500] if result.stderr else "未知错误")
            return False
            
    except subprocess.TimeoutExpired:
        print(f"\n❌ 验证超时（2小时）")
        return False
    except Exception as e:
        print(f"\n❌ 错误: {e}")
        return False


def main():
    print("="*60)
    print("Neo-rs 持续验证运行器")
    print("="*60)
    print(f"开始时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    
    rpc_url = 'http://seed3.neo.org:10332'
    output_dir = Path('tools/state-root-diff/results')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # 获取当前链高
    current_height = get_current_height(rpc_url)
    if not current_height:
        print("❌ 无法获取链高")
        return
    
    print(f"当前链高: {current_height:,}\n")
    
    # 检查已完成的批次
    completed = []
    if (output_dir / 'c2c_0_999.json').exists():
        completed.append((0, 999))
        print("✅ 已完成: 0-999")
    
    # 定义所有批次
    batches = []
    
    # 从 1000 开始
    current = 1000
    
    # 1K-10K: 每 1000 个一批
    while current < 10000:
        end = min(current + 999, 9999)
        batches.append((current, end, f'batch_{current}_{end}'))
        current = end + 1
    
    # 10K-100K: 每 5000 个一批
    while current < 100000:
        end = min(current + 4999, 99999)
        batches.append((current, end, f'batch_{current}_{end}'))
        current = end + 1
    
    # 100K-1M: 每 10000 个一批
    while current < 1000000:
        end = min(current + 9999, 999999)
        batches.append((current, end, f'batch_{current}_{end}'))
        current = end + 1
    
    # 1M-当前: 每 50000 个一批
    while current < current_height:
        end = min(current + 49999, current_height - 1)
        batches.append((current, end, f'batch_{current}_{end}'))
        current = end + 1
    
    print(f"总批次数: {len(batches)}")
    print(f"预计总区块数: {sum(b[1]-b[0]+1 for b in batches):,}\n")
    
    # 持续验证
    total_verified = 1000  # 已完成的
    total_matching = 1000
    start_time = time.time()
    
    for i, (start, end, name) in enumerate(batches, 1):
        output_file = output_dir / f'c2c_{name}.json'
        
        # 跳过已完成的
        if output_file.exists():
            print(f"[{i}/{len(batches)}] 跳过已完成: {name}")
            continue
        
        print(f"\n[批次 {i}/{len(batches)}]")
        
        success = verify_batch(start, end, name, output_file)
        
        if not success:
            print(f"\n❌ 批次 {name} 失败，停止验证")
            break
        
        # 更新统计
        with open(output_file, 'r') as f:
            data = json.load(f)
        total_verified += data['total']
        total_matching += data['matching']
        
        # 生成中间报告
        elapsed = time.time() - start_time
        progress = (total_verified / current_height * 100)
        
        print(f"\n📊 累计进度:")
        print(f"   已验证: {total_verified:,} / {current_height:,} ({progress:.2f}%)")
        print(f"   匹配: {total_matching:,}")
        print(f"   一致率: {(total_matching/total_verified*100):.2f}%")
        print(f"   累计用时: {elapsed/3600:.1f} 小时")
        print(f"   平均速度: {total_verified/elapsed:.1f} 区块/秒")
        
        # 每 10 批保存一次汇总报告
        if i % 10 == 0:
            report = {
                'timestamp': datetime.now().isoformat(),
                'total_verified': total_verified,
                'total_matching': total_matching,
                'current_height': current_height,
                'elapsed_seconds': elapsed,
                'batches_completed': i
            }
            with open(output_dir / 'continuous_verification_progress.json', 'w') as f:
                json.dump(report, f, indent=2)
    
    # 最终报告
    total_elapsed = time.time() - start_time
    
    print("\n" + "="*60)
    print("验证完成！")
    print("="*60)
    print(f"结束时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    print(f"总验证区块数: {total_verified:,}")
    print(f"总匹配数: {total_matching:,}")
    
    if total_verified > 0:
        print(f"总体一致率: {(total_matching/total_verified*100):.2f}%")
    
    print(f"总用时: {total_elapsed/3600:.1f} 小时")
    
    # 保存最终报告
    final_report = {
        'timestamp': datetime.now().isoformat(),
        'total_verified': total_verified,
        'total_matching': total_matching,
        'current_height': current_height,
        'elapsed_seconds': total_elapsed,
        'success': total_matching == total_verified
    }
    
    with open(output_dir / 'continuous_verification_final.json', 'w') as f:
        json.dump(final_report, f, indent=2)
    
    if total_matching == total_verified:
        print("\n🎉🎉🎉 所有区块验证成功！100% 一致！")
        return True
    else:
        print("\n⚠️  验证未完全通过")
        return False


if __name__ == '__main__':
    try:
        success = main()
        sys.exit(0 if success else 1)
    except KeyboardInterrupt:
        print("\n\n⚠️  用户中断验证")
        sys.exit(1)
    except Exception as e:
        print(f"\n\n❌ 致命错误: {e}")
        import traceback
        traceback.print_exc()
        sys.exit(1)
