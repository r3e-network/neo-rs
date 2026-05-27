use neo_core::hardfork::Hardfork;
use neo_core::ledger::Block;
use neo_core::ledger::block_header::BlockHeader;
use neo_core::network::p2p::payloads::Witness;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::native::{NativeContract, TokenManagement};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::{UInt160, UInt256};
use num_bigint::BigInt;
use std::collections::HashMap;
use std::sync::Arc;

const TEST_GAS_LIMIT: i64 = 3_000_000_000;

use neo_core::smart_contract::call_flags::CallFlags;

fn protocol_settings_with_faun() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    hardforks.insert(Hardfork::HfFaun, 0);
    settings.hardforks = hardforks;
    settings
}

fn make_snapshot_with_genesis(settings: &ProtocolSettings) -> Arc<DataCache> {
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = neo_core::ledger::create_genesis_block(settings);

    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("on persist engine");
    on_persist.native_on_persist().expect("native on persist");

    let mut post_persist = ApplicationEngine::new(
        TriggerType::PostPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("post persist engine");
    post_persist
        .native_post_persist()
        .expect("native post persist");

    snapshot
}

fn make_block(index: u32) -> Block {
    let header = BlockHeader {
        index,
        previous_hash: UInt256::zero(),
        merkle_root: UInt256::zero(),
        timestamp: 1,
        nonce: 0,
        primary_index: 0,
        next_consensus: UInt160::zero(),
        witnesses: vec![Witness::empty()],
        ..Default::default()
    };
    Block::new(header, Vec::new())
}

fn sample_account(tag: u8) -> UInt160 {
    let bytes = [tag; 20];
    UInt160::from_bytes(&bytes).unwrap()
}

fn collect_iterator_keys(engine: &mut ApplicationEngine, iterator_result: &[u8]) -> Vec<Vec<u8>> {
    let iterator_id = u32::from_le_bytes(iterator_result.try_into().expect("iterator id length"));
    let mut keys = Vec::new();
    while engine
        .iterator_next_internal(iterator_id)
        .expect("iterator next")
    {
        let value = engine
            .iterator_value_internal(iterator_id)
            .expect("iterator value");
        keys.push(value.as_bytes().expect("iterator key bytes"));
    }
    keys
}

struct NftFixture {
    snapshot: Arc<DataCache>,
    token_mgmt: TokenManagement,
    owner: UInt160,
    engine: ApplicationEngine,
}

impl NftFixture {
    fn new() -> Self {
        let settings = protocol_settings_with_faun();
        let snapshot = make_snapshot_with_genesis(&settings);
        let token_mgmt = TokenManagement::new();
        let owner = sample_account(0x01);
        let block = make_block(1);
        let engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            Some(block),
            settings,
            TEST_GAS_LIMIT,
            None,
        )
        .expect("engine");

        Self {
            snapshot,
            token_mgmt,
            owner,
            engine,
        }
    }

    fn token_hash(&self) -> UInt160 {
        self.token_mgmt.hash()
    }

    fn call(&mut self, method: &str, args: &[Vec<u8>]) -> Vec<u8> {
        let token_hash = self.token_hash();
        self.engine
            .call_native_contract(token_hash, method, args)
            .unwrap_or_else(|err| panic!("{method} call: {err}"))
    }

    fn commit(&self) {
        self.snapshot.commit();
    }
}

#[test]
fn nft_create_and_mint() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"TestNFT";
    let symbol = b"TST";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    assert_eq!(result.len(), 20);

    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    let account = sample_account(0x02);
    let mint_args = vec![asset_id.to_bytes(), account.to_bytes()];

    let mint_result = fixture.call("mintNFT", &mint_args);
    assert_eq!(mint_result.len(), 20);

    let nft_id = UInt160::from_bytes(&mint_result).expect("nft id");
    assert!(!nft_id.is_zero());

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = fixture.call("getNFTInfo", &get_info_args);
    assert!(!info_result.is_empty());
}

