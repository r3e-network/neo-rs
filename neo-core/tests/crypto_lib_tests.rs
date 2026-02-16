//! CryptoLib native contract unit tests matching C# UT_CryptoLib
//!
//! Tests for Neo.SmartContract.Native.CryptoLib functionality.

use hex::decode as hex_decode;
use hex::encode as hex_encode;
use neo_core::UInt160;
use neo_core::cryptography::{
    Bls12381Crypto, Crypto, Ed25519Crypto, NamedCurveHash, NeoHash, Secp256k1Crypto,
    Secp256r1Crypto,
};
use neo_core::hardfork::HardforkManager;
use neo_core::ledger::{TransactionVerificationContext, VerifyResult};
use neo_core::network::p2p::helper::get_sign_data_vec;
use neo_core::network::p2p::payloads::{Signer, Transaction, Witness, WitnessScope};
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::native::NativeContract;
use neo_core::smart_contract::native::crypto_lib::CryptoLib;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_vm::{OpCode, ScriptBuilder};
use num_bigint::BigInt;
use p256::ecdsa::SigningKey as P256SigningKey;
use p256::ecdsa::{Signature as P256Signature, signature::hazmat::PrehashSigner};
use secp256k1::{Message, Secp256k1, SecretKey};
use std::collections::HashMap;
use std::sync::Arc;

const BLS_G1_HEX: &str = "97f1d3a73197d7942695638c4fa9ac0fc3688c4f9774b905a14e3a3f171bac58\
6c55e83ff97a1aeffb3af00adb22c6bb";
const BLS_G2_HEX: &str = concat!(
    "93e02b6052719f607dacd3a088274f65596bd0d09920b61ab5da61bbdc7f5049",
    "334cf11213945d57e5ac7d055d042b7e024aa2b2f08f0a91260805272dc51051",
    "c6e47ad4fa403b02b4510b647ae3d1770bac0326a805bbefd48056c8c121bdb8",
);
const BLS_GT_HEX: &str = concat!(
    "0f41e58663bf08cf068672cbd01a7ec73baca4d72ca93544deff686bfd6df543",
    "d48eaa24afe47e1efde449383b67663104c581234d086a9902249b64728ffd21",
    "a189e87935a954051c7cdba7b3872629a4fafc05066245cb9108f0242d0fe3ef",
    "03350f55a7aefcd3c31b4fcb6ce5771cc6a0e9786ab5973320c806ad360829107",
    "ba810c5a09ffdd9be2291a0c25a99a211b8b424cd48bf38fcef68083b0b0ec5c",
    "81a93b330ee1a677d0d15ff7b984e8978ef48881e32fac91b93b47333e2ba570",
    "6fba23eb7c5af0d9f80940ca771b6ffd5857baaf222eb95a7d2809d61bfe02e1",
    "bfd1b68ff02f0b8102ae1c2d5d5ab1a19f26337d205fb469cd6bd15c3d5a04dc",
    "88784fbb3d0b2dbdea54d43b2b73f2cbb12d58386a8703e0f948226e47ee89d0",
    "18107154f25a764bd3c79937a45b84546da634b8f6be14a8061e55cceba478b2",
    "3f7dacaa35c8ca78beae9624045b4b601b2f522473d171391125ba84dc4007cf",
    "bf2f8da752f7c74185203fcca589ac719c34dffbbaad8431dad1c1fb597aaa519",
    "3502b86edb8857c273fa075a50512937e0794e1e65a7617c90d8bd66065b1fff",
    "e51d7a579973b1315021ec3c19934f1368bb445c7c2d209703f239689ce34c037",
    "8a68e72a6b3b216da0e22a5031b54ddff57309396b38c881c4c849ec23e87089a",
    "1c5b46e5110b86750ec6a532348868a84045483c92b7af5af689452eafabf1a8",
    "943e50439f1d59882a98eaa0170f1250ebd871fc0a92a7b2d83168d0d727272d",
    "441befa15c503dd8e90ce98db3e7b6d194f60839c508a84305aaca1789b6",
);
const BLS_NOT_G1_HEX: &str = "8123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
const BLS_NOT_G2_HEX: &str = concat!(
    "8123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
    "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
);
const BLS_ADD_GT_EXPECTED: &str = concat!(
    "079ab7b345eb23c944c957a36a6b74c37537163d4cbf73bad9751de1dd9c68ef",
    "72cb21447e259880f72a871c3eda1b0c017f1c95cf79b22b459599ea57e613e0",
    "0cb75e35de1f837814a93b443c54241015ac9761f8fb20a44512ff5cfc04ac7f",
    "0f6b8b52b2b5d0661cbf232820a257b8c5594309c01c2a45e64c6a7142301e4f",
    "b36e6e16b5a85bd2e437599d103c3ace06d8046c6b3424c4cd2d72ce98d279f2",
    "290a28a87e8664cb0040580d0c485f34df45267f8c215dcbcd862787ab555c7e",
    "113286dee21c9c63a458898beb35914dc8daaac453441e7114b21af7b5f47d55",
    "9879d477cf2a9cbd5b40c86becd071280900410bb2751d0a6af0fe175dcf9d86",
    "4ecaac463c6218745b543f9e06289922434ee446030923a3e4c4473b4e3b1914",
    "081abd33a78d31eb8d4c1bb3baab0529bb7baf1103d848b4cead1a8e0aa7a7b2",
    "60fbe79c67dbe41ca4d65ba8a54a72b61692a61ce5f4d7a093b2c46aa4bca6c4",
    "a66cf873d405ebc9c35d8aa639763720177b23beffaf522d5e41d3c5310ea333",
    "1409cebef9ef393aa00f2ac64673675521e8fc8fddaf90976e607e62a740ac59",
    "c3dddf95a6de4fba15beb30c43d4e3f803a3734dbeb064bf4bc4a03f945a4921",
    "e49d04ab8d45fd753a28b8fa082616b4b17bbcb685e455ff3bf8f60c3bd32a0c",
    "185ef728cf41a1b7b700b7e445f0b372bc29e370bc227d443c70ae9dbcf73fee",
    "8acedbd317a286a53266562d817269c004fb0f149dd925d2c590a960936763e5",
    "19c2b62e14c7759f96672cd852194325904197b0b19c6b528ab33566946af39b",
);
const BLS_MUL_GT_EXPECTED: &str = concat!(
    "18b2db6b3286baea116ccad8f5554d170a69b329a6de5b24c50b883496524200",
    "1a1c58089fd872b211acd3263897fa660b117248d69d8ac745283a3e6a4ccec6",
    "07f6cf7cedee919575d4b7c8ae14c36001f76be5fca50adc296ef8df4926fa7f",
    "0b55a75f255fe61fc2da7cffe56adc8775aaab54c50d0c4952ad919d90fb0eb2",
    "21c41abb9f2352a11be2d7f176abe41e0e30afb34fc2ce16136de66900d92068",
    "f30011e9882c0a56e7e7b30f08442be9e58d093e1888151136259d059fb53921",
    "0d635bc491d5244a16ca28fdcf10546ec0f7104d3a419ddc081ba30ecb0cd228",
    "9010c2d385946229b7a9735adc82736914fe61ad26c6c38b787775de3b939105",
    "de055f8d7004358272a0823f6f1787a7abb6c3c59c8c9cbd1674ac9005126328",
    "18cdd273f0d38833c07467eaf77743b70c924d43975d3821d47110a358757f92",
    "6fcf970660fbdd74ef15d93b81e3aa290c78f59cbc6ed0c1e0dcbadfd11a73eb",
    "7137850d29efeb6fa321330d0cf70f5c7f6b004bcf86ac99125f8fecf8315793",
    "0bec2af89f8b378c6d7f63b0a07b3651f5207a84f62cee929d574da154ebe795",
    "d519b661086f069c9f061ba3b53dc4910ea1614c87b114e2f9ef328ac94e93d0",
    "0440b412d5ae5a3c396d52d26c0cdf2156ebd3d3f60ea500c42120a7ce1f7ef8",
    "0f15323118956b17c09e80e96ed4e1572461d604cde2533330c684f86680406b",
    "1d3ee830cbafe6d29c9a0a2f41e03e26095b713eb7e782144db1ec6b53047fcb",
    "606b7b665b3dd1f52e95fcf2ae59c4ab159c3f98468c0a43c36c022b548189b6",
);
const BLS_MUL_GT_EXPECTED_NEG: &str = concat!(
    "014e367f06f92bb039aedcdd4df65fc05a0d985b4ca6b79aa2254a6c605eb424",
    "048fa7f6117b8d4da8522cd9c767b0450eef9fa162e25bd305f36d77d8fede11",
    "5c807c0805968129f15c1ad8489c32c41cb49418b4aef52390900720b6d8b02c",
    "0eab6a8b1420007a88412ab65de0d04feecca0302e7806761483410365b5e771",
    "fce7e5431230ad5e9e1c280e8953c68d0bd06236e9bd188437adc14d42728c6e",
    "7177399b6b5908687f491f91ee6cca3a391ef6c098cbeaee83d962fa604a718a",
    "0c9db625a7aac25034517eb8743b5868a3803b37b94374e35f152f922ba423fb",
    "8e9b3d2b2bbf9dd602558ca5237d37420502b03d12b9230ed2a431d807b81bd1",
    "8671ebf78380dd3cf490506187996e7c72f53c3914c76342a38a536ffaed4783",
    "18cdd273f0d38833c07467eaf77743b70c924d43975d3821d47110a358757f92",
    "6fcf970660fbdd74ef15d93b81e3aa290c78f59cbc6ed0c1e0dcbadfd11a73eb",
    "7137850d29efeb6fa321330d0cf70f5c7f6b004bcf86ac99125f8fecf8315793",
    "0bec2af89f8b378c6d7f63b0a07b3651f5207a84f62cee929d574da154ebe795",
    "d519b661086f069c9f061ba3b53dc4910ea1614c87b114e2f9ef328ac94e93d0",
    "0440b412d5ae5a3c396d52d26c0cdf2156ebd3d3f60ea500c42120a7ce1f7ef8",
    "0f15323118956b17c09e80e96ed4e1572461d604cde2533330c684f86680406b",
    "1d3ee830cbafe6d29c9a0a2f41e03e26095b713eb7e782144db1ec6b53047fcb",
    "606b7b665b3dd1f52e95fcf2ae59c4ab159c3f98468c0a43c36c022b548189b6",
);
const BLS_GT_SCALAR_MUL_POINT_1: &str = concat!(
    "14fd52fe9bfd08bbe23fcdf1d3bc5390c62e75a8786a72f8a343123a30a7c5f8",
    "d18508a21a2bf902f4db2c068913bc1c130e7ce13260d601c89ee717acfd3d4e",
    "1d80f409dd2a5c38b176f0b64d3d0a224c502717270dfecf2b825ac24608215c",
    "0d7fcfdf3c1552ada42b7e0521bc2e7389436660c352ecbf2eedf30b77b6b501",
    "df302399e6240473af47abe56fc974780c214542fcc0cf10e3001fa5e82d398f",
    "6ba1ddd1ccdf133bfd75e033eae50aec66bd5e884b8c74d4c1c6ac7c01278ac5",
    "164a54600cb2e24fec168f82542fbf98234dbb9ddf06503dc3c497da88b73db5",
    "84ba19e685b1b398b51f40160e6c8f0917b4a68dedcc04674e5f5739cf0d845b",
    "a801263f712ed4ddda59c1d9909148e3f28124ae770682c9b19233bf0bcfa00d",
    "05bfe708d381b066b83a883ba8251ce2ea6772cbde51e1322d82b2c8a026a215",
    "3f4822e20cb69b8b05003ee74e09cb481728d688caa8a671f90b55488e272f48",
    "c7c5ae32526d3635a5343eb02640358d9ac445c76a5d8f52f653bbaee04ba5ce",
    "03c68b88c25be6fd3611cc21c9968e4f87e541beeccc5170b8696a439bb666ad",
    "8a6608ab30ebc7dfe56eaf0dd9ab8439171a6e4e0d608e6e6c8ac5ddcf8d6d2a",
    "950d06051e6b6c4d3feb6dc8dac2acadd345cadfb890454a2101a112f7471f0e",
    "001701f60f3d4352c4d388c0f198854908c0e939719709c1b3f82d2a25cc7156",
    "a3838bc141e041c259849326fbd0839f15cea6a78b89349dcd1c03695a74e72d",
    "3657af4ee2cf267337bc96363ef4a1c5d5d7a673cc3a3c1a1350043f99537d62",
);
const BLS_GT_SCALAR_MUL_POINT_1_SCALAR: &str =
    "8463159bd9a1d1e1fd815172177ec24c0c291353ed88b3d1838fd9d63b1efd0b";
