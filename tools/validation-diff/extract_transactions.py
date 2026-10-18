#!/usr/bin/env python3
"""
从 MainNet 区块十六进制中提取交易

解析 block_hex，提取其中的交易数据，生成测试向量。
"""

import json
import random
from pathlib import Path
from typing import List, Dict, Any


class BlockParser:
    """简单的区块解析器，从 block_hex 中提取交易"""
    
    def __init__(self, block_hex: str):
        self.hex = block_hex
        self.pos = 0
    
    def read_bytes(self, n: int) -> bytes:
        """读取 n 个字节"""
        result = bytes.fromhex(self.hex[self.pos:self.pos + n * 2])
        self.pos += n * 2
        return result
    
    def read_varint(self) -> int:
        """读取 varint"""
        first_byte = self.read_bytes(1)[0]
        if first_byte < 0xFD:
            return first_byte
        elif first_byte == 0xFD:
            return int.from_bytes(self.read_bytes(2), 'little')
        elif first_byte == 0xFE:
            return int.from_bytes(self.read_bytes(4), 'little')
        else:  # 0xFF
            return int.from_bytes(self.read_bytes(8), 'little')
    
    def parse_block_header(self) -> Dict[str, Any]:
        """解析区块头"""
        version = int.from_bytes(self.read_bytes(4), 'little')
        prev_hash = self.read_bytes(32)
        merkle_root = self.read_bytes(32)
        timestamp = int.from_bytes(self.read_bytes(8), 'little')
        nonce = int.from_bytes(self.read_bytes(8), 'little')
        index = int.from_bytes(self.read_bytes(4), 'little')
        primary_index = self.read_bytes(1)[0]
        next_consensus = self.read_bytes(20)
        
        return {
            'version': version,
            'prev_hash': prev_hash.hex(),
            'merkle_root': merkle_root.hex(),
            'timestamp': timestamp,
            'nonce': nonce,
            'index': index,
            'primary_index': primary_index,
            'next_consensus': next_consensus.hex()
        }
    
    def parse_witness(self) -> Dict[str, Any]:
        """解析见证"""
        invocation_len = self.read_varint()
        invocation = self.read_bytes(invocation_len)
        
        verification_len = self.read_varint()
        verification = self.read_bytes(verification_len)
        
        return {
            'invocation': invocation.hex(),
            'verification': verification.hex()
        }
    
    def parse_transaction(self) -> str:
        """解析单个交易，返回交易的十六进制"""
        tx_start = self.pos
        
        # 版本
        version = self.read_bytes(1)[0]
        
        # Nonce
        nonce = int.from_bytes(self.read_bytes(4), 'little')
        
        # System fee
        system_fee = int.from_bytes(self.read_bytes(8), 'little')
        
        # Network fee
        network_fee = int.from_bytes(self.read_bytes(8), 'little')
        
        # Valid until block
        valid_until_block = int.from_bytes(self.read_bytes(4), 'little')
        
        # Signers
        signers_count = self.read_varint()
        for _ in range(signers_count):
            account = self.read_bytes(20)  # UInt160
            scopes = self.read_bytes(1)[0]
            
            # 根据 scopes 读取额外数据
            if scopes & 0x01:  # CustomContracts
                contracts_count = self.read_varint()
                for _ in range(contracts_count):
                    self.read_bytes(20)
            
            if scopes & 0x02:  # CustomGroups
                groups_count = self.read_varint()
                for _ in range(groups_count):
                    self.read_bytes(33)
            
            if scopes & 0x04:  # Rules
                rules_count = self.read_varint()
                for _ in range(rules_count):
                    action = self.read_bytes(1)[0]
                    condition_type = self.read_bytes(1)[0]
                    # 简化：跳过条件数据（实际需要根据类型解析）
                    if condition_type == 0x01:  # Boolean
                        self.read_bytes(1)
                    elif condition_type == 0x18:  # ScriptHash
                        self.read_bytes(20)
                    elif condition_type == 0x19:  # Group
                        self.read_bytes(33)
                    # ... 其他类型
        
        # Attributes
        attributes_count = self.read_varint()
        for _ in range(attributes_count):
            attr_type = self.read_bytes(1)[0]
            
            if attr_type == 0x01:  # HighPriority
                pass
            elif attr_type == 0x11:  # OracleResponse
                id = int.from_bytes(self.read_bytes(8), 'little')
                code = self.read_bytes(1)[0]
                result_len = self.read_varint()
                result = self.read_bytes(result_len)
            elif attr_type == 0x20:  # NotValidBefore
                height = int.from_bytes(self.read_bytes(4), 'little')
            elif attr_type == 0x21:  # Conflicts
                hash = self.read_bytes(32)
            elif attr_type == 0x22:  # NotaryAssisted
                nkeys = self.read_bytes(1)[0]
        
        # Script
        script_len = self.read_varint()
        script = self.read_bytes(script_len)
        
        # Witnesses
        witnesses_count = self.read_varint()
        for _ in range(witnesses_count):
            invocation_len = self.read_varint()
            invocation = self.read_bytes(invocation_len)
            verification_len = self.read_varint()
            verification = self.read_bytes(verification_len)
        
        # 返回从 tx_start 到当前位置的完整交易十六进制
        tx_end = self.pos
        return self.hex[tx_start:tx_end]
    
    def parse_block(self) -> Dict[str, Any]:
        """解析完整区块"""
        header = self.parse_block_header()
        
        # 见证数量
        witnesses_count = self.read_varint()
        witnesses = []
        for _ in range(witnesses_count):
            witnesses.append(self.parse_witness())
        
        # 交易数量
        tx_count = self.read_varint()
        transactions = []
        for _ in range(tx_count):
            try:
                tx_hex = self.parse_transaction()
                transactions.append(tx_hex)
            except Exception as e:
                print(f"Warning: Failed to parse transaction {len(transactions)}: {e}")
                break
        
        return {
            'header': header,
            'witnesses': witnesses,
            'transactions': transactions
        }


