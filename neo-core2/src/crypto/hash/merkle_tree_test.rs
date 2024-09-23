use std::vec::Vec;
use std::string::String;
use crate::crypto::hash::{NewMerkleTree, CalcMerkleRoot, MerkleTreeNode};
use crate::util::Uint256;
use assert2::{assert, let_assert};
use anyhow::Result;

fn test_compute_merkle_tree(hex_hashes: Vec<&str>, result: &str) -> Result<()> {
    let mut hashes = Vec::with_capacity(hex_hashes.len());
    for str in hex_hashes {
        let hash = Uint256::decode_string_le(str)?;
        hashes.push(hash);
    }

    let merkle = NewMerkleTree(&hashes)?;
    let optimized = CalcMerkleRoot(&hashes);
    assert!(result == optimized.to_string_le());
    assert!(result == merkle.root().to_string_le());
    assert!(merkle.root().is_root());
    assert!(!merkle.root().is_leaf());
    let mut leaf = merkle.root();
    while leaf.left_child().is_some() || leaf.right_child().is_some() {
        if let Some(left) = leaf.left_child() {
            leaf = left;
            continue;
        }
        leaf = leaf.right_child().unwrap();
    }
    assert!(leaf.is_leaf());
    assert!(!leaf.is_root());
    Ok(())
}

#[test]
fn test_compute_merkle_tree1() -> Result<()> {
    // Mainnet block #0
    let raw_hashes = vec![
        "fb5bd72b2d6792d75dc2f1084ffa9e9f70ca85543c717a6b13d9959b452a57d6",
        "c56f33fc6ecfcd0c225c4ab356fee59390af8560be0e930faebe74a6daff7c9b",
        "602c79718b16e442de58778e148d0b1084e3b2dffd5de6b7b16cee7969282de7",
        "3631f66024ca6f5b033d7e0809eb993443374830025af904fb51b0334f127cda",
    ];
    let res = "803ff4abe3ea6533bcc0be574efa02f83ae8fdc651c879056b0d9be336c01bf4";
    test_compute_merkle_tree(raw_hashes, res)
}

#[test]
fn test_compute_merkle_tree2() -> Result<()> {
    // Mainnet block #4635525
    let raw_hashes = vec![
        "c832ef573136eae2c57a35989d3fb9b3a135d08ffa0faa49d8766f7c1a92ca0f",
        "b8606dfeb126a5963d6674f8dbfb786db7f6c27800167c3eef56ff7110ff0ffc",
        "498a5d58179002dd9db7b23df657ecf7e1b2e8218bd48dda035e5accc664830a",
        "5c350282b448c139adb1f5e3fba0e9326476a38c01ea88876ebc4a882c472d42",
        "cea31cc85e7310183561d4f420026984ba48354516f9274c44b52c7f9a5c6107",
        "744f985dd5ad6f4ad6376376b48552abf7755b2ebc5c6271950714f848d1cc3a",
        "02c5fc225b6ead91f73a7b3ebb19bb30a113baea60f439b654c2811d630a2c48",
        "2b3478e0fa91db3a309caeb4d9739f38233c1c189d6fa7e159e24afce9fae082",
        "4d50693cee3ac2c976c092620834d4da264583cf15a1d11dd65d0e94861d49e0",
        "5f179efae999f8f8086269cedd1fbfaf6e90aadf5369a12737db0fff5905b12e",
        "6ef2237b6c8683f626269027050c45cc4be89042ee99e4e89bfd9d9fbd24da19",
        "6fd5154af55b4a1e4a1a5272e33238b2a2da12a30fa06af4f740d207e54ed495",
    ];
    let res = "42489ad8043a834149cd8e406c90c61411a05a0ca9f8e921b456a00b5d5988d7";
    test_compute_merkle_tree(raw_hashes, res)
}

#[test]
fn test_compute_merkle_tree3() -> Result<()> {
    // Mainnet block #2097152
    let raw_hashes = vec![
        "a7ff2c6aa08d4b979d5f7605aec5a1783c9207c738b5852ed5ff17b37ada649d",
        "34fd42c1f47aa306ad2fd0fc04437fd5c828a25b3de59e73b18157253094a8da",
        "36458dffd678d9f75ed9f2a28beb58bf1ad739f8899469b8641b0ddea22fcf5d",
        "3e8144abe2edda263593f24defd4557d403efa1b4fa839409ac217d6f8a87d3a",
        "a1d2cf73841fefcd21ca9986c664518d2af61edcfe3a97b30b3cc58fab4e61f6",
        "c1e868aef0e8fd76b95a18e155b1fa65f30d0a4887bc953411553728664725bc",
        "52d2fda0fe0fd586063d801c5ba77ca123a790d7e4dae34c53398feab36da721",
        "fdf8d4610cb2de35ab4c18d38357b86c52966d149c8975906170dc513cc26345",
        "35a26a11ef65d8f7a2424f7ce5915aa1d8bf3449018516003799767c2696197e",
        "c9d251abfc20a0d6eeac2d5a93b77a6a0632a679a07decea2c809aead89bb503",
        "d92c72873f2929c621ec06433da3053db25ee396b70c83d53abd40801823f66c",
    ];
    let res = "09c2dbc88810c350a2e7ace56bb1b371b2a2b5c4744e7a303adace9a2c2bbf6d";
    test_compute_merkle_tree(raw_hashes, res)
}

#[test]
fn test_new_merkle_tree_fail_without_hashes() {
    let hashes: Vec<Uint256> = Vec::new();
    let result = NewMerkleTree(&hashes);
    assert!(result.is_err());
    let hashes: Vec<Uint256> = Vec::with_capacity(0);
    let result = NewMerkleTree(&hashes);
    assert!(result.is_err());
}

#[test]
fn test_build_merkle_tree_without_nodes() {
    let leaves: Vec<MerkleTreeNode> = Vec::new();
    assert!(std::panic::catch_unwind(|| build_merkle_tree(&leaves)).is_err());
    let leaves: Vec<MerkleTreeNode> = Vec::with_capacity(0);
    assert!(std::panic::catch_unwind(|| build_merkle_tree(&leaves)).is_err());
}
