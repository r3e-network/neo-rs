use super::*;

// Canonical BLS12-381 generators + a Gt element, from UT_CryptoLib
// (s_g1Hex / s_g2Hex / s_gtHex). GT_ADD_HEX = TestBls12381Add expected output.
const G1_GEN: &str = "97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac586c55e83ff97a1aeffb3af00adb22c6bb";
const G2_GEN: &str = "93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8";
const GT_HEX: &str = "0f41e58663bf08cf068672cbd01a7ec73baca4d72ca93544deff686bfd6df543d48eaa24afe47e1efde449383b67663104c581234d086a9902249b64728ffd21a189e87935a954051c7cdba7b3872629a4fafc05066245cb9108f0242d0fe3ef03350f55a7aefcd3c31b4fcb6ce5771cc6a0e9786ab5973320c806ad360829107ba810c5a09ffdd9be2291a0c25a99a211b8b424cd48bf38fcef68083b0b0ec5c81a93b330ee1a677d0d15ff7b984e8978ef48881e32fac91b93b47333e2ba5706fba23eb7c5af0d9f80940ca771b6ffd5857baaf222eb95a7d2809d61bfe02e1bfd1b68ff02f0b8102ae1c2d5d5ab1a19f26337d205fb469cd6bd15c3d5a04dc88784fbb3d0b2dbdea54d43b2b73f2cbb12d58386a8703e0f948226e47ee89d018107154f25a764bd3c79937a45b84546da634b8f6be14a8061e55cceba478b23f7dacaa35c8ca78beae9624045b4b601b2f522473d171391125ba84dc4007cfbf2f8da752f7c74185203fcca589ac719c34dffbbaad8431dad1c1fb597aaa5193502b86edb8857c273fa075a50512937e0794e1e65a7617c90d8bd66065b1fffe51d7a579973b1315021ec3c19934f1368bb445c7c2d209703f239689ce34c0378a68e72a6b3b216da0e22a5031b54ddff57309396b38c881c4c849ec23e87089a1c5b46e5110b86750ec6a532348868a84045483c92b7af5af689452eafabf1a8943e50439f1d59882a98eaa0170f1250ebd871fc0a92a7b2d83168d0d727272d441befa15c503dd8e90ce98db3e7b6d194f60839c508a84305aaca1789b6";
const GT_ADD_HEX: &str = "079ab7b345eb23c944c957a36a6b74c37537163d4cbf73bad9751de1dd9c68ef72cb21447e259880f72a871c3eda1b0c017f1c95cf79b22b459599ea57e613e00cb75e35de1f837814a93b443c54241015ac9761f8fb20a44512ff5cfc04ac7f0f6b8b52b2b5d0661cbf232820a257b8c5594309c01c2a45e64c6a7142301e4fb36e6e16b5a85bd2e437599d103c3ace06d8046c6b3424c4cd2d72ce98d279f2290a28a87e8664cb0040580d0c485f34df45267f8c215dcbcd862787ab555c7e113286dee21c9c63a458898beb35914dc8daaac453441e7114b21af7b5f47d559879d477cf2a9cbd5b40c86becd071280900410bb2751d0a6af0fe175dcf9d864ecaac463c6218745b543f9e06289922434ee446030923a3e4c4473b4e3b1914081abd33a78d31eb8d4c1bb3baab0529bb7baf1103d848b4cead1a8e0aa7a7b260fbe79c67dbe41ca4d65ba8a54a72b61692a61ce5f4d7a093b2c46aa4bca6c4a66cf873d405ebc9c35d8aa639763720177b23beffaf522d5e41d3c5310ea3331409cebef9ef393aa00f2ac64673675521e8fc8fddaf90976e607e62a740ac59c3dddf95a6de4fba15beb30c43d4e3f803a3734dbeb064bf4bc4a03f945a4921e49d04ab8d45fd753a28b8fa082616b4b17bbcb685e455ff3bf8f60c3bd32a0c185ef728cf41a1b7b700b7e445f0b372bc29e370bc227d443c70ae9dbcf73fee8acedbd317a286a53266562d817269c004fb0f149dd925d2c590a960936763e519c2b62e14c7759f96672cd852194325904197b0b19c6b528ab33566946af39b";
const GT_MUL_POS_HEX: &str = "18b2db6b3286baea116ccad8f5554d170a69b329a6de5b24c50b8834965242001a1c58089fd872b211acd3263897fa660b117248d69d8ac745283a3e6a4ccec607f6cf7cedee919575d4b7c8ae14c36001f76be5fca50adc296ef8df4926fa7f0b55a75f255fe61fc2da7cffe56adc8775aaab54c50d0c4952ad919d90fb0eb221c41abb9f2352a11be2d7f176abe41e0e30afb34fc2ce16136de66900d92068f30011e9882c0a56e7e7b30f08442be9e58d093e1888151136259d059fb539210d635bc491d5244a16ca28fdcf10546ec0f7104d3a419ddc081ba30ecb0cd2289010c2d385946229b7a9735adc82736914fe61ad26c6c38b787775de3b939105de055f8d7004358272a0823f6f1787a7abb6c3c59c8c9cbd1674ac900512632818cdd273f0d38833c07467eaf77743b70c924d43975d3821d47110a358757f926fcf970660fbdd74ef15d93b81e3aa290c78f59cbc6ed0c1e0dcbadfd11a73eb7137850d29efeb6fa321330d0cf70f5c7f6b004bcf86ac99125f8fecf83157930bec2af89f8b378c6d7f63b0a07b3651f5207a84f62cee929d574da154ebe795d519b661086f069c9f061ba3b53dc4910ea1614c87b114e2f9ef328ac94e93d00440b412d5ae5a3c396d52d26c0cdf2156ebd3d3f60ea500c42120a7ce1f7ef80f15323118956b17c09e80e96ed4e1572461d604cde2533330c684f86680406b1d3ee830cbafe6d29c9a0a2f41e03e26095b713eb7e782144db1ec6b53047fcb606b7b665b3dd1f52e95fcf2ae59c4ab159c3f98468c0a43c36c022b548189b6";
const GT_MUL_NEG_HEX: &str = "014e367f06f92bb039aedcdd4df65fc05a0d985b4ca6b79aa2254a6c605eb424048fa7f6117b8d4da8522cd9c767b0450eef9fa162e25bd305f36d77d8fede115c807c0805968129f15c1ad8489c32c41cb49418b4aef52390900720b6d8b02c0eab6a8b1420007a88412ab65de0d04feecca0302e7806761483410365b5e771fce7e5431230ad5e9e1c280e8953c68d0bd06236e9bd188437adc14d42728c6e7177399b6b5908687f491f91ee6cca3a391ef6c098cbeaee83d962fa604a718a0c9db625a7aac25034517eb8743b5868a3803b37b94374e35f152f922ba423fb8e9b3d2b2bbf9dd602558ca5237d37420502b03d12b9230ed2a431d807b81bd18671ebf78380dd3cf490506187996e7c72f53c3914c76342a38a536ffaed478318cdd273f0d38833c07467eaf77743b70c924d43975d3821d47110a358757f926fcf970660fbdd74ef15d93b81e3aa290c78f59cbc6ed0c1e0dcbadfd11a73eb7137850d29efeb6fa321330d0cf70f5c7f6b004bcf86ac99125f8fecf83157930bec2af89f8b378c6d7f63b0a07b3651f5207a84f62cee929d574da154ebe795d519b661086f069c9f061ba3b53dc4910ea1614c87b114e2f9ef328ac94e93d00440b412d5ae5a3c396d52d26c0cdf2156ebd3d3f60ea500c42120a7ce1f7ef80f15323118956b17c09e80e96ed4e1572461d604cde2533330c684f86680406b1d3ee830cbafe6d29c9a0a2f41e03e26095b713eb7e782144db1ec6b53047fcb606b7b665b3dd1f52e95fcf2ae59c4ab159c3f98468c0a43c36c022b548189b6";
const NOT_G1: &str = "8123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const NOT_G2: &str = "8123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

