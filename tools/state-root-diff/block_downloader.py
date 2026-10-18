#!/usr/bin/env python3
"""
MainNet 区块下载器

从 Neo N3 MainNet RPC 节点下载指定范围的区块数据。
"""

import json
import requests
import time
from pathlib import Path
from typing import List, Dict, Optional


class BlockDownloader:
    def __init__(self, rpc_url: str = "http://seed1.neo.org:10332"):
        self.rpc_url = rpc_url
        self.session = requests.Session()
        self.request_id = 0
    
    def _rpc_call(self, method: str, params: List) -> Optional[Dict]:
        """执行 RPC 调用"""
        self.request_id += 1
        
        payload = {
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": self.request_id
        }
        
        try:
            response = self.session.post(self.rpc_url, json=payload, timeout=30)
            response.raise_for_status()
            result = response.json()
            
            if "error" in result:
                print(f"RPC error: {result['error']}")
                return None
            
            return result.get("result")
        except Exception as e:
            print(f"Request failed: {e}")
            return None
    
    def get_block_count(self) -> Optional[int]:
        """获取当前区块高度"""
        result = self._rpc_call("getblockcount", [])
        return result if result else None
    
    def get_block(self, height: int, verbose: bool = True) -> Optional[Dict]:
        """获取指定高度的区块"""
        result = self._rpc_call("getblock", [height, verbose])
        return result if result else None
    
    def download_blocks(self, start: int, end: int, output_file: Path, batch_size: int = 100):
        """下载指定范围的区块并保存到文件"""
        blocks = []
        
        print(f"Downloading blocks {start} to {end} from {self.rpc_url}...")
        
        for height in range(start, end + 1):
            # 显示进度
            if height % 10 == 0:
                print(f"Progress: {height - start + 1}/{end - start + 1} blocks downloaded")
            
            # 获取区块
            block = self.get_block(height, verbose=True)
            if block:
                blocks.append({
                    'height': height,
                    'hash': block.get('hash'),
                    'size': block.get('size'),
                    'time': block.get('time'),
                    'tx_count': len(block.get('tx', [])),
                    'block_data': block
                })
            else:
                print(f"Failed to download block {height}")
                break
            
            # 批量保存（避免全部丢失）
            if len(blocks) % batch_size == 0:
                self._save_checkpoint(blocks, output_file)
            
            # 速率限制
            time.sleep(0.1)
        
        # 最终保存
        self._save_blocks(blocks, output_file)
        
        print(f"\n✅ Downloaded {len(blocks)} blocks")
        print(f"Saved to {output_file}")
        
        return len(blocks)
    
    def _save_checkpoint(self, blocks: List[Dict], output_file: Path):
        """保存检查点（追加模式）"""
        checkpoint_file = output_file.parent / f"{output_file.stem}_checkpoint.json"
        with open(checkpoint_file, 'w') as f:
            json.dump({'blocks': blocks, 'count': len(blocks)}, f)
    
    def _save_blocks(self, blocks: List[Dict], output_file: Path):
        """保存区块到文件"""
        output_file.parent.mkdir(parents=True, exist_ok=True)
        
        data = {
            'network': 'mainnet',
            'rpc_url': self.rpc_url,
            'downloaded_at': time.strftime('%Y-%m-%d %H:%M:%S'),
            'start_height': blocks[0]['height'] if blocks else 0,
            'end_height': blocks[-1]['height'] if blocks else 0,
            'block_count': len(blocks),
            'blocks': blocks
        }
        
        with open(output_file, 'w') as f:
            json.dump(data, f, indent=2)


def main():
    import argparse
    
    parser = argparse.ArgumentParser(description='Download Neo N3 MainNet blocks')
    parser.add_argument('--start', type=int, default=0, help='Start block height')
    parser.add_argument('--end', type=int, default=999, help='End block height')
    parser.add_argument('--rpc', type=str, default='http://seed1.neo.org:10332', help='RPC URL')
    parser.add_argument('--output', type=str, help='Output file path')
    
    args = parser.parse_args()
    
    # 默认输出文件名
    if not args.output:
        args.output = f'tools/state-root-diff/data/blocks_{args.start}_{args.end}.json'
    
    downloader = BlockDownloader(rpc_url=args.rpc)
    
    # 检查连接
    print(f"Connecting to {args.rpc}...")
    block_count = downloader.get_block_count()
    if block_count:
        print(f"✅ Connected. Current block height: {block_count}")
    else:
        print("❌ Failed to connect to RPC node")
        return
    
    # 验证范围
    if args.end >= block_count:
        print(f"⚠️  Warning: End height {args.end} >= current height {block_count}")
        print(f"Adjusting end height to {block_count - 1}")
        args.end = block_count - 1
    
    # 下载区块
    output_file = Path(args.output)
    downloader.download_blocks(args.start, args.end, output_file)


if __name__ == '__main__':
    main()