const BLS_GT_SCALAR_MUL_POINT_1_EXPECTED: &str = concat!(
    "03dc980ce0c037634816f9fc1edb2e1807e38a51f838e3a684f195d6c52c41d6",
    "a8a5b64d57d3fda507bebe3bd4b661af0e4f7c46754b373c955982b4d64a2483",
    "8cbc010d04b6ceb499bf411d114dab77eaf70f96ab66c2868dcd63706b602b07",
    "010c487fc16c90b61e1c2ad33c31c8f3fc86d114a59b127ac584640f149f3597",
    "102c55dd1ed8a305a10c052c0a724e570fc079e410123735a6144ccd88d9e4e9",
    "1d7b889f80b18a1741eacd6f244fce3cf57795e619b6648b9238053b4b8e4ed6",
    "115c905fbcb61525370667ff43144e12b700662a7344ac1af97f11d09779ca68",
    "65973f95ff318b42ff00df7c6eb958160947a0ab6cb25534af51ce1f0b076907",
    "c6eb5ce0760bd7670cab8814cc3308766eb6e52b5427dbf85d6424990fd33545",
    "15ab880358bc55075a08f36b855694c02ee0bd63adefe235ba4ee41dc600a1ca",
    "e950c1dc760bf7b1edd8712e9e90eebb19de705e29f4feb870129441bd4b9e91",
    "c3d37e60c12fa79a5b1e4132ba9498044e6fbf2de37e4dd88b4e9095b46f1220",
    "19e73a561ba3967b32813c3ec74b8e1b6ab619eeab698e6638114cb29ca9c3d3",
    "53192db3d392fee2b4dfdfd36b13db440534dd754417cffcd470f4d4cfdcb6d7",
    "896181c27b8b30622d7a4ca0a05a7ea67ca011cab07738235b115bbd33023969",
    "1487d2de5d679a8cad2fe5c7fff16b0b0f3f929619c8005289c3d7ffe5bcd5ea",
    "19651bfc9366682a2790cab45ee9a98815bb7e58dc666e2209cd9d700546cf18",
    "1ceb43fe719243930984b696b0d18d4cd1f5d960e149a2b753b1396e4f8f3b16",
);
const BLS_GT_SCALAR_MUL_POINT_2: &str = concat!(
    "0e0c651ff4a57adebab1fa41aa8d1e53d1cf6a6cc554282a24bb460ea0dc169d",
    "3ede8b5a93a331698f3926d273a729aa18788543413f43ada55a6a7505e3514f",
    "0db7e14d58311c3211962a350bcf908b3af90fbae31ff536fe542328ad25cd3e",
    "044a796200c8a8ead7edbc3a8a37209c5d37433ca7d8b0e644d7aac9726b524c",
    "41fef1cf0d546c252d795dffc445ddee07041f57c4c9a673bd314294e280ab61",
    "390731c09ad904bdd7b8c087d0ce857ea86e78f2d98e75d9b5e377e5751d67cf",
    "1717cbce31bc7ea6df95132549bf6d284a68005c53228127671afa54ecfd4c5c",
    "4debc437c4c6d9b9aeeee8b4159a5691128c6dc68b309fd822b14f3ce8ff390b",
    "d6834d30147e8ab2edc59d0d7b14cc13c79e6eed5fd6cae1795ba3760345d59c",
    "0c585f79c900902515e3e95938d9929ad8310e71fc7fd54be9c7529f244af40d",
    "adaca0b3bd8afd911f24b261079de48b161dd8f340d42bd84e717275193a0375",
    "d9e10fbe048bbea30abd64d3fe085c15b9be192f7baaa0b3a9658bcbb4292a0c",
    "0149beb30e54b065a75df45e5da77583f4471e3454cea90a00b5a9a224c15e2e",
    "be01f0ab8aa86591c1012c618d41fdce07ecfcaddc8dc408b7176b79d8711a41",
    "61a56f41a5be6714cbcaa70e53387ab049826ac9e636640bc6da919e52f86f32",
    "09572b62d9bfd48bd2b5ef217932237b90a70d40167623d0f25a73b753e32143",
    "10bc5b6e017aebc1a9ca0c8067a97da6162c70cc754f1b2ac3b05ba834712758",
    "c8de4641ef09237edf588989182ab3047ee42da2b840fd3633fa0f34d46ad961",
);
const BLS_GT_SCALAR_MUL_POINT_2_SCALAR_1: &str =
    "06c93a0ebbc8b5cd3af798b8f72442a67aa885b395452a08e48ec80b4e9f1b3f";
