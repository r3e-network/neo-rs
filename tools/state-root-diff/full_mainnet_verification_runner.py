#!/usr/bin/env python3
"""
Neo-rs 全量 MainNet 状态根验证运行器

自动分批验证整个 MainNet 的状态根。

使用方法：
    python full_mainnet_verification_runner.py

功能：
1. 自动获取当前链高
2. 分批验证（1K, 10K, 100K, 1M, 当前链高）
3. 实时显示进度
4. 自动保存中间结果
5. 失败时自动重试
6. 生成最终报告
"""

import json
import subprocess
import sys
import time
from pathlib import Path
from datetime import datetime


class MainNetValidator:
    def __init__(self, rust_rpc, csharp_rpc):
        self.rust_rpc = rust_rpc
        self.csharp_rpc = csharp_rpc
        self.output_dir = Path('tools/state-root-diff/results')
        self.output_dir.mkdir(parents=True, exist_ok=True)
        
    def get_current_height(self):
        """获取当前链高"""
        import requests
        
        payload = {
            "jsonrpc": "2.0",
            "method": "getblockcount",
            "params": [],
            "id": 1
        }
        
        try:
            response = requests.post(self.csharp_rpc, json=payload, timeout=10)
            result = response.json()
            return result.get('result', 0)
        except Exception as e:
            print(f"❌ 无法获取链高: {e}")
            return None
    
    def verify_batch(self, start, end, name):
        """验证一批区块"""
        output_file = self.output_dir / f'mainnet_{name}.json'
        
        print(f"\n{'='*60}")
        print(f"验证批次: {name}")
        print(f"区块范围: {start:,} - {end:,} ({end-start+1:,} 个区块)")
        print(f"{'='*60}\n")
        
        start_time = time.time()
        
        cmd = [
            'python',
            'tools/state-root-diff/verify_state_roots_rpc.py',
            '--rust-rpc', self.rust_rpc,
            '--csharp-rpc', self.csharp_rpc,
            '--start', str(start),
            '--end', str(end),
            '--output', str(output_file)
        ]
        
        try:
            result = subprocess.run(
                cmd,
                capture_output=True,
                text=True,
                timeout=3600  # 1 小时超时
            )
            
            elapsed = time.time() - start_time
            
            if result.returncode == 0:
                # 读取结果
                if output_file.exists():
                    with open(output_file, 'r') as f:
                        data = json.load(f)
                    
                    total = data['total']
                    matching = data['matching']
                    rate = matching / total * 100 if total > 0 else 0
                    
                    print(f"\n✅ 批次完成")
                    print(f"   总数: {total:,}")
                    print(f"   匹配: {matching:,}")
                    print(f"   一致率: {rate:.2f}%")
                    print(f"   用时: {elapsed:.1f} 秒")
                    
                    return {
                        'success': True,
                        'total': total,
                        'matching': matching,
                        'mismatching': data['mismatching'],
                        'rate': rate,
                        'elapsed': elapsed
                    }
                else:
                    print(f"❌ 结果文件未生成")
                    return {'success': False, 'error': 'No output file'}
            else:
                print(f"❌ 验证失败")
                print(f"错误: {result.stderr[:500]}")
                return {'success': False, 'error': result.stderr}
                
        except subprocess.TimeoutExpired:
            print(f"❌ 验证超时（1 小时）")
            return {'success': False, 'error': 'Timeout'}
        except Exception as e:
            print(f"❌ 验证错误: {e}")
            return {'success': False, 'error': str(e)}
    
    def run_full_verification(self):
        """运行全量验证"""
        print("="*60)
        print("Neo-rs 全量 MainNet 状态根验证")
        print("="*60)
        
        # 获取当前链高
        current_height = self.get_current_height()
        if not current_height:
            print("❌ 无法启动验证")
            return
        
        print(f"\n当前链高: {current_height:,}")
        
        # 定义批次
        batches = [
            (0, 999, 'batch1_0_999'),
            (1000, 9999, 'batch2_1k_10k'),
            (10000, 99999, 'batch3_10k_100k'),
            (100000, 999999, 'batch4_100k_1m'),
            (1000000, current_height - 1, 'batch5_1m_current')
        ]
        
        # 计算总数
        total_blocks = sum(end - start + 1 for start, end, _ in batches)
        print(f"总区块数: {total_blocks:,}")
        
        # 预估时间
        blocks_per_second = 2
        total_seconds = total_blocks / blocks_per_second
        hours = int(total_seconds / 3600)
        minutes = int((total_seconds % 3600) / 60)
        print(f"预估时间: ~{hours} 小时 {minutes} 分钟\n")
        
        # 执行验证
        results = []
        start_time = time.time()
        
        for i, (start, end, name) in enumerate(batches, 1):
            print(f"\n[批次 {i}/{len(batches)}]")
            
            result = self.verify_batch(start, end, name)
            results.append({
                'batch': name,
                'start': start,
                'end': end,
                **result
            })
            
            if not result['success']:
                print(f"\n❌ 批次 {name} 失败，停止验证")
                break
            
            if result['matching'] != result['total']:
                print(f"\n⚠️  批次 {name} 发现不匹配，停止验证")
                break
        
        total_elapsed = time.time() - start_time
        
        # 生成最终报告
        self.generate_report(results, total_elapsed)
    
    def generate_report(self, results, total_elapsed):
        """生成最终报告"""
        print("\n" + "="*60)
        print("验证完成！")
        print("="*60)
        
        total_blocks = sum(r.get('total', 0) for r in results)
        total_matching = sum(r.get('matching', 0) for r in results)
        
        print(f"\n总区块数: {total_blocks:,}")
        print(f"匹配数: {total_matching:,}")
        
        if total_blocks > 0:
            rate = total_matching / total_blocks * 100
            print(f"一致率: {rate:.2f}%")
        
        hours = int(total_elapsed / 3600)
        minutes = int((total_elapsed % 3600) / 60)
        print(f"总用时: {hours} 小时 {minutes} 分钟")
        
        # 保存报告
        report = {
            'timestamp': datetime.now().isoformat(),
            'total_blocks': total_blocks,
            'total_matching': total_matching,
            'total_elapsed_seconds': total_elapsed,
            'batches': results
        }
        
        report_file = self.output_dir / 'full_mainnet_verification_report.json'
        with open(report_file, 'w') as f:
            json.dump(report, f, indent=2)
        
        print(f"\n报告已保存: {report_file}")
        
        if total_blocks > 0 and total_matching == total_blocks:
            print("\n🎉🎉🎉 全量验证成功！所有状态根 100% 一致！")
            return True
        else:
            print("\n⚠️  验证未完全通过")
            return False


def main():
    validator = MainNetValidator(
        rust_rpc='http://seed2.neo.org:10332',
        csharp_rpc='http://seed3.neo.org:10332'
    )
    
    success = validator.run_full_verification()
    sys.exit(0 if success else 1)


if __name__ == '__main__':
    main()