#[test]
fn g1_g2_compressed_round_trip_matches_csharp_vectors() {
    let g1_bytes = hex::decode(G1_GEN).unwrap();
    let g1 = Bls12381Point::deserialize(&g1_bytes).expect("G1 generator deserializes");
    assert!(matches!(g1, Bls12381Point::G1(_)));
    assert_eq!(g1.serialize(), g1_bytes, "G1 compressed round-trip");

    let g2_bytes = hex::decode(G2_GEN).unwrap();
    let g2 = Bls12381Point::deserialize(&g2_bytes).expect("G2 generator deserializes");
    assert!(matches!(g2, Bls12381Point::G2(_)));
    assert_eq!(g2.serialize(), g2_bytes, "G2 compressed round-trip");
}

#[test]
fn gt_round_trip_and_add_match_csharp_vectors() {
    let gt_bytes = hex::decode(GT_HEX).unwrap();
    assert_eq!(gt_bytes.len(), GT_SIZE);
    let gt = Bls12381Point::deserialize(&gt_bytes).expect("Gt deserializes");
    assert!(matches!(gt, Bls12381Point::Gt(_)));
    // Self-consistency: deserialize -> serialize is identity.
    assert_eq!(gt.serialize(), gt_bytes, "Gt round-trip");
    // Semantic gate: bls12381Add(gt, gt) == gt*gt in Fp12, matching C#.
    let sum = gt.add(&gt).expect("Gt add");
    assert_eq!(
        hex::encode(sum.serialize()),
        GT_ADD_HEX,
        "Gt add matches the C# TestBls12381Add vector"
    );
}

