use std::collections::HashMap;
use std::env;
use std::fs::File;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Once;
use std::str::FromStr;

use crate::compiler::DebugInfo;
use crate::util::Uint160;
use crate::vm::{self, opcode::Opcode};
use crate::testing::TB;

const GO_COVER_PROFILE_FLAG: &str = "test.coverprofile";
const DISABLE_NEOTEST_COVER: &str = "DISABLE_NEOTEST_COVER";

lazy_static! {
    static ref COVERAGE_LOCK: Mutex<()> = Mutex::new(());
    static ref RAW_COVERAGE: Mutex<HashMap<Uint160, ScriptRawCoverage>> = Mutex::new(HashMap::new());
    static ref FLAG_CHECKED: AtomicBool = AtomicBool::new(false);
    static ref COVERAGE_ENABLED: AtomicBool = AtomicBool::new(false);
    static ref COVER_PROFILE: Mutex<String> = Mutex::new(String::new());
}

struct ScriptRawCoverage {
    debug_info: Arc<DebugInfo>,
    offsets_visited: Vec<i32>,
}

struct CoverBlock {
    start_line: u32,
    start_col: u32,
    end_line: u32,
    end_col: u32,
    stmts: u32,
    counts: u32,
}

type DocumentName = String;

fn is_coverage_enabled() -> bool {
    let _lock = COVERAGE_LOCK.lock().unwrap();

    if FLAG_CHECKED.load(Ordering::SeqCst) {
        return COVERAGE_ENABLED.load(Ordering::SeqCst);
    }

    FLAG_CHECKED.store(true, Ordering::SeqCst);

    let disabled_by_environment = env::var(DISABLE_NEOTEST_COVER)
        .map(|v| bool::from_str(&v).unwrap_or(false))
        .unwrap_or(false);

    let go_tool_coverage_enabled = env::args().any(|arg| arg.starts_with(GO_COVER_PROFILE_FLAG));

    COVERAGE_ENABLED.store(!disabled_by_environment && go_tool_coverage_enabled, Ordering::SeqCst);

    if COVERAGE_ENABLED.load(Ordering::SeqCst) {
        let mut cover_profile = COVER_PROFILE.lock().unwrap();
        *cover_profile = env::args()
            .find(|arg| arg.starts_with(GO_COVER_PROFILE_FLAG))
            .unwrap_or_default();
    }

    COVERAGE_ENABLED.load(Ordering::SeqCst)
}

fn coverage_hook(script_hash: Uint160, offset: i32, _opcode: Opcode) {
    let _lock = COVERAGE_LOCK.lock().unwrap();
    if let Some(cov) = RAW_COVERAGE.lock().unwrap().get_mut(&script_hash) {
        cov.offsets_visited.push(offset);
    }
}

fn report_coverage(t: &mut dyn TB) {
    let _lock = COVERAGE_LOCK.lock().unwrap();
    let cover_profile = COVER_PROFILE.lock().unwrap();
    let mut f = File::create(&*cover_profile).unwrap_or_else(|_| {
        t.fatal(&format!("coverage: can't create file '{}' to write coverage report", *cover_profile));
        panic!();
    });
    write_coverage_report(&mut f);
}

fn write_coverage_report<W: Write>(w: &mut W) {
    writeln!(w, "mode: set").unwrap();
    let cover = process_cover();
    for (name, blocks) in cover {
        for b in blocks {
            let c = if b.counts > 0 { 1 } else { 0 };
            writeln!(w, "{}:{}.{},{}.{} {} {}", name, b.start_line, b.start_col, b.end_line, b.end_col, b.stmts, c).unwrap();
        }
    }
}

fn process_cover() -> HashMap<DocumentName, Vec<CoverBlock>> {
    let mut documents = HashMap::new();
    for script_raw_coverage in RAW_COVERAGE.lock().unwrap().values() {
        for document_name in &script_raw_coverage.debug_info.documents {
            documents.insert(document_name.clone(), ());
        }
    }

    let mut cover = HashMap::new();

    for document_name in documents.keys() {
        let mut mapped_blocks = HashMap::new();

        for script_raw_coverage in RAW_COVERAGE.lock().unwrap().values() {
            let di = &script_raw_coverage.debug_info;
            let document_seq_points = document_seq_points(di, document_name);

            for point in &document_seq_points {
                let b = CoverBlock {
                    start_line: point.start_line as u32,
                    start_col: point.start_col as u32,
                    end_line: point.end_line as u32,
                    end_col: point.end_col as u32,
                    stmts: 1 + point.end_line as u32 - point.start_line as u32,
                    counts: 0,
                };
                mapped_blocks.insert(point.opcode, b);
            }
        }

        for script_raw_coverage in RAW_COVERAGE.lock().unwrap().values() {
            let di = &script_raw_coverage.debug_info;
            let document_seq_points = document_seq_points(di, document_name);

            for offset in &script_raw_coverage.offsets_visited {
                for point in &document_seq_points {
                    if point.opcode == *offset {
                        if let Some(b) = mapped_blocks.get_mut(&point.opcode) {
                            b.counts += 1;
                        }
                    }
                }
            }
        }

        let blocks: Vec<CoverBlock> = mapped_blocks.values().cloned().collect();
        cover.insert(document_name.clone(), blocks);
    }

    cover
}

fn document_seq_points(di: &DebugInfo, doc: &DocumentName) -> Vec<DebugSeqPoint> {
    let mut res = Vec::new();
    for method_debug_info in &di.methods {
        for p in &method_debug_info.seq_points {
            if &di.documents[p.document] == doc {
                res.push(p.clone());
            }
        }
    }
    res
}

fn add_script_to_coverage(c: &Contract) {
    let _lock = COVERAGE_LOCK.lock().unwrap();
    let mut raw_coverage = RAW_COVERAGE.lock().unwrap();
    if !raw_coverage.contains_key(&c.hash) {
        raw_coverage.insert(c.hash.clone(), ScriptRawCoverage {
            debug_info: Arc::clone(&c.debug_info),
            offsets_visited: Vec::new(),
        });
    }
}
