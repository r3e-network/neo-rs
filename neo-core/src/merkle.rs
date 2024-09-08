// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use alloc::{vec, vec::Vec};
use primitive_types::H256;
use neo_base::hash::{Sha256, SlicesSha256};

#[allow(dead_code)]
pub struct MerkleTree {
    root: H256,
    nodes: Vec<H256>,
    leaves_offset: usize,
}

impl MerkleTree {
    pub fn new(hashes: &[H256]) -> Self {
        let nodes = build_merkle_nodes(&hashes);

        let root = nodes[0].clone();
        let leaves_offset = nodes.len() - hashes.len();

        Self { nodes, root, leaves_offset }
    }

    pub fn root(&self) -> &H256 { &self.root }
}

fn build_merkle_nodes(hashes: &[H256]) -> Vec<H256> {
    if hashes.len() == 0 {
        return vec![H256::default()];
    }

    if hashes.len() == 1 {
        return vec![hashes[0].clone()];
    }

    let inners = inner_merkle_nodes(hashes.len());
    let mut nodes = vec![H256::default(); 1 + inners + hashes.len()];

    nodes[1 + inners..].copy_from_slice(hashes);

    let mut left = 1 + inners;
    let mut right = nodes.len();

    let mut next_left = left - (right - left + 1) / 2;
    let mut next_right = left;

    while next_left >= 1 {
        for k in 0..(next_right - next_left) {
            let off = 2 * k + left;
            nodes[next_left + k] = children_sha256(off, &nodes[..right]);
        }

        left = next_left;
        right = next_right;

        next_left = left - (right - left + 1) / 2;
        next_right = left;
    }

    nodes[0] = nodes[1].clone();
    nodes
}


fn inner_merkle_nodes(nodes: usize) -> usize {
    if nodes == 0 || nodes == 1 {
        return 1;
    }

    let mut nodes = nodes;
    let mut sum = 0;
    while nodes > 1 {
        nodes = (nodes + 1) / 2;
        sum += nodes;
    }

    sum
}

#[inline]
fn children_sha256(off: usize, hashes: &[H256]) -> H256 {
    let two = if off + 1 >= hashes.len() {
        [&hashes[off], &hashes[off]]
    } else {
        [&hashes[off], &hashes[off + 1]]
    };

    two.iter().slices_sha256().sha256().into()
}


/// Calculating the sha256 merkle-root
pub trait MerkleSha256 {
    fn merkle_sha256(&self) -> H256;
}

impl<T: AsRef<[H256]>> MerkleSha256 for T {
    fn merkle_sha256(&self) -> H256 {
        let hashes = self.as_ref();
        if hashes.len() == 0 {
            return H256::default();
        }

        if hashes.len() == 1 {
            return hashes[0].clone();
        }

        let mut nodes = vec![H256::default(); (hashes.len() + 1) / 2];
        for k in 0..nodes.len() {
            nodes[k] = children_sha256(2 * k, hashes);
        }

        let mut prev = nodes.len();
        let mut right = (nodes.len() + 1) / 2;
        while prev > right {
            for k in 0..right {
                nodes[k] = children_sha256(2 * k, &nodes[..prev]);
            }

            prev = right;
            right = (right + 1) / 2;
        }

        nodes[0]
    }
}


#[cfg(test)]
mod test {
    use primitive_types::H256;
    use super::*;
    use neo_base::{hash::Sha256, encoding::hex::{FromRevHex, ToHex}, bytes::ToArray};
    use crate::merkle::inner_merkle_nodes;
    use crate::types::H256;


    #[test]
    fn test_inner_merkle_nodes() {
        let n = inner_merkle_nodes(2);
        assert_eq!(n, 1);

        let n = inner_merkle_nodes(3);
        assert_eq!(n, 3);

        let n = inner_merkle_nodes(0);
        assert_eq!(n, 1);

        let n = inner_merkle_nodes(1);
        assert_eq!(n, 1);

        let n = inner_merkle_nodes(4);
        assert_eq!(n, 3);

        let n = inner_merkle_nodes(5);
        assert_eq!(n, 6);
    }

    trait MerkleHash {
        fn merkle_hash(&self) -> H256;
    }