#[test]
fn pairing_matches_csharp_vector() {
    // C# TestBls12381Pairing: e(g1, g2) serializes to s_gtHex.
    let g1 = Bls12381Point::deserialize(&hex::decode(G1_GEN).unwrap()).unwrap();
    let g2 = Bls12381Point::deserialize(&hex::decode(G2_GEN).unwrap()).unwrap();
    let gt = g1.pairing(&g2).expect("pairing g1 x g2");
    assert!(matches!(gt, Bls12381Point::Gt(_)));
    assert_eq!(hex::encode(gt.serialize()), GT_HEX, "e(g1,g2) == s_gtHex");
    // Argument typing is enforced (C# accepts only G1 then G2).
    assert!(g2.pairing(&g1).is_err(), "pairing rejects G2 x G1 ordering");
}

#[test]
fn gt_scalar_mul_matches_csharp_vectors() {
    // C# TestBls12381Mul: scalar = 32-byte LE with data[0]=0x03 (i.e. 3).
    let mut scalar = [0u8; SCALAR_SIZE];
    scalar[0] = 0x03;
    let gt = Bls12381Point::deserialize(&hex::decode(GT_HEX).unwrap()).unwrap();

    let pos = gt.mul(&scalar, false).expect("gt * 3");
    assert_eq!(hex::encode(pos.serialize()), GT_MUL_POS_HEX, "gt * 3");

    let neg = gt.mul(&scalar, true).expect("gt * -3");
    assert_eq!(hex::encode(neg.serialize()), GT_MUL_NEG_HEX, "gt * -3");

    // gt*3 and gt*(-3) are inverses: their product is the Gt identity.
    let prod = pos.add(&neg).expect("gt*3 + gt*-3");
    let one = Bls12381Point::deserialize(&hex::decode(GT_HEX).unwrap())
        .unwrap()
        .mul(&[0u8; SCALAR_SIZE], false)
        .expect("gt * 0 = identity");
    assert!(prod.equals(&one), "gt*3 * gt*-3 == identity");

    // Wrong scalar length is rejected.
    assert!(gt.mul(&[0u8; 31], false).is_err());
}

#[test]
fn equals_matches_group_and_point() {
    let g1 = Bls12381Point::deserialize(&hex::decode(G1_GEN).unwrap()).unwrap();
    let g1_again = Bls12381Point::deserialize(&hex::decode(G1_GEN).unwrap()).unwrap();
    let g2 = Bls12381Point::deserialize(&hex::decode(G2_GEN).unwrap()).unwrap();

    assert!(g1.equals(&g1_again), "same G1 point is equal");
    assert!(!g1.equals(&g2), "G1 vs G2 is never equal");
}

#[test]
fn add_rejects_cross_group() {
    let g1 = Bls12381Point::deserialize(&hex::decode(G1_GEN).unwrap()).unwrap();
    let g2 = Bls12381Point::deserialize(&hex::decode(G2_GEN).unwrap()).unwrap();
    assert!(g1.add(&g2).is_err(), "adding G1 + G2 is rejected");
}

#[test]
fn rejects_invalid_and_wrong_length() {
    // C# TestNotG1 / TestNotG2: well-formed length but not valid points.
    assert!(Bls12381Point::deserialize(&hex::decode(NOT_G1).unwrap()).is_err());
    assert!(Bls12381Point::deserialize(&hex::decode(NOT_G2).unwrap()).is_err());
    // Unsupported lengths.
    assert!(Bls12381Point::deserialize(&[]).is_err());
    assert!(Bls12381Point::deserialize(&[0u8; 100]).is_err());
}

// --- Canonicity parity with C# (`Scalar.FromBytes` / `Fp.FromBytes`) ---

