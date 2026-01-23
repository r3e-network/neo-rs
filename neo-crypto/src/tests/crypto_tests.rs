// Converted from /home/neo/git/neo/tests/Neo.UnitTests/Cryptography/UT_Crypto.cs
#[cfg(test)]
mod crypto_tests {
    use super::*;

    #[test]
    fn testverifysignature() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let message = Encoding.Default.GetBytes("HelloWorld");
            let signature = Crypto.Sign(message, _key.PrivateKey);
            assert!(Crypto.VerifySignature(message, signature, _key.PublicKey)...
        assert!(true, "Implement TestVerifySignature test");
    }

    #[test]
    fn testsecp256k1() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // byte[] privkey = "7177f0d04c79fa0b8c91fe90c1cf1d44772d1fba6e5eb9b281a22cd3aafb51fe".HexToBytes();
            byte[] message = "2d46a712699bae19a634563d74d04cc2da497b841456da270dccb75ac2f7c4e7".HexToB...
        assert!(true, "Implement TestSecp256k1 test");
    }

    #[test]
    fn testecrecover() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // // Test case 1
            let message1 = ("5c868fedb8026979ebd26f1ba07c27eedf4ff6d10443505a96ecaf21ba8c4f09" +
                "37b3cd23ffdc3dd429d4cd1905fb8dbcceeff1350020e18b58d2ba70887baa3a" +
   ...
        assert!(true, "Implement TestECRecover test");
    }

    #[test]
    fn testerc2098() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // // Test from https://eips.ethereum.org/EIPS/eip-2098

            // Private Key: 0x1234567890123456789012345678901234567890123456789012345678901234
            // Message: "Hello World"
            /...
        assert!(true, "Implement TestERC2098 test");
    }

}
