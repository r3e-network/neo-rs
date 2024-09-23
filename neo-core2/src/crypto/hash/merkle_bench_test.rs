extern crate test;
extern crate rand;
extern crate assert;

use test::Bencher;
use rand::Rng;
use crate::crypto::hash::{MerkleTree, calc_merkle_root};
use crate::util::Uint256;
use assert::assert_ok;

#[bench]
fn benchmark_merkle(b: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let mut hashes: Vec<Uint256> = (0..100000).map(|_| rng.gen()).collect();

    b.iter(|| {
        b.iter(|| {
            let tr = MerkleTree::new(hashes.clone()).unwrap();
            assert_ok!(tr.root());
        });
    });

    b.iter(|| {
        b.iter(|| {
            assert_ok!(calc_merkle_root(&hashes));
        });
    });
}