const BLS_GT_SCALAR_MUL_POINT_2_EXPECTED_1: &str = concat!(
    "0d6d91f120ab61e14a3163601ce584f053f1de9dc0a548b6fbf37a776ec7b6ce",
    "6b866e8c8b0fc0ac8d32a9a9747c98bf0e6aee5bddd058313958bfc3ac1ed752",
    "84628f92bb9b99fee101e1bee9d74bad7812287ea76bdbe07f20ff9998d6e9f0",
    "16689be1cfc4337433644a679945d5c34a6d4dd984c56d6c28428438268b385c",
    "b1d86f69b0377b18f9b084e1d0b6596213233d559a1b5caaba38be853f667fc3",
    "b1f9f2c4c9020584502ff5f370b0aba7768a1a4ca4328bc3c7be2bc9c3949f5e",
    "16fd3bfc16b11da41b7393e56e777640b000db15b6e6192e5c59dfece90c6fc0",
    "b6071fdeef7061974b5e967c5b88b1db09f7c92077c16f56aff9e9627f5e0992",
    "8e965daee17d05ef3fdc0c502b649db473b5b2bba867d829b04d32cfeab73876",
    "14190b265382378f75e4e085a5537d4f200fe56b74b7c52c5546b30d51862e1a",
    "c1f60eba157880090a42ea9b0295529f134c1fc90f19a4c20dc0be105b07e0c6",
    "7218b2f5619a66d8d770d539658eb74c255743e5847bc437fef3077d0a6c4f17",
    "198d63cf17e6957f2ad9449269af009635697e92254a3f67be9b8760fd9f9748",
    "26a1829fedb4cf66968b7c63b0c88c510da12e6d52255256757afa03ad29b5c1",
    "624292ef7eb463eb4bc81ac7426f36db3fe1513bdd31bc138bfe903bbb0c5207",
    "001335f708c16cea15ef6b77c3215326a779e927b8c2081b15adffe71ba75164",
    "e376665533c5bb59373b27dbe93a0a0e1796d821a1b9ff01846446c5ad53064c",
    "b9b941f97aa870285395e1a44c9f6e5144ea5a0cf57b9fdd962a5ec3ff1f72fe",
);
const BLS_GT_SCALAR_MUL_POINT_2_SCALAR_2: &str =
    "b0010000000000005e0000000000000071f30400000000006d9189c813000000";
const BLS_GT_SCALAR_MUL_POINT_2_EXPECTED_2: &str = concat!(
    "0919ad29cdbe0b6bbd636fbe3c8930a1b959e5aa37294a6cc7d018e277658076",
    "8bb98bf91ce1bc97f2e6fa647e7dad7b15db564645d2e4868129ed414b7e369e",
    "831b8ff93997a22b6ca0e2ba288783f535aed4b44cf3e952897db1536da18a12",
    "0a70da2b9dd901bd12a5a7047d3b6346ba1aea53b642b7355a91f957687fccd8",
    "40ef24af100d0ada6b49e35183456ec30b505098526b975477b6ca0273d3a841",
    "c85e4a8319b950e76ec217a4f939844baa6b875a4046a30c618636fe9b25c620",
    "030f31044f883789945c2bcb75d7d4099b2bc97665e75c1bee27bc3864e7e5e2",
    "ccb57a9da0b57be1a6aca217a6cfda090c4fd222f7b8cfdc32969da4fe8828a5",
    "9ee1314546efdf99ef7ede1a42df6e7a126fe83b4c41b5e70a56bd9ab499f7e8",
    "0e27a08884be05f1d2a527417fc6e30448333c0724463bf92d722ef5fd6f0694",
    "9e294e6f941976d24c856038b55a2ec200d14d958a688f23b572993bd0f18cbb",
    "c20defe88e423b262c552dcc4d9f63ad78e85efbcea9449f81f39e1a887eb79b",
    "07056bb5a672444e240660617ba7a40985a622c687c1d05c12cee7b086abfc5f",
    "39a83a5ad7638ee559f710013b772d4207924687cb30100bcd4e8c83c9fa19dc",
    "e7785bf3ae7681a0968fd9661c990e2dace05902dceeed65aacf51a04e72f0fd",
    "04858ea70fb72f2a3807dc1839a385d85b536abfd3ec76d4931b3bc5ec4d90e2",
    "ebc0342567c9507abdfafa602fc6983f13f20eb26b4169dc3908109fe3c1887d",
    "b4be8f30edad989dc8caa234f9818ac488b110ad30a30f769277168650b6910e",
);
const BLS_GT_SCALAR_MUL_POINT_3: &str = concat!(
    "0bdbfc3b68e7067630a1908de2ce15e1890d57b855ffc2ee0fe765293581c304",
    "d0507254fd9921d8ff4bff3185b1e8ae017091a6b9e243c3108b4302f30e2f4c",
    "b452c4574d23d06942cf915fb0b64c3546aa0bfbba5182dc42b63ebd09cd950f",
    "06ebf85ff360032e63d5422fed5969b80ed4abaf58d29317d9cf8e5a55744993",
    "ffc0ccc586a187c63f9c47d4b41870aa0fd73e13a4f7d3b072407a3bfa6539f8",
    "d56856542b17326ab77833df274e61a41c237a6dbf20a333698a675fded6ab1a",
    "114891795eabbedcb81590ff9bfb4b23b66c8b8376a69cf58511c80f3ac83d52",
    "c0c950be8c30d01108479f232d8e4e8919d869dc85db0b9d6ccf40eb8f8ab08e",
    "43a910c341737a55e751fa4a097ee82c5ac83d38c543d957bd9850af16039d1a",
    "00c96575d2ee24e9990b3401153446aa6593d3afb6ce7ca57d6432b8dda31aaa",
    "1a08834ad38deae5a807d11663adc5c20ae7227a2cbb7917d1489175b89ed1ba",
    "415e4fc55b7d0a286caf2f5f40b0dd39cdd8fc8c271d8a7ae952fe6ece5f7c10",
    "19bfab0167af86314a73bfa37fd16bc6edff6d9ee75610a4eec1818c668ef9f5",
    "09b1cdd54542e73dc0e343a4fd6e3bb618540c1d060b60b63b645a895105425e",
    "b813b08b6ac91be3145da04040f2a45ffcf06e96b685519fca93b0f15238dc0e",
    "030c2199127ba82fa8a193f5f01ae24270e9669923653db38cae711d68169aa2",
    "5df51a8915f3f8219892f4f5e67d550b00910011685017dcc1777a9d48689ce5",
    "90d57c1fc942d49cfad0ed7efc0169a95d7e7378af26bafb90d1619bcdab64cd",
);
const BLS_GT_SCALAR_MUL_POINT_3_SCALAR: &str =
    "688e58217305c1fd2fe0637cbd8e7414d4d0a2113314eb05592f97930d23b34d";
const BLS_GT_SCALAR_MUL_POINT_3_EXPECTED: &str = concat!(
    "056fdc84f044148950c0b7c4c0613f5710fcaeb1b023b9d8f814dc39d48702db",
    "70ce41aa276566960e37237f22b086b017b9ed0e264e2b7872c8a7affb8b9f84",
    "7a528d092a038dab4ac58d3a33d30e2e5078b5e39ebb7441c56ae7556b63ecd6",
    "139ed9be1c5eb9f987cc704c913c1e23d44d2e04377347f6c471edc40cdb2cd4",
    "e32c396194363cd21ceff9bedbd164a41050e701012f0456383210f8054e76c0",
    "906e3f37e10d4a3d6342e79e39d566ea785b385bb692cddbd6c16456dfabf19f",
    "0f84c27ec4bce096af0369ac070747cd89d97bc287afe5ed5e495ed2d743adbd",
    "8eec47df6c3a69628e803e23d824845800e44a8d874756a7541128892e55e9df",
    "1d1fe0583ef967db6740617a9ff50766866c0fa631aed8639cd0c13d3d6f6f21",
    "0b340ee315caec4cc31c916d651db5e002e259fca081fb605258ccf692d786bd",
    "5bb45a054c4d8498ac2a7fa241870df60ba0fd8a2b063740af11e7530db1e758",
    "a8e2858a443104b8337e18c083035768a0e93126f116bb9c50c8cebe30e0ceaa",
    "0c0b53eb2b6a1f96b34b6cc36f3417edda184e19ae1790d255337f14315323e1",
    "d2d7382b344bdc0b6b2cfab5837c24c916640ca351539d5459389a9c7f9b0d79",
    "e04e4a8392e0c2495dcecf7d48b10c7043825b7c6709108d81856ebf98385f0d",
    "099e6521714c48b8eb5d2e97665375175f47c57d427d35a9dc44064a99d1c079",
    "028e36d34540baba947333ab3c8976b801ea48578159f041e740ea5bf73c1de3",
    "c1043a6e03311d0f2463b72694249ccc5d603e4a93cfd8a6713fb0470383c23f",
);
const BLS_GT_SCALAR_MUL_POINT_4: &str = concat!(
    "176ec726aa447f1791e69fc70a71103c84b17385094ef06a9a0235ac7241f663",
    "5377f55ad486c216c8701d61ea2ace3e05ca1605f238dc8f29f868b795e45645",
    "c6f7ff8d9d8ffd77b5e149b0325c2a8f24dde40e80a3381ae72a9a1104ef02d7",
    "0af7cf8f2fe6ff38961b352b0fde6f8536424fc9aa5805b8e12313bdfc01d5c1",
    "db1c0a37654c307fbd252c265dcbfc040ee5605ffd6ac20aab15b0343e47831f",
    "4157a20ecedd7350d2cf070c0c7d423786fd97aa7236b99f4462fb23e1735288",
    "15bf2cf3ccbfc38303fa8154d70ee5e1e3158cbb14d5c87a773cbe948a5cfec2",
    "763c5e7129940906920aed344453b0f801760fd3eac8e254ce8e0ae4edd30c91",
    "4bea9e2935acd4a6a9d42d185a9a6e786c8e462b769b2112423f6591b0933477",
    "18897438ba918b9e4525888194b20ee17709f7dea319cfd053bb1c2227833403",
    "26953fd3763eb6feaaa4d1458ee6ca001818ad88222a97e43a71dca8d2abaef7",
    "0657b9ff7b94ca422d0c50ddb4265fa35514ed534217ce2f0219c6985ec2827a",
    "0ee1dc17940926551072d693d89e36e6d14162f414b52587e5612ed4a562c9ac",
    "15df9d5fa68ccf61d52fea64b2f5d7a600e0a8fa735105bc9a2ecb69b6d9161e",
    "55a4ccdc2285164c6846fa5bdc106d1e0693ebd5fe86432e5e88c55f0159ec32",
    "17332c8492332dfbd93970f002a6a05f23484e081f38815785e766779c843765",
    "d58b2444295a87939ad7f8fa4c11e8530a62426063c9a57cf3481a00372e443d",
    "c014fd6ef4723dd4636105d7ce7b96c4b2b3b641c3a2b6e0fa9be6187e5bfaf9",
);
const BLS_GT_SCALAR_MUL_POINT_4_SCALAR: &str =
    "0000000000000000000000000000000000000000000000000000000000000000";
