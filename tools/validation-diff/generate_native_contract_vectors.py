#!/usr/bin/env python3
"""
生成原生合约调用测试向量

从常见的原生合约调用生成测试向量，用于验证 Rust 和 C# 的输出一致性。
"""

import json
from pathlib import Path
from typing import List, Dict


def generate_neo_token_vectors() -> List[Dict]:
    """生成 NeoToken 合约调用向量"""
    vectors = []
    
    # NeoToken 合约哈希：0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5
    neo_token_hash = "0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5"
    
    # 1. symbol() - 获取符号
    vectors.append({
        'test_id': 'neo_token_symbol',
        'contract_hash': neo_token_hash,
        'method': 'symbol',
        'parameters': [],
        'expected_type': 'String',
        'description': 'NeoToken.symbol() should return "NEO"'
    })
    
    # 2. decimals() - 获取精度
    vectors.append({
        'test_id': 'neo_token_decimals',
        'contract_hash': neo_token_hash,
        'method': 'decimals',
        'parameters': [],
        'expected_type': 'Integer',
        'description': 'NeoToken.decimals() should return 0'
    })
    
    # 3. totalSupply() - 获取总供应量
    vectors.append({
        'test_id': 'neo_token_total_supply',
        'contract_hash': neo_token_hash,
        'method': 'totalSupply',
        'parameters': [],
        'expected_type': 'Integer',
        'description': 'NeoToken.totalSupply() should return 100000000'
    })
    
    # 4. balanceOf(account) - 查询余额
    # 使用一个已知地址（创世多签地址）
    genesis_account = "NXV7ZhHiyM1aHXwpVsRZC6BwNFP2jghXAq"
    vectors.append({
        'test_id': 'neo_token_balance_of_genesis',
        'contract_hash': neo_token_hash,
        'method': 'balanceOf',
        'parameters': [
            {'type': 'Hash160', 'value': genesis_account}
        ],
        'expected_type': 'Integer',
        'description': f'NeoToken.balanceOf({genesis_account})'
    })
    
    return vectors


def generate_gas_token_vectors() -> List[Dict]:
    """生成 GasToken 合约调用向量"""
    vectors = []
    
    # GasToken 合约哈希：0xd2a4cff31913016155e38e474a2c06d08be276cf
    gas_token_hash = "0xd2a4cff31913016155e38e474a2c06d08be276cf"
    
    # 1. symbol()
    vectors.append({
        'test_id': 'gas_token_symbol',
        'contract_hash': gas_token_hash,
        'method': 'symbol',
        'parameters': [],
        'expected_type': 'String',
        'description': 'GasToken.symbol() should return "GAS"'
    })
    
    # 2. decimals()
    vectors.append({
        'test_id': 'gas_token_decimals',
        'contract_hash': gas_token_hash,
        'method': 'decimals',
        'parameters': [],
        'expected_type': 'Integer',
        'description': 'GasToken.decimals() should return 8'
    })
    
    # 3. totalSupply()
    vectors.append({
        'test_id': 'gas_token_total_supply',
        'contract_hash': gas_token_hash,
        'method': 'totalSupply',
        'parameters': [],
        'expected_type': 'Integer',
        'description': 'GasToken.totalSupply()'
    })
    
    return vectors


def generate_policy_contract_vectors() -> List[Dict]:
    """生成 PolicyContract 合约调用向量"""
    vectors = []
    
    # PolicyContract 合约哈希：0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b
    policy_hash = "0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b"
    
    # 1. getFeePerByte()
    vectors.append({
        'test_id': 'policy_get_fee_per_byte',
        'contract_hash': policy_hash,
        'method': 'getFeePerByte',
        'parameters': [],
        'expected_type': 'Integer',
        'description': 'PolicyContract.getFeePerByte() should return 1000'
    })
    
    # 2. getExecFeeFactor()
    vectors.append({
        'test_id': 'policy_get_exec_fee_factor',
        'contract_hash': policy_hash,
        'method': 'getExecFeeFactor',
        'parameters': [],
        'expected_type': 'Integer',
        'description': 'PolicyContract.getExecFeeFactor() should return 30'
    })
    
    # 3. getStoragePrice()
    vectors.append({
        'test_id': 'policy_get_storage_price',
        'contract_hash': policy_hash,
        'method': 'getStoragePrice',
        'parameters': [],
        'expected_type': 'Integer',
        'description': 'PolicyContract.getStoragePrice() should return 100000'
    })
    
    # 4. isBlocked(account)
    vectors.append({
        'test_id': 'policy_is_blocked_zero',
        'contract_hash': policy_hash,
        'method': 'isBlocked',
        'parameters': [
            {'type': 'Hash160', 'value': '0x0000000000000000000000000000000000000000'}
        ],
        'expected_type': 'Boolean',
        'description': 'PolicyContract.isBlocked(zero_address) should return false'
    })
    
    return vectors


