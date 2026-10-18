#!/usr/bin/env python3
"""
从 MainNet 区块中提取测试向量

从已下载的 mainnet_blocks.json 中提取区块和交易的十六进制数据，
生成验证测试向量。
"""

import json
import random
from pathlib import Path


def extract_block_vectors(blocks_json_path: Path, output_path: Path, count: int = 20):
    """从 MainNet 区块中提取有效区块测试向量"""
    with open(blocks_json_path, 'r') as f:
        data = json.load(f)
    
    blocks = data.get('blocks', [])
    
    # 随机采样 count 个区块
    sampled = random.sample(blocks, min(count, len(blocks)))
    
    test_vectors = []
    for i, block in enumerate(sampled):
        test_vectors.append({
            'test_id': f'mainnet_block_{block["height"]}',
            'block_hex': block['block_hex'],
            'expected_valid': True,
            'description': f'MainNet block at height {block["height"]}'
        })
    
    with open(output_path, 'w') as f:
        json.dump(test_vectors, f, indent=2)
    
    print(f"Extracted {len(test_vectors)} valid block test vectors to {output_path}")


def extract_transaction_vectors(blocks_json_path: Path, output_path: Path, count: int = 400):
    """从 MainNet 区块中提取有效交易测试向量"""
    with open(blocks_json_path, 'r') as f:
        data = json.load(f)
    
    blocks = data.get('blocks', [])
    
    # 收集所有交易
    all_transactions = []
    for block in blocks:
        # 从 block_hex 中提取交易需要完整的区块解析
        # 简化：如果有 transactions 字段，直接使用
        if 'transactions' in block:
            for tx in block['transactions']:
                all_transactions.append({
                    'block_height': block['height'],
                    'tx_hash': tx.get('hash', 'unknown'),
                    'tx_hex': tx.get('hex', '')
                })
    
    # 随机采样
    sampled = random.sample(all_transactions, min(count, len(all_transactions)))
    
    test_vectors = []
    for i, tx in enumerate(sampled):
        test_vectors.append({
            'test_id': f'mainnet_tx_{tx["block_height"]}_{i}',
            'tx_hex': tx['tx_hex'],
            'expected_valid': True,
            'description': f'MainNet transaction from block {tx["block_height"]}'
        })
    
    with open(output_path, 'w') as f:
        json.dump(test_vectors, f, indent=2)
    
    print(f"Extracted {len(test_vectors)} valid transaction test vectors to {output_path}")


def main():
    # 输入：已下载的 MainNet 区块
    blocks_json = Path('neo-core/tests/protocol_compliance/test_vectors/mainnet_blocks.json')
    
    if not blocks_json.exists():
        print(f"Error: {blocks_json} not found")
        print("Please run the block downloader first to get MainNet blocks")
        return
    
    # 输出：测试向量
    output_dir = Path('tools/validation-diff/test_vectors')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # 提取有效区块向量（20 个）
    extract_block_vectors(
        blocks_json,
        output_dir / 'valid_blocks.json',
        count=20
    )
    
    # 提取有效交易向量（400 个）
    extract_transaction_vectors(
        blocks_json,
        output_dir / 'valid_transactions.json',
        count=400
    )
    
    print("\nNext steps:")
    print("1. Generate invalid test vectors (30 blocks + 100 transactions)")
    print("2. Run Rust validator: cargo run --bin rust-validator validate-block ...")
    print("3. Run C# validator: dotnet run -- validate-block ...")
    print("4. Compare results: python diff_runner.py ...")


if __name__ == '__main__':
    main()
