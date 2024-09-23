use std::collections::HashSet;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;
use std::time::Duration;
use std::vec::Vec;

use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json;
use sha2::{Digest, Sha256};
use yaml_rust::{YamlLoader, YamlEmitter};

use crate::crypto::keys::{PublicKey, PrivateKey, PublicKeys};
use crate::internal::testserdes;
use crate::require;

#[test]
fn test_encode_decode_infinity() {
    let key = PublicKey::default();
    let b = testserdes::encode_binary(&key).unwrap();
    require::no_error(&b);
    require::equal(1, b.len());

    let mut key_decode = PublicKey::default();
    require::no_error(&key_decode.decode_bytes(&b));
    require::equal(vec![0x00], key_decode.bytes());
}

#[test]
fn test_encode_decode_public_key() {
    for _ in 0..4 {
        let k = PrivateKey::new().unwrap();
        require::no_error(&k);
        let p = k.public_key();
        testserdes::encode_decode_binary(&p, &PublicKey::default());
    }

    let err_cases = vec![vec![], vec![0x02], vec![0x04]];

    for tc in err_cases {
        require::error(&testserdes::decode_binary(&tc, &PublicKey::default()));
    }
}

#[test]
fn test_public_keys_copy() {
    require::nil(PublicKeys::default().copy());

    let mut pubz = vec![PublicKey::default(); 5];
    for i in 0..pubz.len() {
        let priv_key = PrivateKey::new().unwrap();
        require::no_error(&priv_key);
        pubz[i] = priv_key.public_key();
    }
    let pubs = PublicKeys(pubz.clone());

    let cp = pubs.copy();
    let pubx: Vec<PublicKey> = cp.into();
    require::equal(pubz, pubx);

    let priv_key = PrivateKey::new().unwrap();
    require::no_error(&priv_key);
    cp[0] = priv_key.public_key();

    require::not_equal(pubs[0], cp[0]);
    require::equal(&pubs[1..], &cp[1..]);
}

#[test]
fn test_new_public_key_from_bytes() {
    let priv_key = PrivateKey::new().unwrap();
    require::no_error(&priv_key);

    let b = priv_key.public_key().bytes();
    let pub_key = PublicKey::from_bytes(&b, &elliptic::P256).unwrap();
    require::no_error(&pub_key);
    require::equal(priv_key.public_key(), pub_key);
    // Test cached access
    let pub_key2 = PublicKey::from_bytes(&b, &elliptic::P256).unwrap();
    require::no_error(&pub_key2);
    require::same(pub_key, pub_key2);

    require::error(&PublicKey::from_bytes(&[0x00, 0x01], &elliptic::P256));
}

#[test]
fn test_decode_from_string() {
    let str = "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c";
    let pub_key = PublicKey::from_string(str).unwrap();
    require::no_error(&pub_key);
    require::equal(str, pub_key.string_compressed());

    require::error(&PublicKey::from_string(&str[2..]));

    let str = "zzb209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c";
    require::error(&PublicKey::from_string(str));
}

#[test]
fn test_decode_from_string_bad_compressed() {
    let str = "02ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    require::error(&PublicKey::from_string(str));
}

#[test]
fn test_decode_from_string_bad_x_more_than_p() {
    let str = "02ffffffff00000001000000000000000000000001ffffffffffffffffffffffff";
    require::error(&PublicKey::from_string(str));
}

#[test]
fn test_decode_from_string_not_on_curve() {
    let str = "04ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff";
    require::error(&PublicKey::from_string(str));
}

#[test]
fn test_decode_from_string_uncompressed() {
    let str = "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";
    require::no_error(&PublicKey::from_string(str));
}

#[test]
fn test_pubkey_to_address() {
    let pub_key = PublicKey::from_string("031ee4e73a17d8f76dc02532e2620bcb12425b33c0c9f9694cc2caa8226b68cad4").unwrap();
    require::no_error(&pub_key);
    let actual = pub_key.address();
    let expected = "NdxG5MZQy8h2qseawfSt8tTYG2iQPTwsn9";
    require::equal(expected, actual);
}

#[test]
fn test_decode_bytes() {
    let pub_key = get_pub_key();
    let test_bytes_function = |bytes_function: fn() -> Vec<u8>| {
        let mut decoded_pub_key = PublicKey::default();
        require::no_error(&decoded_pub_key.decode_bytes(&bytes_function()));
        require::equal(pub_key, decoded_pub_key);
    };
    test_bytes_function(pub_key.bytes);
    test_bytes_function(pub_key.uncompressed_bytes);
}

#[test]
fn test_sort() {
    let mut pubs1 = PublicKeys::new(10);
    for i in 0..pubs1.len() {
        let priv_key = PrivateKey::new().unwrap();
        require::no_error(&priv_key);
        pubs1[i] = priv_key.public_key();
    }

    let mut pubs2 = pubs1.clone();

    pubs1.sort();

    rand::thread_rng().shuffle(&mut pubs2);
    pubs2.sort();

    // Check that sort on the same set of values produce the same result.
    require::equal(pubs1, pubs2);
}

#[test]
fn test_contains() {
    let pub_key = get_pub_key();
    let pub_keys = PublicKeys::from(vec![get_pub_key()]);
    require::true(pub_keys.contains(&pub_key));
}

