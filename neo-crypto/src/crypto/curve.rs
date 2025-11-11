/// Supported elliptic curves for signing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Curve {
    Secp256r1,
    Secp256k1,
}
