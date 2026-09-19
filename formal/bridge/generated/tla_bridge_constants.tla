\\ dBFT quorum + varint 语义常量 — 由 constants.json 生成器产出（协议规格层）
\\ f = (n-1)/3、M = n-f（同 neo-consensus context），varint 长度分档
N == 7
F == 2
M_ == 5
SafeQuorumBridge == (2 * M_ - N) > F

VarintLen(v) == CASE v < 253 -> 1
                  [] v <= 65535 -> 3
                  [] v <= 4294967295 -> 5
                  [] OTHER -> 9