#[test]
fn test_unique() {
    let pub_keys = PublicKeys::from(vec![get_pub_key(), get_pub_key()]);
    let unique = pub_keys.unique();
    require::equal(1, unique.len());
}

fn get_pub_key() -> PublicKey {
    let pub_key = PublicKey::from_string("031ee4e73a17d8f76dc02532e2620bcb12425b33c0c9f9694cc2caa8226b68cad4").unwrap();
    require::no_error(&pub_key);
    pub_key
}

#[test]
fn test_marshall_json() {
    let str = "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c";
    let pub_key = PublicKey::from_string(str).unwrap();
    require::no_error(&pub_key);

    let bytes = serde_json::to_vec(&pub_key).unwrap();
    require::no_error(&bytes);
    require::equal(format!("\"{}\"", str).as_bytes(), &bytes);
}

#[test]
fn test_unmarshall_json() {
    let str = "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c";
    let expected = PublicKey::from_string(str).unwrap();
    require::no_error(&expected);

    let actual: PublicKey = serde_json::from_str(&format!("\"{}\"", str)).unwrap();
    require::no_error(&actual);
    require::equal(expected, actual);
}

#[test]
fn test_unmarshall_json_bad_compressed() {
    let str = "\"02ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"";
    let actual: Result<PublicKey, _> = serde_json::from_str(str);
    require::error(&actual);
}

#[test]
fn test_unmarshall_json_not_a_hex() {
    let str = "\"04Tb17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5\"";
    let actual: Result<PublicKey, _> = serde_json::from_str(str);
    require::error(&actual);
}

#[test]
fn test_unmarshall_json_bad_format() {
    let str = "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";
    let actual: Result<PublicKey, _> = serde_json::from_str(str);
    require::error(&actual);
}

#[bench]
fn benchmark_public_equal(b: &mut test::Bencher) {
    let k11 = get_pub_key();
    let k12 = get_pub_key();
    let k2 = PublicKey::from_string("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap();
    require::no_error(&k2);
    b.iter(|| {
        k11.equal(&k12);
        k11.equal(&k2);
    });
}

#[bench]
fn benchmark_public_bytes(b: &mut test::Bencher) {
    let k = get_pub_key();
    b.iter(|| {
        k.bytes();
    });
}

#[bench]
fn benchmark_public_uncompressed_bytes(b: &mut test::Bencher) {
    let k = get_pub_key();
    b.iter(|| {
        k.uncompressed_bytes();
    });
}

#[bench]
fn benchmark_public_decode_bytes(b: &mut test::Bencher) {
    let key_bytes = hex::decode("03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c").unwrap();
    let mut k = PublicKey::default();
    b.iter(|| {
        require::no_error(&k.decode_bytes(&key_bytes));
    });
}

#[test]
fn test_marshall_yaml() {
    let str = "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c";
    let pub_key = PublicKey::from_string(str).unwrap();
    require::no_error(&pub_key);

    let bytes = yaml_rust::YamlEmitter::new().dump(&pub_key).unwrap();
    require::no_error(&bytes);

    let expected = format!("{}\n", str).as_bytes(); // YAML marshaller adds new line in the end which is expected.
    require::equal(expected, &bytes);
}

#[test]
fn test_unmarshall_yaml() {
    let str = "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c";
    let expected = PublicKey::from_string(str).unwrap();
    require::no_error(&expected);

    let actual: PublicKey = yaml_rust::YamlLoader::new().load_from_str(str).unwrap();
    require::no_error(&actual);
    require::equal(expected, actual);
}

#[test]
fn test_unmarshall_yaml_bad_compressed() {
    let str = "\"02ffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff\"";
    let actual: Result<PublicKey, _> = yaml_rust::YamlLoader::new().load_from_str(str);
    require::error(&actual);
    require::contains(&actual.unwrap_err().to_string(), "error computing Y for compressed point");
}

#[test]
fn test_unmarshall_yaml_not_a_hex() {
    let str = "\"04Tb17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5\"";
    let actual: Result<PublicKey, _> = yaml_rust::YamlLoader::new().load_from_str(str);
    require::error(&actual);
    require::contains(&actual.unwrap_err().to_string(), "failed to decode public key from hex bytes");
}

#[test]
fn test_unmarshall_yaml_uncompressed() {
    let str = "046b17d1f2e12c4247f8bce6e563a440f277037d812deb33a0f4a13945d898c2964fe342e2fe1a7f9b8ee7eb4a7c0f9e162bce33576b315ececbb6406837bf51f5";
    let expected = PublicKey::from_string(str).unwrap();
    require::no_error(&expected);

    let actual: PublicKey = yaml_rust::YamlLoader::new().load_from_str(str).unwrap();
    require::no_error(&actual);
    require::equal(expected, actual);
}

#[test]
fn test_marshal_unmarshal_yaml() {
    let str = "03b209fd4f53a7170ea4444e0cb0a6bb6a53c2bd016926989cf85f9b0fba17a70c";
    let expected = PublicKey::from_string(str).unwrap();
    require::no_error(&expected);

    testserdes::marshal_unmarshal_yaml(&expected, &PublicKey::default());
}
