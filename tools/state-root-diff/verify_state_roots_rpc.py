#!/usr/bin/env python3
"""
简化的状态根验证器

使用 RPC 从运行中的节点查询状态根并比对。
不需要重新实现完整的区块处理逻辑。
"""

import json
import requests
import time
from pathlib import Path
from typing import Optional, List, Dict


class StateRootVerifier:
    def __init__(self, rust_rpc: str, csharp_rpc: str):
        self.rust_rpc = rust_rpc
        self.csharp_rpc = csharp_rpc
        self.session = requests.Session()
        self.request_id = 0
    
    def _rpc_call(self, rpc_url: str, method: str, params: List) -> Optional[Dict]:
        """执行 RPC 调用"""
        self.request_id += 1
        
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": self.request_id
        }
        
        try:
            response = self.session.post(rpc_url, json=payload, timeout=30)
            response.raise_for_status()
            result = response.json()
            
            if "error" in result:
                return {"error": result['error']}
            
            return result.get("result")
        except Exception as e:
            return {"error": str(e)}
    
    def get_state_root(self, rpc_url: str, height: int) -> Optional[Dict]:
        """获取指定高度的状态根"""
        return self._rpc_call(rpc_url, "getstateroot", [height])
    
    def get_block_count(self, rpc_url: str) -> Optional[int]:
        """获取当前区块高度"""
        result = self._rpc_call(rpc_url, "getblockcount", [])
        return result if isinstance(result, int) else None
    
    def verify_range(self, start: int, end: int, output_file: Path) -> Dict:
        """验证指定范围的状态根"""
        
        print(f"Verifying state roots from block {start} to {end}...")
        print(f"Rust RPC: {self.rust_rpc}")
        print(f"C# RPC: {self.csharp_rpc}")
        
        results = {
            'start_height': start,
            'end_height': end,
            'total': 0,
            'matching': 0,
            'mismatching': 0,
            'rust_errors': 0,
            'csharp_errors': 0,
            'divergence_point': None,
            'comparisons': []
        }
        
        for height in range(start, end + 1):
            if height % 100 == 0:
                print(f"Progress: {height - start + 1}/{end - start + 1}")
            
            # 获取 Rust 状态根
            rust_sr = self.get_state_root(self.rust_rpc, height)
            
            # 获取 C# 状态根
            csharp_sr = self.get_state_root(self.csharp_rpc, height)
            
            comparison = {
                'height': height,
                'rust_state_root': None,
                'csharp_state_root': None,
                'match': False,
                'rust_error': None,
                'csharp_error': None
            }
            
            # 检查错误
            if isinstance(rust_sr, dict) and 'error' in rust_sr:
                results['rust_errors'] += 1
                comparison['rust_error'] = rust_sr['error']
            else:
                comparison['rust_state_root'] = rust_sr.get('roothash') if rust_sr else None
            
            if isinstance(csharp_sr, dict) and 'error' in csharp_sr:
                results['csharp_errors'] += 1
                comparison['csharp_error'] = csharp_sr['error']
            else:
                comparison['csharp_state_root'] = csharp_sr.get('roothash') if csharp_sr else None
            
            # 比对
            if comparison['rust_state_root'] and comparison['csharp_state_root']:
                results['total'] += 1
                
                if comparison['rust_state_root'] == comparison['csharp_state_root']:
                    results['matching'] += 1
                    comparison['match'] = True
                else:
                    results['mismatching'] += 1
                    if results['divergence_point'] is None:
                        results['divergence_point'] = height
            
            results['comparisons'].append(comparison)
            
            # 短暂延迟避免压垮节点
            time.sleep(0.05)
        
        # 保存结果
        output_file.parent.mkdir(parents=True, exist_ok=True)
        with open(output_file, 'w') as f:
            json.dump(results, f, indent=2)
        
        return results


def main():
    import argparse
    
    parser = argparse.ArgumentParser(description='Verify state roots between Rust and C# nodes')
    parser.add_argument('--rust-rpc', type=str, default='http://127.0.0.1:30332', help='Rust node RPC URL')
    parser.add_argument('--csharp-rpc', type=str, default='http://127.0.0.1:10332', help='C# node RPC URL')
    parser.add_argument('--start', type=int, default=0, help='Start block height')
    parser.add_argument('--end', type=int, default=999, help='End block height')
    parser.add_argument('--output', type=str, default='tools/state-root-diff/results/verification_result.json', help='Output file')
    
    args = parser.parse_args()
    
    verifier = StateRootVerifier(args.rust_rpc, args.csharp_rpc)
    
    # 检查连接
    print("Checking connections...")
    rust_height = verifier.get_block_count(args.rust_rpc)
    csharp_height = verifier.get_block_count(args.csharp_rpc)
    
    print(f"Rust node: {'✅ Connected' if rust_height else '❌ Failed'} (height: {rust_height})")
    print(f"C# node: {'✅ Connected' if csharp_height else '❌ Failed'} (height: {csharp_height})")
    
    if not rust_height or not csharp_height:
        print("\n❌ Cannot connect to one or both nodes")
        print("\nPlease ensure both nodes are running:")
        print(f"  Rust node: {args.rust_rpc}")
        print(f"  C# node: {args.csharp_rpc}")
        return
    
    # 调整范围
    max_height = min(rust_height, csharp_height, args.end)
    if args.end > max_height:
        print(f"\n⚠️  Adjusting end height from {args.end} to {max_height}")
        args.end = max_height
    
    # 验证
    output_file = Path(args.output)
    results = verifier.verify_range(args.start, args.end, output_file)
    
    # 显示结果
    print(f"\n{'='*60}")
    print("VERIFICATION RESULTS")
    print(f"{'='*60}")
    print(f"Total comparisons: {results['total']}")
    print(f"Matching: {results['matching']}")
    print(f"Mismatching: {results['mismatching']}")
    print(f"Rust errors: {results['rust_errors']}")
    print(f"C# errors: {results['csharp_errors']}")
    
    if results['total'] > 0:
        match_rate = results['matching'] / results['total'] * 100
        print(f"Match rate: {match_rate:.2f}%")
    
    if results['divergence_point'] is not None:
        print(f"\n⚠️  First divergence at block {results['divergence_point']}")
    else:
        print("\n✅ All state roots match!")
    
    print(f"\nResults saved to: {output_file}")


if __name__ == '__main__':
    main()