#[test]
fn nft_burn() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"BurnableNFT";
    let symbol = b"BRN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let account = sample_account(0x02);
    let mint_args = vec![asset_id.to_bytes(), account.to_bytes()];

    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    fixture.engine.set_calling_script_hash(Some(account));

    fixture.commit();

    let burn_args = vec![nft_id.to_bytes()];

    let burn_result = fixture.call("burnNFT", &burn_args);
    assert_eq!(burn_result, vec![1]);

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = fixture.call("getNFTInfo", &get_info_args);
    assert!(info_result.is_empty());
}

#[test]
fn nft_transfer() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"TransferableNFT";
    let symbol = b"TRN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let from_account = sample_account(0x02);
    let to_account = sample_account(0x03);

    let mint_args = vec![asset_id.to_bytes(), from_account.to_bytes()];

    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    fixture.engine.set_calling_script_hash(Some(from_account));

    fixture.commit();

    let transfer_args = vec![
        nft_id.to_bytes(),
        from_account.to_bytes(),
        to_account.to_bytes(),
        Vec::new(),
    ];

    let transfer_result = fixture.call("transferNFT", &transfer_args);
    assert_eq!(transfer_result, vec![1]);

    let get_nfts_args = vec![asset_id.to_bytes()];

    let nfts_result = fixture.call("getNFTs", &get_nfts_args);
    assert!(!nfts_result.is_empty());
}

#[test]
fn direct_invoke_transfer_nft_rejects_extra_arguments() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let create_args = vec![
        owner.to_bytes(),
        b"ExtraArgNFT".to_vec(),
        b"EAN".to_vec(),
        vec![1],
    ];
    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x02);
    let recipient = sample_account(0x03);
    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    let err = fixture
        .token_mgmt
        .invoke(
            &mut fixture.engine,
            "transferNFT",
            &[
                nft_id.to_bytes(),
                holder.to_bytes(),
                recipient.to_bytes(),
                Vec::new(),
                vec![0xFF],
            ],
        )
        .expect_err("direct invoke with extra args should fail arity validation");
    assert!(
        err.to_string()
            .contains("TokenManagement.transferNFT: invalid arguments")
    );
}

#[test]
fn nft_get_nfts_of_owner() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"MultiNFT";
    let symbol = b"MFT";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x05);

    for _ in 0..3 {
        let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

        let _ = fixture.call("mintNFT", &mint_args);
    }

    let get_owner_nfts_args = vec![holder.to_bytes()];

    let owner_nfts_result = fixture.call("getNFTsOfOwner", &get_owner_nfts_args);
    assert!(
        !owner_nfts_result.is_empty(),
        "getNFTsOfOwner should return NFTs for owner"
    );
}

#[test]
fn nft_multiple_mints_same_block() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"UniqueNFT";
    let symbol = b"UNQ";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let account = sample_account(0x06);

    let mut nft_ids = Vec::new();
    for i in 0..5 {
        let mint_args = vec![asset_id.to_bytes(), account.to_bytes()];

        let nft_result = fixture.call("mintNFT", &mint_args);
        let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");
        nft_ids.push(nft_id);

        let get_info_args = vec![nft_id.to_bytes()];

        let info_result = fixture.call("getNFTInfo", &get_info_args);
        assert!(!info_result.is_empty(), "NFT {} should exist", i);
    }

    let unique_nfts: std::collections::HashSet<_> = nft_ids.into_iter().collect();
    assert_eq!(unique_nfts.len(), 5, "All NFT IDs should be unique");
}

#[test]
fn nft_owner_verification() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"OwnerTestNFT";
    let symbol = b"OWN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x10);
    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = fixture.call("getNFTInfo", &get_info_args);
    assert!(!info_result.is_empty());
}

