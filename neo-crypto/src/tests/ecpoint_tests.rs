// Converted from /home/neo/git/neo/tests/Neo.UnitTests/Cryptography/ECC/UT_ECPoint.cs
#[cfg(test)]
mod ecpoint_tests {
    use super::*;

    #[test]
    fn testcompareto() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // ECFieldElement X1 = new ECFieldElement(new BigInteger(100), ECCurve.Secp256k1);
            ECFieldElement X2 = new ECFieldElement(new BigInteger(200), ECCurve.Secp256k1);
            ECFieldElement X...
        assert!(true, "Implement TestCompareTo test");
    }

    #[test]
    fn testecpointconstructor() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // ECPoint point = ECPoint::new();
            assert!(point.X.is_none());
            assert!(point.Y.is_none());
            assert_eq!(ECCurve.Secp256r1, point.Curve);

            ECFieldElement X = ...
        assert!(true, "Implement TestECPointConstructor test");
    }

    #[test]
    fn testdecodepoint() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // byte[] input1 = [0];
            Action action = () => ECPoint.DecodePoint(input1, ECCurve.Secp256k1);
            assert!(result.is_err());

            let uncompressed = s_uncompressed.HexToBytes()...
        assert!(true, "Implement TestDecodePoint test");
    }

    #[test]
    fn testdeserializefrom() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // byte[] input1 = [0];
            let reader1 = new MemoryReader(input1);
            try
            {
                ECPoint.DeserializeFrom(ref reader1, ECCurve.Secp256k1);
                Assert.F...
        assert!(true, "Implement TestDeserializeFrom test");
    }

    #[test]
    fn testencodepoint() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // ECPoint point = new ECPoint(None, None, ECCurve.Secp256k1);
            byte[] result1 = [0];
            Collectionassert_eq!(result1, point.EncodePoint(true));

            point = ECCurve.Secp256k1...
        assert!(true, "Implement TestEncodePoint test");
    }

    #[test]
    fn testequals() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let point = ECCurve.Secp256k1.G;
            assert!(point.Equals(point));
            assert!(!point.Equals(None));

            point = new ECPoint(None, None, ECCurve.Secp256k1);
            assert...
        assert!(true, "Implement TestEquals test");
    }

    #[test]
    fn testgethashcode() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let pointA = new ECPoint(ECCurve.Secp256k1.G.X, ECCurve.Secp256k1.G.Y, ECCurve.Secp256k1);
            let pointB = new ECPoint(ECCurve.Secp256k1.G.Y, ECCurve.Secp256k1.G.X, ECCurve.Secp256k1);
      ...
        assert!(true, "Implement TestGetHashCode test");
    }

    #[test]
    fn testequalsobject() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // object point = ECCurve.Secp256k1.G;
            assert!(point.Equals(point));
            assert!(!point.Equals(None));
            assert!(!point.Equals(1u));

            point = new ECPoint(None, N...
        assert!(true, "Implement TestEqualsObject test");
    }

    #[test]
    fn testfrombytes() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // byte[] input1 = [0];
            Action action = () => ECPoint.FromBytes(input1, ECCurve.Secp256k1);
            assert!(result.is_err());

            let input2 = s_uncompressed.HexToBytes();
      ...
        assert!(true, "Implement TestFromBytes test");
    }

    #[test]
    fn testgetsize() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // assert_eq!(33, ECCurve.Secp256k1.G.Size);
            assert_eq!(1, ECCurve.Secp256k1.Infinity.Size);...
        assert!(true, "Implement TestGetSize test");
    }

    #[test]
    fn testmultiply() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // ECPoint p = ECCurve.Secp256k1.G;
            BigInteger k = BigInteger.Parse("100");
            assert_eq!(
                new ECPoint(
                    new(BigInteger.Parse("10730358229073309792...
        assert!(true, "Implement TestMultiply test");
    }

    #[test]
    fn testdeserialize() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let point = new ECPoint(None, None, ECCurve.Secp256k1);
            ISerializable serializable = point;

            let input = s_uncompressed.HexToBytes();
            let reader = new MemoryReader(...
        assert!(true, "Implement TestDeserialize test");
    }

    #[test]
    fn testserialize() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let stream = MemoryStream::new();
            let point = new ECPoint(None, None, ECCurve.Secp256k1);
            ISerializable serializable = point;
            serializable.Serialize(new BinaryWrite...
        assert!(true, "Implement TestSerialize test");
    }

    #[test]
    fn testopaddition() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // assert_eq!(ECCurve.Secp256k1.Infinity + ECCurve.Secp256k1.G, ECCurve.Secp256k1.G);
            assert_eq!(ECCurve.Secp256k1.G + ECCurve.Secp256k1.Infinity, ECCurve.Secp256k1.G);
            assert_eq!...
        assert!(true, "Implement TestOpAddition test");
    }

    #[test]
    fn testopmultiply() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let p = ECCurve.Secp256k1.G;
            byte[] n = [1];
            Action action = () => p = p * n;
            assert!(result.is_err());

            p = ECCurve.Secp256k1.Infinity;
            n =...
        assert!(true, "Implement TestOpMultiply test");
    }

    #[test]
    fn testopsubtraction() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // assert_eq!(ECCurve.Secp256k1.G, ECCurve.Secp256k1.G - ECCurve.Secp256k1.Infinity);
            assert_eq!(ECCurve.Secp256k1.Infinity, ECCurve.Secp256k1.G - ECCurve.Secp256k1.G);...
        assert!(true, "Implement TestOpSubtraction test");
    }

    #[test]
    fn testopunarynegation() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // assert_eq!(new ECPoint(ECCurve.Secp256k1.G.X, -ECCurve.Secp256k1.G.Y!, ECCurve.Secp256k1), -ECCurve.Secp256k1.G);...
        assert!(true, "Implement TestOpUnaryNegation test");
    }

    #[test]
    fn testtryparse() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // assert!(!ECPoint.TryParse("00", ECCurve.Secp256k1, out var result));
            assert!(result.is_none());

            assert!(ECPoint.TryParse(s_uncompressed, ECCurve.Secp256k1, out result));
     ...
        assert!(true, "Implement TestTryParse test");
    }

    #[test]
    fn testtwice() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // assert_eq!(ECCurve.Secp256k1.Infinity, ECCurve.Secp256k1.Infinity.Twice());
            assert_eq!(
                ECCurve.Secp256k1.Infinity, new ECPoint(
                    new(BigInteger.Zero, EC...
        assert!(true, "Implement TestTwice test");
    }

}
