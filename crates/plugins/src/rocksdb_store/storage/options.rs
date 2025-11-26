use rocksdb::{BlockBasedOptions, Options, ReadOptions, WriteOptions};

/// Creates the RocksDB database options mirroring Neo.Plugins.RocksDBStore.Options.CreateDbOptions().
pub fn create_db_options() -> Options {
    let mut options = Options::default();
    options.create_missing_column_families(true);
    options.create_if_missing(true);
    options.set_error_if_exists(false);
    options.set_max_open_files(1_000);
    options.set_paranoid_checks(false);
    options.set_write_buffer_size(4 << 20);

    let mut table_options = BlockBasedOptions::default();
    table_options.set_block_size(4_096);
    options.set_block_based_table_factory(&table_options);

    options
}

/// Returns the default RocksDB options used by the plugin.
pub fn db_options() -> Options {
    create_db_options()
}

/// Returns the default read options.
pub fn read_options() -> ReadOptions {
    ReadOptions::default()
}

/// Returns read options configured with the supplied snapshot.
pub fn read_options_with_snapshot<'a>(snapshot: &'a rocksdb::Snapshot<'a>) -> ReadOptions {
    let mut options = ReadOptions::default();
    options.fill_cache(false);
    options.set_snapshot(snapshot);
    options
}

/// Returns the default write options (async writes).
pub fn write_options() -> WriteOptions {
    WriteOptions::default()
}

/// Returns the synchronous write options.
pub fn write_options_sync() -> WriteOptions {
    let mut options = WriteOptions::default();
    options.set_sync(true);
    options
}
