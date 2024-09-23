use serde_json;
use rand::Rng;
use crate::random;
use crate::testserdes;
use crate::mpt;
use crate::io;
use crate::require;

struct ProofWithKey {
    key: Vec<u8>,
    proof: Vec<Vec<u8>>,
}

impl ProofWithKey {
    fn new() -> Self {
        let mut rng = rand::thread_rng();
        ProofWithKey {
            key: random::bytes(10),
            proof: vec![
                random::bytes(12),
                random::bytes(0),
                random::bytes(34),
            ],
        }
    }
}

#[test]
fn test_get_proof_marshal_json() {
    fn good() {
        let p = ProofWithKey::new();
        testserdes::marshal_unmarshal_json(&p, &ProofWithKey::new());
    }

    fn compatibility() {
        let js = b"Bfn///8SBiQBAQ8D6yfHa4wV24kQ9eXarzY5Bw55VFzysUbkJjrz5FipqkjSAAQEBAQEBAMcbFvhto6QJgYoJs/uzqTrZNrPxpkgNiF5Z/ME98copwPQ4q6ZqLA8S7XUXNCrJNF68vMu8Gx3W8Ooo3qwMomm0gQDiT6zHh/siCZ0c2bfBEymPmRNTiXSAKFIammjmnnBnJYD+CNwgcEzBJqYfnc7RMhr8cPhffKN0281w0M7XLQ9BO4D7W+t3cleDNdiNc6tqWR8jyIP+bolh5QnZIyKXPwGHjsEBAQDcpxkuWYJr6g3ilENTh1sztlZsXZvt6Eedmyy6kI2gQoEKQEGDw8PDw8PA33qzf1Q5ILAwmYxBnM2N80A8JtFHKR7UHhVEqo5nQ0eUgADbChDXdc7hSDZpD9xbhYGuJxVxRWqhsVRTR2dE+18gd4DG5gRFexXofB0aNb6G2kzQUSTD+aWVsfmnKGf4HHivzAEBAQEBAQEBAQEBAQEBARSAAQEA2IMPmRKP0b2BqhMB6IgtfpPeuXKJMdMze7Cr1TeJqbmA1vvqQgR5DN9ew+Zp/nc5SBQbjV5gEq7F/tIipWaQJ1hBAQEBAQEBAQEBAQEBAMCAR4=";

        let p: ProofWithKey = serde_json::from_slice(js).unwrap();
        require::no_error(p.proof.len() == 6);
        for proof in p.proof.iter() {
            let mut r = io::BinReader::from_buf(proof);
            let mut n = mpt::NodeObject::default();
            n.decode_binary(&mut r);
            require::no_error(r.err().is_none());
            require::not_nil(n.node());
        }
    }

    good();
    compatibility();
}

#[test]
fn test_proof_with_key_encode_string() {
    let expected = ProofWithKey::new();
    let mut actual = ProofWithKey::default();
    require::no_error(actual.from_string(&expected.to_string()));
    require::equal(&expected, &actual);
}

#[test]
fn test_verify_proof_marshal_json() {
    fn good() {
        let vp = VerifyProof { data: random::bytes(100) };
        testserdes::marshal_unmarshal_json(&vp, &VerifyProof::default());
    }

    fn no_value() {
        let vp = VerifyProof::default();
        testserdes::marshal_unmarshal_json(&vp, &VerifyProof { data: vec![1, 2, 3] });
    }

    good();
    no_value();
}