const BLS_G1_SCALAR_MUL_POINT: &str = "a1f9855f7670a63e4c80d64dfe6ddedc2ed2bfaebae27e4da82d71ba474987a39808e8921d3df97df6e5d4b979234de8";
const BLS_G1_SCALAR_MUL_SCALAR: &str = BLS_GT_SCALAR_MUL_POINT_1_SCALAR;
const BLS_G1_SCALAR_MUL_EXPECTED: &str = "ae85e3e2d677c9e3424ed79b5a7554262c3d6849202b84d2e7024e4b1f2e9dd3f7cf20b807a9f2a67d87e47e9e94d361";
const BLS_G1_SCALAR_MUL_EXPECTED_NEG: &str = "8e85e3e2d677c9e3424ed79b5a7554262c3d6849202b84d2e7024e4b1f2e9dd3f7cf20b807a9f2a67d87e47e9e94d361";
const BLS_G1_SCALAR_MUL_EXPECTED_ZERO: &str = "c00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000";
const BLS_G2_SCALAR_MUL_POINT: &str = concat!(
    "a41e586fdd58d39616fea921a855e65417a5732809afc35e28466e3acaeed3d5",
    "3dd4b97ca398b2f29bf6bbcaca026a6609a42bdeaaeef42813ae225e35c23c61",
    "c293e6ecb6759048fb76ac648ba3bc49f0fcf62f73fca38cdc5e7fa5bf511365",
);
const BLS_G2_SCALAR_MUL_SCALAR: &str =
    "cbfffe3e37e53e31306addde1a1725641fbe88cd047ee7477966c44a3f764b47";
const BLS_G2_SCALAR_MUL_EXPECTED: &str = concat!(
    "88ae9bba988e854877c66dfb7ff84aa5e107861aa51d1a2a8dac2414d716a7e2",
    "19bc4b0239e4b12d2182f57b5eea82830639f2e6713098ae8d4b4c3942f36661",
    "4bac35c91c83ecb57fa90fe03094aca1ecd3555a7a6fdfa2417b5bb06917732e",
);
const BLS_G2_SCALAR_MUL_EXPECTED_NEG: &str = concat!(
    "a8ae9bba988e854877c66dfb7ff84aa5e107861aa51d1a2a8dac2414d716a7e2",
    "19bc4b0239e4b12d2182f57b5eea82830639f2e6713098ae8d4b4c3942f36661",
    "4bac35c91c83ecb57fa90fe03094aca1ecd3555a7a6fdfa2417b5bb06917732e",
);
const BLS_G2_SCALAR_MUL_EXPECTED_ZERO: &str = concat!(
    "c000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
    "0000000000000000000000000000000000000000000000000000000000000000",
);

/// Tests that CryptoLib has correct contract ID (-3)
#[test]
fn test_crypto_lib_id() {
    let crypto = CryptoLib::new();
    assert_eq!(crypto.id(), -3, "CryptoLib ID should be -3");
}

/// Tests that CryptoLib has correct name
#[test]
fn test_crypto_lib_name() {
    let crypto = CryptoLib::new();
    assert_eq!(crypto.name(), "CryptoLib", "CryptoLib name should match");
}

/// Tests SHA256 hash function
#[test]
fn test_sha256() {
    let data = b"Hello, World!";
    let hash = NeoHash::sha256(data);

    // Known SHA256 hash of "Hello, World!"
    let expected =
        hex::decode("dffd6021bb2bd5b0af676290809ec3a53191dd81c7f70a4b28688a362182986f").unwrap();

    assert_eq!(
        hash.as_slice(),
        expected.as_slice(),
        "SHA256 hash should match"
    );
}

/// Tests SHA256 with empty input
#[test]
fn test_sha256_empty() {
    let data = b"";
    let hash = NeoHash::sha256(data);

    // SHA256 of empty string
    let expected =
        hex::decode("e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855").unwrap();

    assert_eq!(
        hash.as_slice(),
        expected.as_slice(),
        "SHA256 of empty should match"
    );
}

/// Tests RIPEMD160 hash function
#[test]
fn test_ripemd160() {
    let data = b"Hello, World!";
    let hash = NeoHash::ripemd160(data);

    // Known RIPEMD160 hash of "Hello, World!" (verified with Python hashlib and OpenSSL)
    let expected = hex::decode("527a6a4b9a6da75607546842e0e00105350b1aaf").unwrap();

    assert_eq!(
        hash.as_slice(),
        expected.as_slice(),
        "RIPEMD160 hash should match"
    );
}

/// Tests RIPEMD160 with empty input
#[test]
fn test_ripemd160_empty() {
    let data = b"";
    let hash = NeoHash::ripemd160(data);

    // RIPEMD160 of empty string
    let expected = hex::decode("9c1185a5c5e9fc54612808977ee8f548b2258d31").unwrap();

    assert_eq!(
        hash.as_slice(),
        expected.as_slice(),
        "RIPEMD160 of empty should match"
    );
}

/// Tests Hash160 (SHA256 + RIPEMD160)
#[test]
fn test_hash160() {
    let data = b"test";
    let hash = NeoHash::hash160(data);

    // Hash160 is RIPEMD160(SHA256(data))
    let sha256_result = NeoHash::sha256(data);
    let expected = NeoHash::ripemd160(&sha256_result);

    assert_eq!(
        hash, expected,
        "Hash160 should equal RIPEMD160(SHA256(data))"
    );
}

/// Tests Hash256 (double SHA256)
#[test]
fn test_hash256() {
    let data = b"test";
    let hash = NeoHash::hash256(data);

    // Hash256 is SHA256(SHA256(data))
    let first_hash = NeoHash::sha256(data);
    let expected = NeoHash::sha256(&first_hash);

    assert_eq!(hash, expected, "Hash256 should equal SHA256(SHA256(data))");
}

/// Tests CryptoLib methods are registered
#[test]
fn test_crypto_lib_methods() {
    let crypto = CryptoLib::new();
    let methods = crypto.methods();

    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    assert!(method_names.contains(&"sha256"), "Should have sha256");
    assert!(method_names.contains(&"ripemd160"), "Should have ripemd160");
    assert!(method_names.contains(&"keccak256"), "Should have keccak256");
    assert!(
        method_names.contains(&"verifyWithECDsa"),
        "Should have verifyWithECDsa"
    );
    assert!(
        method_names.contains(&"verifyWithEd25519"),
        "Should have verifyWithEd25519"
    );
    assert!(
        method_names.contains(&"recoverSecp256K1"),
        "Should have recoverSecp256K1"
    );
    assert!(method_names.contains(&"murmur32"), "Should have murmur32");
}

fn protocol_settings_all_active() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    for hardfork in HardforkManager::all() {
        hardforks.insert(hardfork, 0);
    }
    settings.hardforks = hardforks;
    settings
}

fn protocol_settings_pre_cockatrice() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    for hardfork in HardforkManager::all() {
        hardforks.insert(hardfork, 1);
    }
    settings.hardforks = hardforks;
    settings
}

fn make_engine(settings: ProtocolSettings) -> ApplicationEngine {
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        400_000_000,
        None,
    )
    .expect("engine")
}

fn curve_hash_bytes(curve: NamedCurveHash) -> Vec<u8> {
    vec![curve.to_byte()]
}

fn decode_hex(value: &str) -> Vec<u8> {
    hex_decode(value).expect("hex decode")
}

const BLS_TAG_G1_AFFINE: u8 = 0x01;
const BLS_TAG_G1_PROJECTIVE: u8 = 0x02;
const BLS_TAG_G2_AFFINE: u8 = 0x03;
const BLS_TAG_G2_PROJECTIVE: u8 = 0x04;
const BLS_TAG_GT: u8 = 0x05;

