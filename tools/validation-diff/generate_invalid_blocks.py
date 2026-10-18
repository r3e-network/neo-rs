#!/usr/bin/env python3
"""
生成无效区块测试向量

基于有效的 MainNet 区块，通过修改关键字段生成无效变种。
"""

import json
import random
from pathlib import Path
from typing import List, Dict


def modify_timestamp(block_hex: str, invalid_type: str) -> str:
    """修改区块的 timestamp 使其无效"""
    # Block 格式：version(4) + prev_hash(32) + merkle_root(32) + timestamp(8) + ...
    # timestamp 从字节 68 开始（4+32+32）
    
    pos = (4 + 32 + 32) * 2  # 转换为十六进制字符位置
    
    if invalid_type == 'timestamp_too_early':
        # 设置为 0（早于创世块时间）
        new_timestamp = b'\x00' * 8
    elif invalid_type == 'timestamp_too_late':
        # 设置为未来很远的时间（年份 2100）
        new_timestamp = (4102444800000).to_bytes(8, 'little')  # 2100-01-01
    else:
        return block_hex
    
    return block_hex[:pos] + new_timestamp.hex() + block_hex[pos + 16:]


def modify_version(block_hex: str) -> str:
    """修改区块的 version 使其无效"""
    # version 从字节 0 开始
    # Neo N3 区块 version 必须是 0
    invalid_version = (1).to_bytes(4, 'little')
    return invalid_version.hex() + block_hex[8:]


def modify_nonce(block_hex: str) -> str:
    """修改区块的 nonce（冲突测试）"""
    # nonce 从字节 72 开始（4+32+32+8）
    pos = (4 + 32 + 32 + 8) * 2
    
    # 设置为一个可能冲突的 nonce
    invalid_nonce = (0xFFFFFFFFFFFFFFFF).to_bytes(8, 'little')
    return block_hex[:pos] + invalid_nonce.hex() + block_hex[pos + 16:]


def generate_invalid_blocks(valid_blocks_path: Path, output_path: Path, count: int = 30):
    """生成无效区块测试向量"""
    with open(valid_blocks_path, 'r') as f:
        valid_blocks = json.load(f)
    
    if len(valid_blocks) == 0:
        print("Error: No valid blocks to base invalid blocks on")
        return
    
    invalid_blocks = []
    
    # 生成各种类型的无效区块
    invalid_types = [
        ('timestamp_too_early', '时间戳过早（早于创世块）'),
        ('timestamp_too_late', '时间戳过晚（未来很远）'),
        ('version_invalid', '版本号错误（非 0）'),
        ('nonce_conflict', 'Nonce 冲突'),
    ]
    
    for i in range(count):
        # 随机选择一个有效区块作为基础
        base_block = random.choice(valid_blocks)
        
        # 随机选择一种无效类型
        invalid_type, description = random.choice(invalid_types)
        
        # 修改区块
        if invalid_type == 'timestamp_too_early':
            modified_hex = modify_timestamp(base_block['block_hex'], 'timestamp_too_early')
        elif invalid_type == 'timestamp_too_late':
            modified_hex = modify_timestamp(base_block['block_hex'], 'timestamp_too_late')
        elif invalid_type == 'version_invalid':
            modified_hex = modify_version(base_block['block_hex'])
        elif invalid_type == 'nonce_conflict':
            modified_hex = modify_nonce(base_block['block_hex'])
        else:
            continue
        
        invalid_blocks.append({
            'test_id': f'invalid_block_{i}_{invalid_type}',
            'block_hex': modified_hex,
            'expected_valid': False,
            'description': f'基于 MainNet 区块 {base_block["description"]}，{description}'
        })
    
    with open(output_path, 'w') as f:
        json.dump(invalid_blocks, f, indent=2)
    
    print(f"Generated {len(invalid_blocks)} invalid block test vectors to {output_path}")


def combine_test_vectors(valid_path: Path, invalid_path: Path, output_path: Path):
    """合并有效和无效测试向量"""
    with open(valid_path, 'r') as f:
        valid = json.load(f)
    
    with open(invalid_path, 'r') as f:
        invalid = json.load(f)
    
    combined = valid + invalid
    
    # 随机打乱顺序
    random.shuffle(combined)
    
    with open(output_path, 'w') as f:
        json.dump(combined, f, indent=2)
    
    print(f"Combined {len(valid)} valid + {len(invalid)} invalid = {len(combined)} total test vectors")
    print(f"Saved to {output_path}")


def main():
    output_dir = Path('tools/validation-diff/test_vectors')
    
    # 生成无效区块
    generate_invalid_blocks(
        output_dir / 'valid_blocks.json',
        output_dir / 'invalid_blocks.json',
        count=30
    )
    
    # 合并所有区块测试向量
    combine_test_vectors(
        output_dir / 'valid_blocks.json',
        output_dir / 'invalid_blocks.json',
        output_dir / 'all_blocks.json'
    )
    
    print("\n✅ Test vectors ready!")
    print(f"Total: 50 block test vectors (20 valid + 30 invalid)")
    print("\nNext step: Run validators and compare results")


if __name__ == '__main__':
    main()