    impl MerkleHash for [H256; 2] {
        fn merkle_hash(&self) -> H256 {
            self.iter().slices_sha256().sha256().into()
        }
    }

    #[test]
    fn test_merkle_tree() {
        let tree = MerkleTree::new(&[]);
        assert_eq!(tree.leaves_offset, 1);
        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.root, H256::default());
        assert_eq!(tree.root(), &H256::default());
        assert_eq!([].merkle_sha256(), H256::default());

        let one = "Hello world!".sha256().into();
        let tree = MerkleTree::new(core::array::from_ref(&one).as_slice());
        assert_eq!(tree.nodes.len(), 1);
        assert_eq!(tree.leaves_offset, 0);
        assert_eq!(tree.root(), &one);
        assert_eq!([one].merkle_sha256(), one);

        let two = "Hello Rust!".sha256().into();
        let tree = MerkleTree::new(&[one, two]);
        assert_eq!(tree.nodes.len(), 4);
        assert_eq!(tree.leaves_offset, 2);

        assert_eq!(tree.root(), &[one, two].merkle_hash());
        assert_eq!([one, two].merkle_sha256(), [one, two].merkle_hash());

        let three = "ABC".sha256().into();
        let tree = MerkleTree::new(&[one, two, three]);
        assert_eq!(tree.nodes.len(), 7);
        assert_eq!(tree.leaves_offset, 4);
        assert_eq!(tree.root, [[one, two].merkle_hash(), [three, three].merkle_hash()].merkle_hash());
        assert_eq!([one, two, three].merkle_sha256(), tree.root);

        let four = "Ok".sha256().into();
        let tree = MerkleTree::new(&[one, two, three, four]);
        assert_eq!(tree.nodes.len(), 8);
        assert_eq!(tree.leaves_offset, 4);
        assert_eq!(tree.root, [[one, two].merkle_hash(), [three, four].merkle_hash()].merkle_hash());
    }

    #[test]
    fn test_merkle_root1() {
        let hashes = [
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
        ]
            .iter()
            .map(|v| Vec::from_rev_hex(v).expect("decode should be ok"))
            .map(|v| H256::from(v.to_array()))
            .collect::<Vec<_>>();

        let should = "6dbf2b2c9aceda3a307a4e74c4b5a2b271b3b16be5ace7a250c31088c8dbc209";
        let tree = MerkleTree::new(&hashes);
        assert_eq!(&tree.root().to_hex(), should);

        let root = hashes.merkle_sha256();
        assert_eq!(&root.to_hex(), should);
    }

    #[test]
    fn test_merkle_root2() {
        let hashes = [
            "fb5bd72b2d6792d75dc2f1084ffa9e9f70ca85543c717a6b13d9959b452a57d6",
            "c56f33fc6ecfcd0c225c4ab356fee59390af8560be0e930faebe74a6daff7c9b",
            "602c79718b16e442de58778e148d0b1084e3b2dffd5de6b7b16cee7969282de7",
            "3631f66024ca6f5b033d7e0809eb993443374830025af904fb51b0334f127cda",
        ]
            .iter()
            .map(|v| Vec::from_rev_hex(v).expect("decode should be ok"))
            .map(|v| H256::from(v.to_array()))
            .collect::<Vec<_>>();

        let should = "f41bc036e39b0d6b0579c851c6fde83af802fa4e57bec0bc3365eae3abf43f80";
        let tree = MerkleTree::new(&hashes);
        assert_eq!(&tree.root().to_hex(), should);

        let root = hashes.merkle_sha256();
        assert_eq!(&root.to_hex(), should);
    }

    #[test]
    fn test_merkle_root3() {
        let hashes = [
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
        ]
            .iter()
            .map(|v| Vec::from_rev_hex(v).expect("decode should be ok"))
            .map(|v| H256::from(v.to_array()))
            .collect::<Vec<_>>();

        let should = "d788595d0ba056b421e9f8a90c5aa01114c6906c408ecd4941833a04d89a4842";
        let tree = MerkleTree::new(&hashes);
        assert_eq!(&tree.root().to_hex(), should);

        let root = hashes.merkle_sha256();
        assert_eq!(&root.to_hex(), should);
    }
}