fn bls_affine_tag_for_len(len: usize) -> u8 {
    match len {
        48 => BLS_TAG_G1_AFFINE,
        96 => BLS_TAG_G2_AFFINE,
        576 => BLS_TAG_GT,
        _ => panic!("unsupported BLS point length"),
    }
}

fn bls_projective_tag_for_len(len: usize) -> u8 {
    match len {
        48 => BLS_TAG_G1_PROJECTIVE,
        96 => BLS_TAG_G2_PROJECTIVE,
        576 => BLS_TAG_GT,
        _ => panic!("unsupported BLS point length"),
    }
}

fn encode_bls_interop(point: &[u8]) -> Vec<u8> {
    let tag = bls_affine_tag_for_len(point.len());
    let mut out = Vec::with_capacity(point.len() + 1);
    out.push(tag);
    out.extend_from_slice(point);
    out
}

fn decode_bls_interop(result: Vec<u8>, expected_tag: u8) -> Vec<u8> {
    assert!(!result.is_empty(), "empty interop payload");
    assert_eq!(result[0], expected_tag, "unexpected interop payload tag");
    result[1..].to_vec()
}

fn assert_native_keccak256(input: &[u8], expected_hex: &str) {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let result = engine
        .call_native_contract(crypto.hash(), "keccak256", &[input.to_vec()])
        .expect("keccak256");
    assert_eq!(hex_encode(result), expected_hex);
}

fn call_bls_mul(
    engine: &mut ApplicationEngine,
    crypto: &CryptoLib,
    point_hex: &str,
    scalar_hex: &str,
    neg: bool,
) -> Vec<u8> {
    let point = decode_hex(point_hex);
    let encoded_point = encode_bls_interop(&point);
    let scalar = decode_hex(scalar_hex);
    let neg_flag = vec![if neg { 1 } else { 0 }];
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "bls12381Mul",
            &[encoded_point, scalar, neg_flag],
        )
        .expect("bls12381Mul");
    let expected_tag = bls_projective_tag_for_len(point.len());
    decode_bls_interop(result, expected_tag)
}

#[test]
fn crypto_lib_sha256_matches_csharp_hello_world() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let result = engine
        .call_native_contract(crypto.hash(), "sha256", &[b"Hello, world!".to_vec()])
        .expect("sha256");
    assert_eq!(
        hex_encode(result),
        "315f5bdb76d078c43b8ac0064e4a0164612b1fce77c869345bfc94c75894edd3"
    );
}

#[test]
fn crypto_lib_ripemd160_matches_csharp_hello_world() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let result = engine
        .call_native_contract(crypto.hash(), "ripemd160", &[b"Hello, world!".to_vec()])
        .expect("ripemd160");
    assert_eq!(
        hex_encode(result),
        "58262d1fbdbe4530d8865d3518c6d6e41002610f"
    );
}

#[test]
fn crypto_lib_murmur32_matches_csharp_hello_world() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let seed = 0u32.to_le_bytes().to_vec();
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "murmur32",
            &[b"Hello, world!".to_vec(), seed],
        )
        .expect("murmur32");
    assert_eq!(hex_encode(result), "433e36c0");
}

#[test]
fn crypto_lib_keccak256_matches_csharp_hello_world() {
    assert_native_keccak256(
        b"Hello, World!",
        "acaf3289d7b601cbd114fb36c4d29c85bbfd5e133f14cb355c3fd8d99367964f",
    );
}

#[test]
fn crypto_lib_keccak256_matches_csharp_keccak() {
    assert_native_keccak256(
        b"Keccak",
        "868c016b666c7d3698636ee1bd023f3f065621514ab61bf26f062c175fdbe7f2",
    );
}

#[test]
fn crypto_lib_keccak256_matches_csharp_cryptography() {
    assert_native_keccak256(
        b"Cryptography",
        "53d49d225dd2cfe77d8c5e2112bcc9efe77bea1c7aa5e5ede5798a36e99e2d29",
    );
}

#[test]
fn crypto_lib_keccak256_matches_csharp_testing123() {
    assert_native_keccak256(
        b"Testing123",
        "3f82db7b16b0818a1c6b2c6152e265f682d5ebcf497c9aad776ad38bc39cb6ca",
    );
}

#[test]
fn crypto_lib_keccak256_matches_csharp_long_string() {
    assert_native_keccak256(
        b"This is a longer string for Keccak256 testing purposes.",
        "24115e5c2359f85f6840b42acd2f7ea47bc239583e576d766fa173bf711bdd2f",
    );
}

#[test]
fn crypto_lib_keccak256_matches_csharp_blank_string() {
    assert_native_keccak256(
        b"",
        "c5d2460186f7233c927e7db2dcc703c0e500b653ca82273b7bfad8045d85a470",
    );
}

#[test]
fn crypto_lib_verify_with_ecdsa_named_curve_hash_sha256() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let message = b"HelloWorld";

    let priv_r1 = hex::decode("6e63fda41e9e3aba9bb5696d58a75731f044a9bdc48fe546da571543b2fa460e")
        .expect("priv r1");
    let priv_r1: [u8; 32] = priv_r1.as_slice().try_into().expect("priv r1 len");
    let pub_r1 = Secp256r1Crypto::derive_public_key(&priv_r1)
        .expect("r1 pubkey")
        .to_vec();
    let sig_r1 = Secp256r1Crypto::sign(message, &priv_r1)
        .expect("r1 signature")
        .to_vec();
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "verifyWithECDsa",
            &[
                message.to_vec(),
                pub_r1,
                sig_r1,
                curve_hash_bytes(NamedCurveHash::Secp256r1SHA256),
            ],
        )
        .expect("verify r1");
    assert_eq!(result, vec![1]);

    let priv_k1 = hex::decode("0b5fb3a050385196b327be7d86cbce6e40a04c8832445af83ad19c82103b3ed9")
        .expect("priv k1");
    let priv_k1: [u8; 32] = priv_k1.as_slice().try_into().expect("priv k1 len");
    let pub_k1 = Secp256k1Crypto::derive_public_key(&priv_k1)
        .expect("k1 pubkey")
        .to_vec();
    let sig_k1 = Secp256k1Crypto::sign(message, &priv_k1)
        .expect("k1 signature")
        .to_vec();
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "verifyWithECDsa",
            &[
                message.to_vec(),
                pub_k1,
                sig_k1,
                curve_hash_bytes(NamedCurveHash::Secp256k1SHA256),
            ],
        )
        .expect("verify k1");
    assert_eq!(result, vec![1]);

    let bad_sig = vec![0u8; 64];
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "verifyWithECDsa",
            &[
                message.to_vec(),
                vec![0u8; 33],
                bad_sig,
                curve_hash_bytes(NamedCurveHash::Secp256r1SHA256),
            ],
        )
        .expect("verify bad");
    assert_eq!(result, vec![0]);
}