// BLS12-381 base-field modulus p, big-endian (48 bytes).
const FP_MODULUS_BE_HEX: &str = "1a0111ea397fe69a4b1ba7b6434bacd764774b84f38512bf6730d2a0f6b0f6241eabfffeb153ffffb9feffffffffaaab";
// BLS12-381 scalar-field order r, little-endian (32 bytes).
const R_MODULUS_LE: [u8; SCALAR_SIZE] = [
    0x01, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xfe, 0x5b, 0xfe, 0xff, 0x02, 0xa4, 0xbd, 0x53,
    0x05, 0xd8, 0xa1, 0x09, 0x08, 0xd8, 0x39, 0x33, 0x48, 0x7d, 0x9d, 0x29, 0x53, 0xa7, 0xed, 0x73,
];

#[test]
fn mul_rejects_non_canonical_scalar_like_csharp() {
    // C# `bls12381Mul` -> `Scalar.FromBytes(mul)` throws `FormatException` when
    // the little-endian scalar is `>= r`. blst would multiply by the raw bits,
    // so neo-rs must FAULT on the same inputs.
    let gt = Bls12381Point::deserialize(&hex::decode(GT_HEX).unwrap()).unwrap();

    // scalar == r  (non-canonical: >= r) -> reject.
    assert!(
        gt.mul(&R_MODULUS_LE, false).is_err(),
        "scalar == r must be rejected (C# Scalar.FromBytes throws)"
    );
    assert!(
        gt.mul(&R_MODULUS_LE, true).is_err(),
        "scalar == r must be rejected regardless of neg"
    );

    // scalar == r + 1  (non-canonical) -> reject.
    let mut r_plus_1 = R_MODULUS_LE;
    // r ends in ...0x01 at LE index 0; +1 -> 0x02 (no carry needed).
    r_plus_1[0] = 0x02;
    assert!(
        gt.mul(&r_plus_1, false).is_err(),
        "scalar == r+1 must be rejected"
    );

    // all-0xFF (2^256 - 1, far above r) -> reject.
    assert!(
        gt.mul(&[0xFFu8; SCALAR_SIZE], false).is_err(),
        "scalar 2^256-1 must be rejected"
    );

    // scalar == r - 1  (canonical: < r) -> accept.
    let mut r_minus_1 = R_MODULUS_LE;
    r_minus_1[0] = 0x00; // r ends in 0x01 -> r-1 ends in 0x00, no borrow.
    let out = gt.mul(&r_minus_1, false);
    assert!(
        out.is_ok(),
        "scalar == r-1 is canonical and must be accepted"
    );

    // The largest canonical scalar (r-1) and a small one both round-trip;
    // sanity-check that a normal small scalar still works.
    let mut three = [0u8; SCALAR_SIZE];
    three[0] = 0x03;
    assert!(gt.mul(&three, false).is_ok(), "scalar 3 is accepted");
}

#[test]
fn deserialize_rejects_non_canonical_gt_coefficient_like_csharp() {
    // C# `Gt.FromBytes` -> `Fp12/Fp6/Fp2.FromBytes` -> `Fp.FromBytes` throws
    // `FormatException` when any 48-byte big-endian coefficient is `>= p`.
    // blst's `blst_fp_from_bendian` silently reduces mod p, so neo-rs must
    // FAULT on a non-canonical coefficient to match the C# accept-set.
    let valid = hex::decode(GT_HEX).unwrap();
    assert_eq!(valid.len(), GT_SIZE);
    // Baseline: the valid vector deserializes.
    assert!(Bls12381Point::deserialize(&valid).is_ok());

    let p_be = hex::decode(FP_MODULUS_BE_HEX).unwrap();
    assert_eq!(p_be.len(), 48);

    // Overwrite the FIRST coefficient (bytes 0..48) with exactly p (== modulus
    // -> non-canonical, borrow check gives >= p).
    let mut coeff_eq_p = valid.clone();
    coeff_eq_p[0..48].copy_from_slice(&p_be);
    assert!(
        Bls12381Point::deserialize(&coeff_eq_p).is_err(),
        "Gt with a coefficient == p must be rejected"
    );

    // Overwrite the LAST coefficient (bytes 528..576) with p as well, to prove
    // the check covers every coefficient, not just the first.
    let mut last_eq_p = valid.clone();
    last_eq_p[528..576].copy_from_slice(&p_be);
    assert!(
        Bls12381Point::deserialize(&last_eq_p).is_err(),
        "Gt with the last coefficient == p must be rejected"
    );

    // A coefficient strictly above p (all-0xFF) is likewise non-canonical.
    let mut coeff_all_ff = valid.clone();
    coeff_all_ff[0..48].copy_from_slice(&[0xFFu8; 48]);
    assert!(
        Bls12381Point::deserialize(&coeff_all_ff).is_err(),
        "Gt with a coefficient of all-0xFF (>= p) must be rejected"
    );
}
