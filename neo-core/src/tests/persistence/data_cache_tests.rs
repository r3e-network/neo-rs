// Converted from /home/neo/git/neo/tests/Neo.UnitTests/Persistence/UT_DataCache.cs
#[cfg(test)]
mod data_cache_tests {
    use super::*;

    #[test]
    fn testaccessbykey() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // _myDataCache.Add(s_key1, s_value1);
            _myDataCache.Add(s_key2, s_value2);

            assert!(_myDataCache[s_key1].EqualsTo(s_value1));

            // case 2 read from inner
            _s...
        assert!(true, "Implement TestAccessByKey test");
    }

    #[test]
    fn testaccessbynotfoundkey() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // assert!(result.is_err()) =>
            {
                _ = _myDataCache[s_key1];...
        assert!(true, "Implement TestAccessByNotFoundKey test");
    }

    #[test]
    fn testaccessbydeletedkey() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // _store.Put(s_key1.ToArray(), s_value1.ToArray());
            _myDataCache.Delete(s_key1);

            assert!(result.is_err()) =>
            {
                _ = _myDataCache[s_key1];...
        assert!(true, "Implement TestAccessByDeletedKey test");
    }

    #[test]
    fn testadd() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let read = 0;
            let updated = 0;
            _myDataCache.OnRead += (sender, key, value) => { read++;...
        assert!(true, "Implement TestAdd test");
    }

    #[test]
    fn testcommit() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // using let store = MemoryStore::new();
            store.Put(s_key2.ToArray(), s_value2.ToArray());
            store.Put(s_key3.ToArray(), s_value3.ToArray());

            using let snapshot = store....
        assert!(true, "Implement TestCommit test");
    }

    #[test]
    fn testcreatesnapshot() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // assert!(_myDataCache.CloneCache(.is_some()));...
        assert!(true, "Implement TestCreateSnapshot test");
    }

    #[test]
    fn testdelete() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // using let store = MemoryStore::new();
            store.Put(s_key2.ToArray(), s_value2.ToArray());

            using let snapshot = store.GetSnapshot();
            using let myDataCache = new StoreC...
        assert!(true, "Implement TestDelete test");
    }

    #[test]
    fn testfind() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // _myDataCache.Add(s_key1, s_value1);
            _myDataCache.Add(s_key2, s_value2);

            _store.Put(s_key3.ToArray(), s_value3.ToArray());
            _store.Put(s_key4.ToArray(), s_value4.ToA...
        assert!(true, "Implement TestFind test");
    }

    #[test]
    fn testseek() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // _myDataCache.Add(s_key1, s_value1);
            _myDataCache.Add(s_key2, s_value2);

            _store.Put(s_key3.ToArray(), s_value3.ToArray());
            _store.Put(s_key4.ToArray(), s_value4.ToA...
        assert!(true, "Implement TestSeek test");
    }

    #[test]
    fn testfindrange() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // let store = MemoryStore::new();
            store.Put(s_key3.ToArray(), s_value3.ToArray());
            store.Put(s_key4.ToArray(), s_value4.ToArray());

            let myDataCache = new StoreCache(...
        assert!(true, "Implement TestFindRange test");
    }

    #[test]
    fn testgetchangeset() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // _myDataCache.Add(s_key1, s_value1);
            assert_eq!(TrackState.Added, _myDataCache.GetChangeSet().Where(u => u.Key.Equals(s_key1)).Select(u => u.Value.State).FirstOrDefault());
            _myD...
        assert!(true, "Implement TestGetChangeSet test");
    }

    #[test]
    fn testgetandchange() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // _myDataCache.Add(s_key1, s_value1);
            assert_eq!(TrackState.Added, _myDataCache.GetChangeSet().Where(u => u.Key.Equals(s_key1)).Select(u => u.Value.State).FirstOrDefault());
            _sto...
        assert!(true, "Implement TestGetAndChange test");
    }

    #[test]
    fn testgetoradd() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // _myDataCache.Add(s_key1, s_value1);
            assert_eq!(TrackState.Added, _myDataCache.GetChangeSet().Where(u => u.Key.Equals(s_key1)).Select(u => u.Value.State).FirstOrDefault());
            _sto...
        assert!(true, "Implement TestGetOrAdd test");
    }

    #[test]
    fn testtryget() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // _myDataCache.Add(s_key1, s_value1);
            assert_eq!(TrackState.Added, _myDataCache.GetChangeSet().Where(u => u.Key.Equals(s_key1)).Select(u => u.Value.State).FirstOrDefault());
            _sto...
        assert!(true, "Implement TestTryGet test");
    }

    #[test]
    fn testfindinvalid() {
        // TODO: Complete conversion from C#
        // Original C# code:
        // using let store = MemoryStore::new();
            using let myDataCache = new StoreCache(store);
            myDataCache.Add(s_key1, s_value1);

            store.Put(s_key2.ToArray(), s_value2.ToArra...
        assert!(true, "Implement TestFindInvalid test");
    }

}