#[test]
fn crypto_lib_verify_with_ecdsa_named_curve_hash_keccak() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let message = b"HelloWorld";

    let priv_r1 = hex::decode("6e63fda41e9e3aba9bb5696d58a75731f044a9bdc48fe546da571543b2fa460e")
        .expect("priv r1");
    let priv_r1: [u8; 32] = priv_r1.as_slice().try_into().expect("priv r1 len");
    let pub_r1 = Secp256r1Crypto::derive_public_key(&priv_r1)
        .expect("r1 pubkey")
        .to_vec();
    let signing_key = P256SigningKey::from_bytes(&priv_r1.into()).expect("r1 signing key");
    let digest = Crypto::keccak256(message);
    let sig_r1: P256Signature = signing_key
        .sign_prehash(&digest)
        .expect("r1 keccak signature");
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "verifyWithECDsa",
            &[
                message.to_vec(),
                pub_r1,
                sig_r1.to_bytes().to_vec(),
                curve_hash_bytes(NamedCurveHash::Secp256r1Keccak256),
            ],
        )
        .expect("verify r1 keccak");
    assert_eq!(result, vec![1]);

    let priv_k1 = hex::decode("0b5fb3a050385196b327be7d86cbce6e40a04c8832445af83ad19c82103b3ed9")
        .expect("priv k1");
    let priv_k1: [u8; 32] = priv_k1.as_slice().try_into().expect("priv k1 len");
    let pub_k1 = Secp256k1Crypto::derive_public_key(&priv_k1)
        .expect("k1 pubkey")
        .to_vec();
    let secp = Secp256k1::new();
    let secret = SecretKey::from_slice(&priv_k1).expect("k1 secret");
    let digest = Crypto::keccak256(message);
    let msg = Message::from_digest_slice(&digest).expect("k1 message");
    let sig_k1 = secp.sign_ecdsa(&msg, &secret).serialize_compact();
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "verifyWithECDsa",
            &[
                message.to_vec(),
                pub_k1,
                sig_k1.to_vec(),
                curve_hash_bytes(NamedCurveHash::Secp256k1Keccak256),
            ],
        )
        .expect("verify k1 keccak");
    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_lib_verify_with_ecdsa_custom_tx_witness_single_sig() {
    let settings = protocol_settings_all_active();
    let priv_key = hex::decode("7177f0d04c79fa0b8c91fe90c1cf1d44772d1fba6e5eb9b281a22cd3aafb51fe")
        .expect("priv key");
    let priv_key: [u8; 32] = priv_key.as_slice().try_into().expect("priv key len");
    let pub_key = Secp256k1Crypto::derive_public_key(&priv_key).expect("pub key");

    let mut verification = ScriptBuilder::new();
    verification.emit_push_int(NamedCurveHash::Secp256k1Keccak256.to_byte() as i64);
    verification.emit_opcode(OpCode::SWAP);
    verification.emit_push(&pub_key);
    verification
        .emit_syscall("System.Runtime.GetNetwork")
        .expect("get network syscall");
    verification.emit_push_int(0x1_0000_0000);
    verification.emit_opcode(OpCode::ADD);
    verification.emit_opcode(OpCode::PUSH4);
    verification.emit_opcode(OpCode::LEFT);
    verification
        .emit_syscall("System.Runtime.GetScriptContainer")
        .expect("get container syscall");
    verification.emit_opcode(OpCode::PUSH0);
    verification.emit_opcode(OpCode::PICKITEM);
    verification.emit_opcode(OpCode::CAT);
    verification.emit_opcode(OpCode::PUSH4);
    verification.emit_opcode(OpCode::PACK);
    verification.emit_opcode(OpCode::PUSH0);
    verification.emit_push_string("verifyWithECDsa");
    verification.emit_push(&CryptoLib::new().hash().to_bytes());
    verification
        .emit_syscall("System.Contract.Call")
        .expect("contract call syscall");
    let verification_script = verification.to_array();

    let account = UInt160::from_script(&verification_script);
    let mut tx = Transaction::new();
    tx.set_network_fee(1_0000_0000);
    tx.set_valid_until_block(10);
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.add_signer(Signer::new(account, WitnessScope::NONE));

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let digest = Crypto::keccak256(&sign_data);
    let secp = Secp256k1::new();
    let secret = SecretKey::from_slice(&priv_key).expect("secret");
    let msg = Message::from_digest_slice(&digest).expect("message");
    let signature = secp.sign_ecdsa(&msg, &secret).serialize_compact();

    let mut invocation = ScriptBuilder::new();
    invocation.emit_push(&signature);
    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation.to_array(),
        verification_script,
    )]);

    assert_eq!(
        tx.verify_state_independent(&settings),
        VerifyResult::Succeed
    );

    let snapshot = DataCache::new(false);
    let context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(5_0000_0000i64));
    let result = tx.verify_state_dependent(&settings, &snapshot, Some(&context), &[]);
    assert_eq!(result, VerifyResult::Succeed);
}

#[test]
fn crypto_lib_verify_with_ecdsa_custom_tx_witness_multi_sig() {
    let settings = protocol_settings_all_active();

    let priv1 = hex::decode("b2dde592bfce654ef03f1ceea452d2b0112e90f9f52099bcd86697a2bd0a2b60")
        .expect("priv1");
    let priv2 = hex::decode("b9879e26941872ee6c9e6f01045681496d8170ed2cc4a54ce617b39ae1891b3a")
        .expect("priv2");
    let priv3 = hex::decode("4e1fe2561a6da01ee030589d504d62b23c26bfd56c5e07dfc9b8b74e4602832a")
        .expect("priv3");
    let priv4 = hex::decode("6dfd066bb989d3786043aa5c1f0476215d6f5c44f5fc3392dd15e2599b67a728")
        .expect("priv4");

    let priv1: [u8; 32] = priv1.as_slice().try_into().expect("priv1 len");
    let priv2: [u8; 32] = priv2.as_slice().try_into().expect("priv2 len");
    let priv3: [u8; 32] = priv3.as_slice().try_into().expect("priv3 len");
    let priv4: [u8; 32] = priv4.as_slice().try_into().expect("priv4 len");

    let mut keys = [
        (
            Secp256k1Crypto::derive_public_key(&priv1).expect("pub1"),
            priv1,
        ),
        (
            Secp256k1Crypto::derive_public_key(&priv2).expect("pub2"),
            priv2,
        ),
        (
            Secp256k1Crypto::derive_public_key(&priv3).expect("pub3"),
            priv3,
        ),
        (
            Secp256k1Crypto::derive_public_key(&priv4).expect("pub4"),
            priv4,
        ),
    ];
    keys.sort_by(|a, b| a.0.cmp(&b.0));

    let m = 3usize;
    let n = keys.len();

    let mut vrf = ScriptBuilder::new();
    vrf.emit_push_int(m as i64);
    for (pub_key, _) in keys.iter() {
        vrf.emit_push(pub_key);
    }
    vrf.emit_push_int(n as i64);

    vrf.emit_instruction(OpCode::INITSLOT, &[7, 0]);
    vrf.emit_opcode(OpCode::STLOC5);
    vrf.emit_opcode(OpCode::LDLOC5);
    vrf.emit_opcode(OpCode::PACK);
    vrf.emit_opcode(OpCode::STLOC1);
    vrf.emit_opcode(OpCode::STLOC6);

    vrf.emit_opcode(OpCode::DEPTH);
    vrf.emit_opcode(OpCode::LDLOC6);
    vrf.emit_opcode(OpCode::JMPEQ);
    vrf.emit(0);
    let sigs_len_check_end = vrf.len();
    vrf.emit_opcode(OpCode::ABORT);

    let check_start = vrf.len();
    vrf.emit_opcode(OpCode::LDLOC6);
    vrf.emit_opcode(OpCode::PACK);
    vrf.emit_opcode(OpCode::STLOC0);

    vrf.emit_syscall("System.Runtime.GetNetwork")
        .expect("get network syscall");
    vrf.emit_push_int(0x1_0000_0000);
    vrf.emit_opcode(OpCode::ADD);
    vrf.emit_opcode(OpCode::PUSH4);
    vrf.emit_opcode(OpCode::LEFT);
    vrf.emit_syscall("System.Runtime.GetScriptContainer")
        .expect("get container syscall");
    vrf.emit_opcode(OpCode::PUSH0);
    vrf.emit_opcode(OpCode::PICKITEM);
    vrf.emit_opcode(OpCode::CAT);
    vrf.emit_opcode(OpCode::STLOC2);

    vrf.emit_opcode(OpCode::PUSH0);
    vrf.emit_opcode(OpCode::STLOC3);
    vrf.emit_opcode(OpCode::PUSH0);
    vrf.emit_opcode(OpCode::STLOC4);

    let loop_start = vrf.len();
    vrf.emit_opcode(OpCode::LDLOC3);
    vrf.emit_opcode(OpCode::LDLOC6);
    vrf.emit_opcode(OpCode::GE);
    vrf.emit_opcode(OpCode::LDLOC4);
    vrf.emit_opcode(OpCode::LDLOC5);
    vrf.emit_opcode(OpCode::GE);
    vrf.emit_opcode(OpCode::OR);
    vrf.emit_opcode(OpCode::JMPIF);
    vrf.emit(0);
    let loop_condition_offset = vrf.len();

    vrf.emit_push_int(NamedCurveHash::Secp256k1Keccak256.to_byte() as i64);
    vrf.emit_opcode(OpCode::LDLOC0);
    vrf.emit_opcode(OpCode::LDLOC3);
    vrf.emit_opcode(OpCode::PICKITEM);
    vrf.emit_opcode(OpCode::LDLOC1);
    vrf.emit_opcode(OpCode::LDLOC4);
    vrf.emit_opcode(OpCode::PICKITEM);
    vrf.emit_opcode(OpCode::LDLOC2);
    vrf.emit_opcode(OpCode::PUSH4);
    vrf.emit_opcode(OpCode::PACK);
    vrf.emit_opcode(OpCode::PUSH0);
    vrf.emit_push_string("verifyWithECDsa");
    vrf.emit_push(&CryptoLib::new().hash().to_bytes());
    vrf.emit_syscall("System.Contract.Call")
        .expect("contract call syscall");

    vrf.emit_opcode(OpCode::LDLOC3);
    vrf.emit_opcode(OpCode::ADD);
    vrf.emit_opcode(OpCode::STLOC3);
    vrf.emit_opcode(OpCode::LDLOC4);
    vrf.emit_opcode(OpCode::INC);
    vrf.emit_opcode(OpCode::STLOC4);

    vrf.emit_opcode(OpCode::JMP);
    vrf.emit(0);
    let loop_end_offset = vrf.len();

    let prog_ret_offset = vrf.len();
    vrf.emit_opcode(OpCode::LDLOC3);
    vrf.emit_opcode(OpCode::LDLOC6);
    vrf.emit_opcode(OpCode::NUMEQUAL);

    let mut verification_script = vrf.to_array();
    let sigs_offset = (check_start as i32 - sigs_len_check_end as i32 + 2) as i8;
    verification_script[sigs_len_check_end - 1] = sigs_offset as u8;
    let loop_back_offset = (loop_start as i32 - loop_end_offset as i32 + 2) as i8;
    verification_script[loop_end_offset - 1] = loop_back_offset as u8;
    let loop_exit_offset = (prog_ret_offset as i32 - loop_condition_offset as i32 + 2) as i8;
    verification_script[loop_condition_offset - 1] = loop_exit_offset as u8;

    let account = UInt160::from_script(&verification_script);
    let mut tx = Transaction::new();
    tx.set_network_fee(1_0000_0000);
    tx.set_valid_until_block(10);
    tx.set_script(vec![OpCode::PUSH1 as u8]);
    tx.add_signer(Signer::new(account, WitnessScope::NONE));

    let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
    let digest = Crypto::keccak256(&sign_data);
    let secp = Secp256k1::new();
    let msg = Message::from_digest_slice(&digest).expect("message");

    let mut invocation = ScriptBuilder::new();
    for (index, (_, priv_key)) in keys.iter().enumerate() {
        if index == 1 {
            continue;
        }
        let secret = SecretKey::from_slice(priv_key).expect("secret");
        let signature = secp.sign_ecdsa(&msg, &secret).serialize_compact();
        invocation.emit_push(&signature);
    }

    tx.set_witnesses(vec![Witness::new_with_scripts(
        invocation.to_array(),
        verification_script,
    )]);

    assert_eq!(
        tx.verify_state_independent(&settings),
        VerifyResult::Succeed
    );

    let snapshot = DataCache::new(false);
    let context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(5_0000_0000i64));
    let result = tx.verify_state_dependent(&settings, &snapshot, Some(&context), &[]);
    assert_eq!(result, VerifyResult::Succeed);
}