def generate_ledger_contract_vectors() -> List[Dict]:
    """生成 LedgerContract 合约调用向量"""
    vectors = []
    
    # LedgerContract 合约哈希：0xda65b600f7124ce6c79950c1772a36403104f2be
    ledger_hash = "0xda65b600f7124ce6c79950c1772a36403104f2be"
    
    # 1. currentIndex()
    vectors.append({
        'test_id': 'ledger_current_index',
        'contract_hash': ledger_hash,
        'method': 'currentIndex',
        'parameters': [],
        'expected_type': 'Integer',
        'description': 'LedgerContract.currentIndex()'
    })
    
    # 2. currentHash()
    vectors.append({
        'test_id': 'ledger_current_hash',
        'contract_hash': ledger_hash,
        'method': 'currentHash',
        'parameters': [],
        'expected_type': 'Hash256',
        'description': 'LedgerContract.currentHash()'
    })
    
    # 3. getBlock(0) - 创世块
    vectors.append({
        'test_id': 'ledger_get_block_0',
        'contract_hash': ledger_hash,
        'method': 'getBlock',
        'parameters': [
            {'type': 'Integer', 'value': 0}
        ],
        'expected_type': 'Array',
        'description': 'LedgerContract.getBlock(0) - genesis block'
    })
    
    return vectors


def generate_std_lib_vectors() -> List[Dict]:
    """生成 StdLib 合约调用向量"""
    vectors = []
    
    # StdLib 合约哈希：0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0
    stdlib_hash = "0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0"
    
    # 1. base64Encode()
    vectors.append({
        'test_id': 'stdlib_base64_encode',
        'contract_hash': stdlib_hash,
        'method': 'base64Encode',
        'parameters': [
            {'type': 'ByteArray', 'value': 'hello'}
        ],
        'expected_type': 'String',
        'description': 'StdLib.base64Encode("hello") should return "aGVsbG8="'
    })
    
    # 2. base64Decode()
    vectors.append({
        'test_id': 'stdlib_base64_decode',
        'contract_hash': stdlib_hash,
        'method': 'base64Decode',
        'parameters': [
            {'type': 'String', 'value': 'aGVsbG8='}
        ],
        'expected_type': 'ByteArray',
        'description': 'StdLib.base64Decode("aGVsbG8=") should return "hello"'
    })
    
    # 3. itoa()
    vectors.append({
        'test_id': 'stdlib_itoa',
        'contract_hash': stdlib_hash,
        'method': 'itoa',
        'parameters': [
            {'type': 'Integer', 'value': 123},
            {'type': 'Integer', 'value': 10}
        ],
        'expected_type': 'String',
        'description': 'StdLib.itoa(123, 10) should return "123"'
    })
    
    return vectors


def main():
    output_dir = Path('tools/validation-diff/test_vectors')
    output_dir.mkdir(parents=True, exist_ok=True)
    
    # 生成所有向量
    all_vectors = []
    all_vectors.extend(generate_neo_token_vectors())
    all_vectors.extend(generate_gas_token_vectors())
    all_vectors.extend(generate_policy_contract_vectors())
    all_vectors.extend(generate_ledger_contract_vectors())
    all_vectors.extend(generate_std_lib_vectors())
    
    # 保存
    output_file = output_dir / 'native_contract_calls.json'
    with open(output_file, 'w') as f:
        json.dump(all_vectors, f, indent=2)
    
    print(f"Generated {len(all_vectors)} native contract call test vectors")
    print(f"Saved to {output_file}")
    
    # 统计
    by_contract = {}
    for v in all_vectors:
        contract = v['contract_hash']
        by_contract[contract] = by_contract.get(contract, 0) + 1
    
    print("\nBreakdown by contract:")
    contract_names = {
        "0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5": "NeoToken",
        "0xd2a4cff31913016155e38e474a2c06d08be276cf": "GasToken",
        "0xcc5e4edd9f5f8dba8bb65734541df7a1c081c67b": "PolicyContract",
        "0xda65b600f7124ce6c79950c1772a36403104f2be": "LedgerContract",
        "0xacce6fd80d44e1796aa0c2c625e9e4e0ce39efc0": "StdLib"
    }
    
    for contract_hash, count in by_contract.items():
        name = contract_names.get(contract_hash, "Unknown")
        print(f"  {name}: {count} calls")


if __name__ == '__main__':
    main()
