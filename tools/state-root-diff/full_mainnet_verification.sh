#!/bin/bash
# Neo-rs 全量 MainNet 状态根验证脚本
# 
# 使用方法：
#   bash full_mainnet_verification.sh
#
# 或在 Windows PowerShell 中：
#   python full_mainnet_verification_runner.py

set -e

RUST_RPC="http://seed2.neo.org:10332"
CSHARP_RPC="http://seed3.neo.org:10332"
OUTPUT_DIR="tools/state-root-diff/results"

echo "=========================================="
echo "Neo-rs 全量 MainNet 状态根验证"
echo "=========================================="
echo ""

# 获取当前链高
CURRENT_HEIGHT=$(curl -s -X POST $CSHARP_RPC \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"getblockcount","params":[],"id":1}' \
  | jq -r '.result')

echo "当前链高: $CURRENT_HEIGHT"
echo ""

# 分批验证
BATCHES=(
  "0:999:batch1_0_999"
  "1000:9999:batch2_1k_10k"
  "10000:99999:batch3_10k_100k"
  "100000:999999:batch4_100k_1m"
  "1000000:$((CURRENT_HEIGHT-1)):batch5_1m_current"
)

for batch in "${BATCHES[@]}"; do
  IFS=':' read -r start end name <<< "$batch"
  
  echo "----------------------------------------"
  echo "验证批次: $name ($start - $end)"
  echo "----------------------------------------"
  
  python tools/state-root-diff/verify_state_roots_rpc.py \
    --rust-rpc $RUST_RPC \
    --csharp-rpc $CSHARP_RPC \
    --start $start \
    --end $end \
    --output "$OUTPUT_DIR/mainnet_$name.json"
  
  # 检查结果
  if [ -f "$OUTPUT_DIR/mainnet_$name.json" ]; then
    TOTAL=$(jq -r '.total' "$OUTPUT_DIR/mainnet_$name.json")
    MATCHING=$(jq -r '.matching' "$OUTPUT_DIR/mainnet_$name.json")
    RATE=$(echo "scale=2; $MATCHING * 100 / $TOTAL" | bc)
    
    echo ""
    echo "结果: $MATCHING/$TOTAL 匹配 ($RATE%)"
    echo ""
    
    if [ "$MATCHING" != "$TOTAL" ]; then
      echo "⚠️  发现不匹配，停止验证"
      exit 1
    fi
  else
    echo "❌ 结果文件未生成"
    exit 1
  fi
done

echo ""
echo "=========================================="
echo "✅ 全量验证完成！"
echo "=========================================="