#[test]
fn crypto_lib_verify_with_ecdsa_legacy_rejects_keccak() {
    let mut engine = make_engine(protocol_settings_pre_cockatrice());
    let crypto = CryptoLib::new();
    let message = b"HelloWorld";

    let priv_r1 = [1u8; 32];
    let pub_r1 = Secp256r1Crypto::derive_public_key(&priv_r1)
        .expect("r1 pubkey")
        .to_vec();
    let sig_r1 = Secp256r1Crypto::sign(message, &priv_r1)
        .expect("r1 signature")
        .to_vec();

    let result = engine
        .call_native_contract(
            crypto.hash(),
            "verifyWithECDsa",
            &[
                message.to_vec(),
                pub_r1,
                sig_r1,
                curve_hash_bytes(NamedCurveHash::Secp256r1SHA256),
            ],
        )
        .expect("legacy verify");
    assert_eq!(result, vec![1]);

    let keccak_result = engine.call_native_contract(
        crypto.hash(),
        "verifyWithECDsa",
        &[
            message.to_vec(),
            vec![0u8; 33],
            vec![0u8; 64],
            curve_hash_bytes(NamedCurveHash::Secp256r1Keccak256),
        ],
    );
    assert!(keccak_result.is_err());
}

#[test]
fn crypto_lib_verify_with_ed25519() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let message = b"hello ed25519";

    let private_key = Ed25519Crypto::generate_private_key();
    let public_key = Ed25519Crypto::derive_public_key(&private_key)
        .expect("ed25519 pubkey")
        .to_vec();
    let signature = Ed25519Crypto::sign(message, &private_key)
        .expect("ed25519 signature")
        .to_vec();

    let result = engine
        .call_native_contract(
            crypto.hash(),
            "verifyWithEd25519",
            &[message.to_vec(), public_key.clone(), signature],
        )
        .expect("verify ed25519");
    assert_eq!(result, vec![1]);

    let bad_result = engine
        .call_native_contract(
            crypto.hash(),
            "verifyWithEd25519",
            &[message.to_vec(), public_key, vec![0u8; 64]],
        )
        .expect("verify ed25519 bad");
    assert_eq!(bad_result, vec![0]);
}

#[test]
fn crypto_lib_recover_secp256k1_roundtrip() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let message = b"recover me";
    let message_hash = Crypto::sha256(message);

    let secp = Secp256k1::new();
    let secret = SecretKey::from_slice(&[7u8; 32]).expect("secret");
    let msg = Message::from_digest_slice(&message_hash).expect("message");
    let (recid, sig_bytes) = secp
        .sign_ecdsa_recoverable(&msg, &secret)
        .serialize_compact();

    let mut signature = sig_bytes.to_vec();
    signature.push((recid.to_i32() as u8) + 27);

    let result = engine
        .call_native_contract(
            crypto.hash(),
            "recoverSecp256K1",
            &[message_hash.to_vec(), signature],
        )
        .expect("recover");

    let expected = secp256k1::PublicKey::from_secret_key(&secp, &secret)
        .serialize()
        .to_vec();
    assert_eq!(result, expected);
}

#[test]
fn crypto_lib_bls12381_deserialize_g1() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let g1 = decode_hex(BLS_G1_HEX);

    let result = decode_bls_interop(
        engine
            .call_native_contract(
                crypto.hash(),
                "bls12381Deserialize",
                std::slice::from_ref(&g1),
            )
            .expect("bls12381Deserialize g1"),
        BLS_TAG_G1_AFFINE,
    );

    assert_eq!(hex_encode(result), BLS_G1_HEX);
}

#[test]
fn crypto_lib_bls12381_deserialize_g2() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let g2 = decode_hex(BLS_G2_HEX);

    let result = decode_bls_interop(
        engine
            .call_native_contract(
                crypto.hash(),
                "bls12381Deserialize",
                std::slice::from_ref(&g2),
            )
            .expect("bls12381Deserialize g2"),
        BLS_TAG_G2_AFFINE,
    );

    assert_eq!(hex_encode(result), BLS_G2_HEX);
}

#[test]
fn crypto_lib_bls12381_deserialize_gt() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let gt = decode_hex(BLS_GT_HEX);

    let result = decode_bls_interop(
        engine
            .call_native_contract(
                crypto.hash(),
                "bls12381Deserialize",
                std::slice::from_ref(&gt),
            )
            .expect("bls12381Deserialize gt"),
        BLS_TAG_GT,
    );

    assert_eq!(hex_encode(result), BLS_GT_HEX);
}

#[test]
fn crypto_lib_bls12381_deserialize_invalid_g1() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let not_g1 = decode_hex(BLS_NOT_G1_HEX);

    let result = engine.call_native_contract(crypto.hash(), "bls12381Deserialize", &[not_g1]);

    assert!(result.is_err());
}

#[test]
fn crypto_lib_bls12381_deserialize_invalid_g2() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let not_g2 = decode_hex(BLS_NOT_G2_HEX);

    let result = engine.call_native_contract(crypto.hash(), "bls12381Deserialize", &[not_g2]);

    assert!(result.is_err());
}

#[test]
fn crypto_lib_bls12381_serialize_roundtrip() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();

    let g1 = decode_hex(BLS_G1_HEX);
    let g1_interop = encode_bls_interop(&g1);
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "bls12381Serialize",
            std::slice::from_ref(&g1_interop),
        )
        .expect("bls12381Serialize g1");
    assert_eq!(hex_encode(result), BLS_G1_HEX);

    let g2 = decode_hex(BLS_G2_HEX);
    let g2_interop = encode_bls_interop(&g2);
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "bls12381Serialize",
            std::slice::from_ref(&g2_interop),
        )
        .expect("bls12381Serialize g2");
    assert_eq!(hex_encode(result), BLS_G2_HEX);

    let gt = decode_hex(BLS_GT_HEX);
    let gt_interop = encode_bls_interop(&gt);
    let result = engine
        .call_native_contract(
            crypto.hash(),
            "bls12381Serialize",
            std::slice::from_ref(&gt_interop),
        )
        .expect("bls12381Serialize gt");
    assert_eq!(hex_encode(result), BLS_GT_HEX);
}

#[test]
fn crypto_lib_bls12381_add_gt() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let gt = decode_hex(BLS_GT_HEX);
    let gt_interop = encode_bls_interop(&gt);

    let result = decode_bls_interop(
        engine
            .call_native_contract(
                crypto.hash(),
                "bls12381Add",
                &[gt_interop.clone(), gt_interop],
            )
            .expect("bls12381Add gt"),
        BLS_TAG_GT,
    );

    assert_eq!(hex_encode(result), BLS_ADD_GT_EXPECTED);
}

#[test]
fn crypto_lib_bls12381_mul_gt() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let gt = decode_hex(BLS_GT_HEX);
    let gt_interop = encode_bls_interop(&gt);
    let mut scalar = vec![0u8; 32];
    scalar[0] = 0x03;

    let result = decode_bls_interop(
        engine
            .call_native_contract(
                crypto.hash(),
                "bls12381Mul",
                &[gt_interop.clone(), scalar.clone(), vec![0]],
            )
            .expect("bls12381Mul gt"),
        BLS_TAG_GT,
    );

    assert_eq!(hex_encode(result), BLS_MUL_GT_EXPECTED);

    let result = decode_bls_interop(
        engine
            .call_native_contract(crypto.hash(), "bls12381Mul", &[gt_interop, scalar, vec![1]])
            .expect("bls12381Mul gt neg"),
        BLS_TAG_GT,
    );

    assert_eq!(hex_encode(result), BLS_MUL_GT_EXPECTED_NEG);
}

