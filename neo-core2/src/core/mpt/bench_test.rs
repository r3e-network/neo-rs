use test::Bencher;
use crate::core::mpt::{Node, NewExtensionNode, NewLeafNode, NewHashNode, NewBranchNode};
use crate::internal::random;

fn benchmark_bytes(b: &mut Bencher, n: &mut dyn Node) {
    b.iter(|| {
        n.invalidate_cache();
        let _ = n.bytes();
    });
}

#[bench]
fn benchmark_bytes_extension(b: &mut Bencher) {
    let mut n = NewExtensionNode(random::bytes(10), NewLeafNode(random::bytes(10)));
    benchmark_bytes(b, &mut n);
}

#[bench]
fn benchmark_bytes_leaf(b: &mut Bencher) {
    let mut n = NewLeafNode(vec![0; 15]);
    benchmark_bytes(b, &mut n);
}

#[bench]
fn benchmark_bytes_hash(b: &mut Bencher) {
    let mut n = NewHashNode(random::uint256());
    benchmark_bytes(b, &mut n);
}

#[bench]
fn benchmark_bytes_branch(b: &mut Bencher) {
    let mut n = NewBranchNode();
    n.children[0] = Some(Box::new(NewLeafNode(random::bytes(10))));
    n.children[4] = Some(Box::new(NewLeafNode(random::bytes(10))));
    n.children[7] = Some(Box::new(NewLeafNode(random::bytes(10))));
    n.children[8] = Some(Box::new(NewLeafNode(random::bytes(10))));
    benchmark_bytes(b, &mut n);
}
