use std::sync::Arc;
use std::sync::Mutex;
use std::error::Error;

use neo_core::basicchain;
use neo_core::config;
use neo_core::core::block;
use neo_core::core::chaindump;
use neo_core::io;
use neo_core::neotest;
use neo_core::neotest::chain;
use neo_core::require;

#[test]
fn test_blockchain_dump_and_restore() {
    let mut t = require::Test::new();

    t.run("no state root", || {
        test_dump_and_restore(&mut t, |c| {
            c.state_root_in_header = false;
            c.p2p_sig_extensions = true;
        }, None);
    });

    t.run("with state root", || {
        test_dump_and_restore(&mut t, |c| {
            c.state_root_in_header = true;
            c.p2p_sig_extensions = true;
        }, None);
    });

    t.run("remove untraceable", || {
        // Dump can only be created if all blocks and transactions are present.
        test_dump_and_restore(&mut t, |c| {
            c.p2p_sig_extensions = true;
        }, Some(|c| {
            c.max_traceable_blocks = 2;
            c.ledger.remove_untraceable_blocks = true;
            c.p2p_sig_extensions = true;
        }));
    });
}

fn test_dump_and_restore<F>(t: &mut require::Test, dump_f: F, restore_f: Option<F>)
where
    F: Fn(&mut config::Blockchain) + Copy,
{
    let restore_f = restore_f.unwrap_or(dump_f);

    let (bc, validators, committee) = chain::new_multi_with_custom_config(t, dump_f);
    let e = neotest::Executor::new(t, bc.clone(), validators, committee);

    basicchain::init(t, "../../../", &e);
    require::require_true(bc.block_height() > 5); // ensure that test is valid

    let mut w = io::BufBinWriter::new();
    require::require_no_error(chaindump::dump(&bc, &mut w.bin_writer(), 0, bc.block_height() + 1));
    require::require_no_error(w.err());

    let buf = w.bytes();
    t.run("invalid start", || {
        let (bc2, _, _) = chain::new_multi_with_custom_config(t, restore_f);

        let mut r = io::BinReader::from_buf(&buf);
        require::require_error(chaindump::restore(&bc2, &mut r, 2, 1, None));
    });

    t.run("good", || {
        let (bc2, _, _) = chain::new_multi_with_custom_config(t, dump_f);

        let mut r = io::BinReader::from_buf(&buf);
        require::require_no_error(chaindump::restore(&bc2, &mut r, 0, 2, None));
        require::require_equal(1, bc2.block_height());

        r = io::BinReader::from_buf(&buf); // new reader because start is relative to dump
        require::require_no_error(chaindump::restore(&bc2, &mut r, 2, 1, None));

        t.run("check handler", || {
            let last_index = Arc::new(Mutex::new(0));
            let err_stopped = "stopped".to_string();
            let f = |b: &block::Block| -> Result<(), Box<dyn Error>> {
                let mut last_index = last_index.lock().unwrap();
                *last_index = b.index;
                if b.index >= bc.block_height() - 1 {
                    return Err(Box::new(err_stopped.clone()));
                }
                Ok(())
            };

            require::require_no_error(chaindump::restore(&bc2, &mut r, 0, 1, Some(f.clone())));
            require::require_equal(bc2.block_height(), *last_index.lock().unwrap());

            r = io::BinReader::from_buf(&buf);
            let err = chaindump::restore(&bc2, &mut r, 4, bc.block_height() - bc2.block_height(), Some(f));
            require::require_error_is(err, &err_stopped);
            require::require_equal(bc.block_height() - 1, *last_index.lock().unwrap());
        });
    });
}
