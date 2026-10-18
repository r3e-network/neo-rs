#!/usr/bin/env python3
"""
继续验证运行器 - 从当前进度继续验证

自动检测已完成的区块，从下一个区块开始继续验证
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


def find_max_verified_block(results_dir):
    """找到已验证的最大区块号"""
    max_block = 0
    
    for file in results_dir.glob('c2c_*.json'):
        try:
            with open(file, 'r') as f:
                data = json.load(f)
                # 从文件名或数据中提取区块范围
                if 'end_block' in data:
                    max_block = max(max_block, data['end_block'])
                elif 'blocks' in data and len(data['blocks']) > 0:
                    max_block = max(max_block, max(b['block_number'] for b in data['blocks']))
        except:
            continue
    
    # 也检查文件名中的范围
    for file in results_dir.glob('c2c_batch_*.json'):
        try:
            name = file.stem.replace('c2c_batch_', '')
            if '_' in name:
                end = int(name.split('_')[1])
                max_block = max(max_block, end)
        except:
            continue
    
    return max_block


def verify_batch(start, end, output_file):
    """验证一批区块"""
    print(f"\n{'='*70}")
    print(f"验证: {start:,} - {end:,} ({end-start+1:,} 个区块)")
    print(f"时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
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
            
            print(f"\n✅ 完成")
            print(f"   总数: {data['total']:,}")
            print(f"   匹配: {data['matching']:,}")
            print(f"   一致率: {rate:.2f}%")
            print(f"   用时: {elapsed/60:.1f} 分钟")
            
            return data['matching'] == data['total']
        else:
            print(f"\n❌ 失败")
            return False
            
    except subprocess.TimeoutExpired:
        print(f"\n❌ 超时")
        return False
    except Exception as e:
        print(f"\n❌ 错误: {e}")
        return False


def main():
    print("="*70)
    print("继续验证运行器")
    print("="*70)
    print(f"启动时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
    
    output_dir = Path('tools/state-root-diff/results')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # 获取当前链高
    rpc_url = 'http://seed3.neo.org:10332'
    current_height = get_current_height(rpc_url)
    if not current_height:
        print("❌ 无法获取链高")
        return False
    
    print(f"MainNet 当前链高: {current_height:,}")
    
    # 找到已验证的最大区块
    max_verified = find_max_verified_block(output_dir)
    print(f"已验证到区块: {max_verified:,}\n")
    
    # 从下一个区块开始
    current = max_verified + 1
    batch_size = 5000
    
    print(f"继续从区块 {current:,} 开始验证")
    print(f"批次大小: {batch_size:,} 个区块\n")
    
    total_verified = max_verified
    
    # 持续验证
    while current < current_height:
        end = min(current + batch_size - 1, current_height - 1)
        batch_name = f'batch_{current}_{end}'
        output_file = output_dir / f'c2c_{batch_name}.json'
        
        # 跳过已完成的
        if output_file.exists():
            print(f"[跳过] {batch_name} (已存在)")
            current = end + 1
            continue
        
        success = verify_batch(current, end, output_file)
        
        if not success:
            print(f"\n❌ 验证失败，停止")
            break
        
        total_verified = end
        current = end + 1
        
        # 显示进度
        progress = (total_verified / current_height * 100)
        print(f"\n📊 总进度: {total_verified:,} / {current_height:,} ({progress:.2f}%)")
    
    print("\n" + "="*70)
    print("验证会话结束")
    print("="*70)
    print(f"最终验证到: {total_verified:,}")
    print(f"完成时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}")
    
    return True


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
