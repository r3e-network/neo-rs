$names = @('build_trie_from_hex_pairs','check_local_roots','check_unclaimed_gas','count_state_at_root','disassemble_contract_script','disassemble_tx','dump_block_header','dump_block_txs','dump_genesis_trie','dump_state_root','dump_tx_full','dump_tx_raw','find_txs_by_script_hex','find_txs_by_script_text','gas_balance_at_root','get_state_at_root','inspect_contract_by_id','inspect_contract_state','inspect_gas_balance','inspect_neo_account_state','list_raw_prefix','list_storage_prefix','print_height','print_storage_key','print_tx_state','rebuild_trie_4410','replay_tx_once','scan_sender_txs','serialization_diff_runner','signature_verify_diff_runner','vm_syscall_runner','witness_verify_diff_runner')
$dirs = @('scripts','tools','docs','Makefile','.github','fuzz','benches-package','monitoring','deployment','config','tests')
$out = foreach ($n in $names) {
  $hits = Get-ChildItem -Path $dirs -Recurse -File -ErrorAction SilentlyContinue | Select-String -Pattern ([regex]::Escape($n)) -List -ErrorAction SilentlyContinue
  if ($hits) {
    "REFERENCED: $n :: " + (($hits | ForEach-Object { $_.Path.Replace('D:\Git\neo-rs\','') }) -join ' | ')
  } else {
    "no-ref: $n"
  }
}
$out | Set-Content -Path D:\Git\neo-rs\.task228-example-refs.txt -Encoding UTF8
Write-Host "done"
