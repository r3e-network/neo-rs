// Converted from /home/neo/git/neo/tests/Neo.UnitTests/SmartContract/Native/UT_NeoToken.cs
#[cfg(test)]
mod neo_token_tests {
    use super::*;

    #[test]
    fn check_name() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // string json = UT_ProtocolSettings.CreateHFSettings("\"HF_Echidna\": 10");
            using let stream = new MemoryStream(Encoding.UTF8.GetBytes(json));
            let settings = ProtocolSettings.Loa...
        assert!(true, "Implement Check_Name test");
    }

    #[test]
    fn check_vote() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let persistingBlock = new Block
            {
                Header = new Header
                {
                    PrevHash = UInt256.Ze...
        assert!(true, "Implement Check_Vote test");
    }

    #[test]
    fn check_vote_sameaccounts() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let persistingBlock = new Block
            {
                Header = new Header
                {
                    PrevHash = UInt256.Ze...
        assert!(true, "Implement Check_Vote_Sameaccounts test");
    }

    #[test]
    fn check_vote_changevote() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let persistingBlock = new Block
            {
                Header = new Header
                {
                    PrevHash = UInt256.Ze...
        assert!(true, "Implement Check_Vote_ChangeVote test");
    }

    #[test]
    fn check_vote_votetonull() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let persistingBlock = new Block
            {
                Header = new Header
                {
                    PrevHash = UInt256.Ze...
        assert!(true, "Implement Check_Vote_VoteToNull test");
    }

    #[test]
    fn check_unclaimedgas() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let persistingBlock = new Block
            {
                Header = new Header
                {
                    PrevHash = UInt256.Ze...
        assert!(true, "Implement Check_UnclaimedGas test");
    }

    #[test]
    fn check_registervalidator() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();

            let keyCount = clonedCache.GetChangeSet().Count();
            let point = TestProtocolSettings.Default.StandbyValidators[0].EncodePoint(tru...
        assert!(true, "Implement Check_RegisterValidator test");
    }

    #[test]
    fn check_registervalidatorvianep27() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let point = ECPoint.Parse("021821807f923a3da004fb73871509d7635bcc05f41edef2a3ca5c941d8bbc1231", ECCurve.Secp256r1);
            let pointData...
        assert!(true, "Implement Check_RegisterValidatorViaNEP27 test");
    }

    #[test]
    fn check_unregistercandidate() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            _persistingBlock.Header.Index = 1;
            let keyCount = clonedCache.GetChangeSet().Count();
            let point = TestProtocolSetting...
        assert!(true, "Implement Check_UnregisterCandidate test");
    }

    #[test]
    fn check_getcommittee() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let keyCount = clonedCache.GetChangeSet().Count();
            let point = TestProtocolSettings.Default.StandbyValidators[0].EncodePoint(true...
        assert!(true, "Implement Check_GetCommittee test");
    }

    #[test]
    fn check_transfer() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let persistingBlock = new Block
            {
                Header = new Header
                {
                    PrevHash = UInt256.Ze...
        assert!(true, "Implement Check_Transfer test");
    }

    #[test]
    fn check_balanceof() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            byte[] account = Contract.GetBFTAddress(TestProtocolSettings.Default.StandbyValidators).ToArray();

            assert_eq!(100_000_000, Nativ...
        assert!(true, "Implement Check_BalanceOf test");
    }

    #[test]
    fn check_committeebonus() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let persistingBlock = new Block
            {
                Header = new()
                {
                    Index = 1,
               ...
        assert!(true, "Implement Check_CommitteeBonus test");
    }

    #[test]
    fn check_initialize() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();

            // StandbyValidators

            Check_GetCommittee(clonedCache, None);...
        assert!(true, "Implement Check_Initialize test");
    }

    #[test]
    fn testcalculatebonus() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let persistingBlock = new Block
            {
                Header = new()
                {
                    Index = 1,
                    Witness = Witness.Empty,
                    MerkleRoo...
        assert!(true, "Implement TestCalculateBonus test");
    }

    #[test]
    fn testgetnextblockvalidators1() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let snapshotCache = TestBlockchain.GetTestSnapshotCache();
            let result = (VM.Types.Array)NativeContract.NEO.Call(snapshotCache, "getNextBlockValidators");
            Assert.HasCount(7, res...
        assert!(true, "Implement TestGetNextBlockValidators1 test");
    }

    #[test]
    fn testgetnextblockvalidators2() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let result = NativeContract.NEO.GetNextBlockValidators(clonedCache, 7);
            Assert.HasCount(7, result);
            assert_eq!("02486...
        assert!(true, "Implement TestGetNextBlockValidators2 test");
    }

    #[test]
    fn testgetcandidates1() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let snapshotCache = TestBlockchain.GetTestSnapshotCache();
            let array = (VM.Types.Array)NativeContract.NEO.Call(snapshotCache, "getCandidates");
            Assert.IsEmpty(array);...
        assert!(true, "Implement TestGetCandidates1 test");
    }

    #[test]
    fn testgetcandidates2() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let result = NativeContract.NEO.GetCandidatesInternal(clonedCache);
            assert_eq!(0, result.Count());

            StorageKey key = ...
        assert!(true, "Implement TestGetCandidates2 test");
    }

    #[test]
    fn testcheckcandidate() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let cloneCache = _snapshotCache.CloneCache();
            let committee = NativeContract.NEO.GetCommittee(cloneCache);
            let point = committee[0].EncodePoint(true);

            // Prepare P...
        assert!(true, "Implement TestCheckCandidate test");
    }

    #[test]
    fn testgetcommittee() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = TestBlockchain.GetTestSnapshotCache();
            let result = (VM.Types.Array)NativeContract.NEO.Call(clonedCache, "getCommittee");
            Assert.HasCount(21, result);
       ...
        assert!(true, "Implement TestGetCommittee test");
    }

    #[test]
    fn testgetvalidators() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            let result = NativeContract.NEO.ComputeNextBlockValidators(clonedCache, TestProtocolSettings.Default);
            assert_eq!("02486fd15702c4...
        assert!(true, "Implement TestGetValidators test");
    }

    #[test]
    fn testonbalancechanging() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let ret = Transfer4TesingOnBalanceChanging(new BigInteger(0), false);
            assert!(ret.Result);
            assert!(ret.State);

            ret = Transfer4TesingOnBalanceChanging(new BigIntege...
        assert!(true, "Implement TestOnBalanceChanging test");
    }

    #[test]
    fn testtotalsupply() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            assert_eq!(new BigInteger(100000000), NativeContract.NEO.TotalSupply(clonedCache));...
        assert!(true, "Implement TestTotalSupply test");
    }

    #[test]
    fn testeconomicparameter() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // const byte Prefix_CurrentBlock = 12;
            let persistingBlock = new Block
            {
                Header = new()
                {
                    Index = 10,
                    Witn...
        assert!(true, "Implement TestEconomicParameter test");
    }

    #[test]
    fn testclaimgas() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // using let engine = ApplicationEngine.Create(TriggerType.Application,
                new Nep17NativeContractExtensions.ManualWitness(UInt160.Zero),
                _snapshotCache.CloneCache(), None, s...
        assert!(true, "Implement TestClaimGas test");
    }

    #[test]
    fn testunclaimedgas() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let persistingBlock = new Block
            {
                Header = new()
                {
                    Index = 10,
                    Witness = Witness.Empty,
                    MerkleRo...
        assert!(true, "Implement TestUnclaimedGas test");
    }

    #[test]
    fn testvote() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let clonedCache = _snapshotCache.CloneCache();
            UInt160 account = UInt160.Parse("01ff00ff00ff00ff00ff00ff00ff00ff00ff00a4");
            StorageKey keyAccount = CreateStorageKey(20, account...
        assert!(true, "Implement TestVote test");
    }

}