def extract_transactions_from_blocks(blocks_json_path: Path, output_path: Path, count: int = 400):
    """从 MainNet 区块中提取交易测试向量"""
    with open(blocks_json_path, 'r') as f:
        data = json.load(f)
    
    blocks = data.get('blocks', [])
    
    # 收集所有交易
    all_transactions = []
    for block in blocks:
        block_hex = block['block_hex']
        block_height = block['height']
        tx_count = block.get('tx_count', 0)
        
        if tx_count == 0:
            continue
        
        try:
            parser = BlockParser(block_hex)
            parsed = parser.parse_block()
            
            for i, tx_hex in enumerate(parsed['transactions']):
                all_transactions.append({
                    'block_height': block_height,
                    'tx_index': i,
                    'tx_hex': tx_hex
                })
        except Exception as e:
            print(f"Warning: Failed to parse block {block_height}: {e}")
            continue
    
    print(f"Extracted {len(all_transactions)} transactions from {len(blocks)} blocks")
    
    # 随机采样
    if len(all_transactions) == 0:
        print("Warning: No transactions extracted!")
        test_vectors = []
    else:
        sampled = random.sample(all_transactions, min(count, len(all_transactions)))
        
        test_vectors = []
        for i, tx in enumerate(sampled):
            test_vectors.append({
                'test_id': f'mainnet_tx_block{tx["block_height"]}_idx{tx["tx_index"]}',
                'tx_hex': tx['tx_hex'],
                'expected_valid': True,
                'description': f'MainNet transaction from block {tx["block_height"]}, index {tx["tx_index"]}'
            })
    
    with open(output_path, 'w') as f:
        json.dump(test_vectors, f, indent=2)
    
    print(f"Saved {len(test_vectors)} valid transaction test vectors to {output_path}")
    return len(test_vectors)


def main():
    # 输入：已下载的 MainNet 区块
    blocks_json = Path('neo-core/tests/protocol_compliance/test_vectors/mainnet_blocks.json')
    
    if not blocks_json.exists():
        print(f"Error: {blocks_json} not found")
        return
    
    # 输出：测试向量
    output_dir = Path('tools/validation-diff/test_vectors')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # 提取交易向量
    tx_count = extract_transactions_from_blocks(
        blocks_json,
        output_dir / 'valid_transactions.json',
        count=400
    )
    
    if tx_count > 0:
        print(f"\n✅ Successfully extracted {tx_count} transaction test vectors")
    else:
        print("\n⚠️ No transactions extracted - will use block-only validation")
    
    print("\nNext steps:")
    print("1. Generate invalid test vectors (30 blocks + 100 transactions)")
    print("2. Run Rust validator: cargo run --bin rust-validator ...")
    print("3. Run C# validator: dotnet run -- validate-block ...")
    print("4. Compare results: python diff_runner.py ...")


if __name__ == '__main__':
    main()