#[test]
fn crypto_lib_bls12381_mul_invalid_scalar_length() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let gt = decode_hex(BLS_GT_HEX);
    let gt_interop = encode_bls_interop(&gt);
    let scalar = vec![0x01, 0x02, 0x03];

    let result =
        engine.call_native_contract(crypto.hash(), "bls12381Mul", &[gt_interop, scalar, vec![0]]);

    assert!(result.is_err());
}

#[test]
fn crypto_lib_bls12381_mul_vectors() {
    let crypto = CryptoLib::new();
    let run = |point, scalar, neg| {
        let mut engine = make_engine(protocol_settings_all_active());
        call_bls_mul(&mut engine, &crypto, point, scalar, neg)
    };

    let result = run(
        BLS_GT_SCALAR_MUL_POINT_1,
        BLS_GT_SCALAR_MUL_POINT_1_SCALAR,
        false,
    );
    assert_eq!(hex_encode(result), BLS_GT_SCALAR_MUL_POINT_1_EXPECTED);

    let result = run(
        BLS_GT_SCALAR_MUL_POINT_2,
        BLS_GT_SCALAR_MUL_POINT_2_SCALAR_1,
        false,
    );
    assert_eq!(hex_encode(result), BLS_GT_SCALAR_MUL_POINT_2_EXPECTED_1);

    let result = run(
        BLS_GT_SCALAR_MUL_POINT_2,
        BLS_GT_SCALAR_MUL_POINT_2_SCALAR_2,
        false,
    );
    assert_eq!(hex_encode(result), BLS_GT_SCALAR_MUL_POINT_2_EXPECTED_2);

    let result = run(
        BLS_GT_SCALAR_MUL_POINT_3,
        BLS_GT_SCALAR_MUL_POINT_3_SCALAR,
        true,
    );
    assert_eq!(hex_encode(result), BLS_GT_SCALAR_MUL_POINT_3_EXPECTED);

    let result = run(
        BLS_GT_SCALAR_MUL_POINT_4,
        BLS_GT_SCALAR_MUL_POINT_4_SCALAR,
        false,
    );
    let expected = format!("{:0>1152}", "1");
    assert_eq!(hex_encode(result), expected);

    let result = run(BLS_G1_SCALAR_MUL_POINT, BLS_G1_SCALAR_MUL_SCALAR, false);
    assert_eq!(hex_encode(result), BLS_G1_SCALAR_MUL_EXPECTED);

    let result = run(BLS_G1_SCALAR_MUL_POINT, BLS_G1_SCALAR_MUL_SCALAR, true);
    assert_eq!(hex_encode(result), BLS_G1_SCALAR_MUL_EXPECTED_NEG);

    let result = run(
        BLS_G1_SCALAR_MUL_POINT,
        BLS_GT_SCALAR_MUL_POINT_4_SCALAR,
        false,
    );
    assert_eq!(hex_encode(result), BLS_G1_SCALAR_MUL_EXPECTED_ZERO);

    let result = run(BLS_G2_SCALAR_MUL_POINT, BLS_G2_SCALAR_MUL_SCALAR, false);
    assert_eq!(hex_encode(result), BLS_G2_SCALAR_MUL_EXPECTED);

    let result = run(BLS_G2_SCALAR_MUL_POINT, BLS_G2_SCALAR_MUL_SCALAR, true);
    assert_eq!(hex_encode(result), BLS_G2_SCALAR_MUL_EXPECTED_NEG);

    let result = run(
        BLS_G2_SCALAR_MUL_POINT,
        BLS_GT_SCALAR_MUL_POINT_4_SCALAR,
        false,
    );
    assert_eq!(hex_encode(result), BLS_G2_SCALAR_MUL_EXPECTED_ZERO);
}

#[test]
fn crypto_lib_bls12381_pairing_g1_g2() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let g1 = decode_hex(BLS_G1_HEX);
    let g2 = decode_hex(BLS_G2_HEX);
    let g1_interop = encode_bls_interop(&g1);
    let g2_interop = encode_bls_interop(&g2);

    let result = decode_bls_interop(
        engine
            .call_native_contract(crypto.hash(), "bls12381Pairing", &[g1_interop, g2_interop])
            .expect("bls12381Pairing"),
        BLS_TAG_GT,
    );

    assert_eq!(hex_encode(result), BLS_GT_HEX);
}

#[test]
fn crypto_lib_bls12381_equal_g1() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let g1 = decode_hex(BLS_G1_HEX);
    let g1_interop = encode_bls_interop(&g1);

    let result = engine
        .call_native_contract(
            crypto.hash(),
            "bls12381Equal",
            &[g1_interop.clone(), g1_interop],
        )
        .expect("bls12381Equal");

    assert_eq!(result, vec![1]);
}

#[test]
fn crypto_lib_bls12381_equal_type_mismatch_faults() {
    let mut engine = make_engine(protocol_settings_all_active());
    let crypto = CryptoLib::new();
    let g1 = decode_hex(BLS_G1_HEX);
    let g2 = decode_hex(BLS_G2_HEX);
    let g1_interop = encode_bls_interop(&g1);
    let g2_interop = encode_bls_interop(&g2);

    let result =
        engine.call_native_contract(crypto.hash(), "bls12381Equal", &[g1_interop, g2_interop]);

    assert!(result.is_err());
}

/// Tests BLS12-381 key derivation
#[test]
fn test_bls12381_derive_public_key() {
    // Test private key (32 bytes)
    let private_key: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];

    let result = Bls12381Crypto::derive_public_key(&private_key);
    assert!(result.is_ok(), "Should derive public key successfully");

    let public_key = result.unwrap();
    assert_eq!(
        public_key.len(),
        96,
        "BLS public key should be 96 bytes (compressed G2)"
    );
}

/// Tests BLS12-381 sign and verify
#[test]
fn test_bls12381_sign_verify() {
    let private_key: [u8; 32] = [
        0x2b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];

    let message = b"test message";

    // Derive public key
    let public_key =
        Bls12381Crypto::derive_public_key(&private_key).expect("Should derive public key");

    // Sign message
    let signature = Bls12381Crypto::sign(message, &private_key).expect("Should sign message");

    assert_eq!(
        signature.len(),
        48,
        "BLS signature should be 48 bytes (compressed G1)"
    );

    // Verify signature
    let is_valid = Bls12381Crypto::verify(message, &signature, &public_key)
        .expect("Verification should not error");

    assert!(is_valid, "Signature should be valid");
}

/// Tests BLS12-381 verify with wrong message fails
#[test]
fn test_bls12381_verify_wrong_message() {
    let private_key: [u8; 32] = [
        0x2b, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
        0x00, 0x00,
    ];

    let message = b"test message";
    let wrong_message = b"wrong message";

    let public_key =
        Bls12381Crypto::derive_public_key(&private_key).expect("Should derive public key");

    let signature = Bls12381Crypto::sign(message, &private_key).expect("Should sign message");

    // Verify with wrong message should fail
    let is_valid = Bls12381Crypto::verify(wrong_message, &signature, &public_key)
        .expect("Verification should not error");

    assert!(!is_valid, "Signature should be invalid for wrong message");
}

/// Tests BLS12-381 signature aggregation
#[test]
fn test_bls12381_aggregate_signatures() {
    let private_key1: [u8; 32] = [0x01; 32];
    let private_key2: [u8; 32] = [0x02; 32];

    let message = b"shared message";

    let sig1 = Bls12381Crypto::sign(message, &private_key1).expect("Should sign with key 1");
    let sig2 = Bls12381Crypto::sign(message, &private_key2).expect("Should sign with key 2");

    let aggregated =
        Bls12381Crypto::aggregate_signatures(&[sig1, sig2]).expect("Should aggregate signatures");

    assert_eq!(
        aggregated.len(),
        48,
        "Aggregated signature should be 48 bytes"
    );
}

/// Tests consistent hashing produces same results
#[test]
fn test_hash_consistency() {
    let data = b"consistent test data";

    // Hash the same data multiple times
    let hash1 = NeoHash::sha256(data);
    let hash2 = NeoHash::sha256(data);
    let hash3 = NeoHash::sha256(data);

    assert_eq!(hash1, hash2, "SHA256 should be deterministic");
    assert_eq!(hash2, hash3, "SHA256 should be deterministic");
}

/// Tests hash of different data produces different results
#[test]
fn test_hash_uniqueness() {
    let data1 = b"data one";
    let data2 = b"data two";

    let hash1 = NeoHash::sha256(data1);
    let hash2 = NeoHash::sha256(data2);

    assert_ne!(
        hash1, hash2,
        "Different data should produce different hashes"
    );
}