#[test]
fn nft_transfer_verification() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"TransferVerifyNFT";
    let symbol = b"TVN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let from_account = sample_account(0x11);
    let to_account = sample_account(0x12);

    let mint_args = vec![asset_id.to_bytes(), from_account.to_bytes()];

    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    fixture.engine.set_calling_script_hash(Some(from_account));

    fixture.commit();

    let transfer_args = vec![
        nft_id.to_bytes(),
        from_account.to_bytes(),
        to_account.to_bytes(),
        Vec::new(),
    ];

    let transfer_result = fixture.call("transferNFT", &transfer_args);
    assert_eq!(transfer_result, vec![1]);

    fixture.engine.set_calling_script_hash(Some(to_account));

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = fixture.call("getNFTInfo", &get_info_args);
    assert!(!info_result.is_empty());
}

#[test]
fn nft_burn_verification() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"BurnVerifyNFT";
    let symbol = b"BVN";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x13);
    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    fixture.engine.set_calling_script_hash(Some(holder));

    fixture.commit();

    let burn_args = vec![nft_id.to_bytes()];

    let burn_result = fixture.call("burnNFT", &burn_args);
    assert_eq!(burn_result, vec![1]);

    let get_info_args = vec![nft_id.to_bytes()];

    let info_result = fixture.call("getNFTInfo", &get_info_args);
    assert!(info_result.is_empty(), "NFT should not exist after burn");
}

#[test]
fn nft_balance_of() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"BalanceNFT";
    let symbol = b"BAL";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x20);

    for _ in 0..3 {
        let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

        let _ = fixture.call("mintNFT", &mint_args);
    }

    let balance_args = vec![asset_id.to_bytes(), holder.to_bytes()];

    let balance_result = fixture.call("balanceOf", &balance_args);

    let balance = BigInt::from_signed_bytes_le(&balance_result);
    assert_eq!(balance, BigInt::from(3), "Holder should have 3 NFTs");
}

#[test]
fn nft_get_nfts_returns_all_for_asset() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"GetNFTsTestNFT";
    let symbol = b"GNT";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x30);

    let mut minted_ids = Vec::new();
    for i in 0..5 {
        let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];

        let nft_result = fixture.call("mintNFT", &mint_args);
        let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");
        minted_ids.push(nft_id);
        assert!(!nft_id.is_zero(), "NFT {} should have valid ID", i);
    }

    let get_nfts_args = vec![asset_id.to_bytes()];

    let nfts_result = fixture.call("getNFTs", &get_nfts_args);
    assert!(!nfts_result.is_empty(), "getNFTs should return iterator");
    let nft_keys = collect_iterator_keys(&mut fixture.engine, &nfts_result);
    let mut expected_keys = minted_ids.iter().map(UInt160::to_bytes).collect::<Vec<_>>();
    expected_keys.sort();
    assert_eq!(nft_keys, expected_keys);
}

#[test]
fn nft_get_nfts_of_owner_after_transfer() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"TransferOwnerNFT";
    let symbol = b"TOW";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let holder1 = sample_account(0x40);
    let holder2 = sample_account(0x41);

    let mint_args = vec![asset_id.to_bytes(), holder1.to_bytes()];
    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    fixture.engine.set_calling_script_hash(Some(holder1));
    fixture.commit();

    let transfer_args = vec![
        nft_id.to_bytes(),
        holder1.to_bytes(),
        holder2.to_bytes(),
        Vec::new(),
    ];

    let transfer_result = fixture.call("transferNFT", &transfer_args);
    assert_eq!(transfer_result, vec![1]);

    let get_owner_nfts_args = vec![holder2.to_bytes()];

    let owner_nfts_result = fixture.call("getNFTsOfOwner", &get_owner_nfts_args);
    assert!(
        !owner_nfts_result.is_empty(),
        "getNFTsOfOwner should return NFT for new owner"
    );
    let owner_nft_keys = collect_iterator_keys(&mut fixture.engine, &owner_nfts_result);
    assert_eq!(owner_nft_keys, vec![nft_id.to_bytes()]);
}

