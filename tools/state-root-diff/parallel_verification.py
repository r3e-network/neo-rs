#!/usr/bin/env python3
"""
加速验证运行器 - 启动多个并行验证进程

策略：同时验证多个不重叠的区块范围
"""

import subprocess
import sys
import time
from pathlib import Path
from datetime import datetime


def start_verification_range(start, end, output_name):
    """启动一个验证进程"""
    cmd = [
        'python',
        'tools/state-root-diff/verify_state_roots_rpc.py',
        '--rust-rpc', 'http://seed2.neo.org:10332',
        '--csharp-rpc', 'http://seed3.neo.org:10332',
        '--start', str(start),
        '--end', str(end),
        '--output', f'tools/state-root-diff/results/{output_name}.json'
    ]
    
    log_file = f'tools/state-root-diff/results/{output_name}.log'
    
    with open(log_file, 'w') as f:
        process = subprocess.Popen(
            cmd,
            stdout=f,
            stderr=subprocess.STDOUT,
            cwd='D:\\Git\\neo-rs'
        )
    
    return process, log_file


def main():
    print("="*60)
    print("加速验证运行器 - 并行验证")
    print("="*60)
    print(f"启动时间: {datetime.now().strftime('%Y-%m-%d %H:%M:%S')}\n")
    
    # 定义并行验证范围（避免与已完成的重叠）
    ranges = [
        (10000, 19999, 'parallel_10k_20k'),
        (20000, 29999, 'parallel_20k_30k'),
        (30000, 39999, 'parallel_30k_40k'),
        (40000, 49999, 'parallel_40k_50k'),
    ]
    
    processes = []
    
    for start, end, name in ranges:
        print(f"启动: {name} ({start:,}-{end:,})")
        proc, log = start_verification_range(start, end, name)
        processes.append((name, proc, log))
        time.sleep(2)  # 避免同时启动导致资源竞争
    
    print(f"\n已启动 {len(processes)} 个并行验证进程\n")
    print("监控命令:")
    for name, _, log in processes:
        print(f"  tail -f {log}")
    
    print("\n验证将在后台持续运行...")
    
    return 0


if __name__ == '__main__':
    sys.exit(main())