fn nft_index_updates_after_burn() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;

    let name = b"BurnIndexNFT";
    let symbol = b"BNI";
    let create_args = vec![owner.to_bytes(), name.to_vec(), symbol.to_vec(), vec![1]];

    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let holder = sample_account(0x50);

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    fixture.engine.set_calling_script_hash(Some(holder));
    fixture.commit();

    let burn_args = vec![nft_id.to_bytes()];

    let burn_result = fixture.call("burnNFT", &burn_args);
    assert_eq!(burn_result, vec![1]);

    let get_info_args = vec![nft_id.to_bytes()];
    let info_result = fixture.call("getNFTInfo", &get_info_args);
    assert!(info_result.is_empty(), "Burned NFT should not exist");

    let get_nfts_args = vec![asset_id.to_bytes()];
    let asset_nfts_result = fixture.call("getNFTs", &get_nfts_args);
    let asset_nft_keys = collect_iterator_keys(&mut fixture.engine, &asset_nfts_result);
    assert!(
        asset_nft_keys.is_empty(),
        "getNFTs should be empty after burn"
    );

    let get_owner_nfts_args = vec![holder.to_bytes()];
    let owner_nfts_result = fixture.call("getNFTsOfOwner", &get_owner_nfts_args);
    let owner_nft_keys = collect_iterator_keys(&mut fixture.engine, &owner_nfts_result);
    assert!(
        owner_nft_keys.is_empty(),
        "getNFTsOfOwner should be empty after burn"
    );
}

#[test]
fn get_nfts_excludes_burned_nft_in_same_overlay() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;
    let holder = sample_account(0x50);

    let create_args = vec![
        owner.to_bytes(),
        b"OverlayBurnNFT".to_vec(),
        b"OBN".to_vec(),
        vec![1],
    ];
    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    fixture
        .engine
        .load_script(vec![neo_vm_rs::OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load overlay script");
    fixture.engine.set_calling_script_hash(Some(holder));

    let burn_args = vec![nft_id.to_bytes()];
    let burn_result = fixture.call("burnNFT", &burn_args);
    assert_eq!(burn_result, vec![1]);

    let get_nfts_args = vec![asset_id.to_bytes()];
    let asset_nfts_result = fixture.call("getNFTs", &get_nfts_args);
    let asset_nft_keys = collect_iterator_keys(&mut fixture.engine, &asset_nfts_result);
    assert!(
        asset_nft_keys.is_empty(),
        "getNFTs should be empty after overlay burn"
    );
}

#[test]
fn get_nfts_of_owner_excludes_burned_nft_in_same_overlay() {
    let mut fixture = NftFixture::new();
    let owner = fixture.owner;
    let holder = sample_account(0x51);

    let create_args = vec![
        owner.to_bytes(),
        b"OverlayOwnerNFT".to_vec(),
        b"OWN".to_vec(),
        vec![1],
    ];
    let result = fixture.call("createNonFungible", &create_args);
    let asset_id = UInt160::from_bytes(&result).expect("asset id");

    fixture.engine.set_current_script_hash(Some(owner));
    fixture.engine.set_calling_script_hash(Some(owner));

    let mint_args = vec![asset_id.to_bytes(), holder.to_bytes()];
    let nft_result = fixture.call("mintNFT", &mint_args);
    let nft_id = UInt160::from_bytes(&nft_result).expect("nft id");

    fixture
        .engine
        .load_script(vec![neo_vm_rs::OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load overlay script");
    fixture.engine.set_calling_script_hash(Some(holder));

    let burn_args = vec![nft_id.to_bytes()];
    let burn_result = fixture.call("burnNFT", &burn_args);
    assert_eq!(burn_result, vec![1]);

    let get_owner_nfts_args = vec![holder.to_bytes()];
    let owner_nfts_result = fixture.call("getNFTsOfOwner", &get_owner_nfts_args);
    let owner_nft_keys = collect_iterator_keys(&mut fixture.engine, &owner_nfts_result);
    assert!(
        owner_nft_keys.is_empty(),
        "getNFTsOfOwner should be empty after overlay burn"
    );
}

#[test]
fn nft_index_updates_after_burn_regression() {
    nft_index_updates_after_burn();
}
