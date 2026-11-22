//! `ApplicationEngine.OpCodePricesV1` - dynamic opcode pricing since `HF_Huyao`.
//!
//! Transcribed from neo v3.11.2
//! `src/Neo/SmartContract/ApplicationEngine.OpCodePricesV1.cs`. Before `HF_Huyao`
//! an instruction costs `ExecFeeFactor * OpCodePriceTable[opcode]`; afterwards it
//! costs `ExecFeeFactor * price`, where `price` comes from a per-opcode
//! calculator fed with the [`RunStats`] the jump table collected while running
//! the instruction. Opcodes without a calculator use [`STATIC_COEFFICIENTS`].
//!
//! All weights are expressed in 1e-11 GAS and multiplied by `ExecFeeFactor`
//! (which already carries the `FeeFactor` decimals), so the product is femtoGAS.
use super::ApplicationEngine;
use neo_vm::OpCode;
use neo_vm::run_stats::{RunStats, StackItemType};

/// The price of `System.Contract.Call` and CALLT since `HF_Huyao`, in 1e-11 GAS
/// (C# `ApplicationEngine.Contract.cs` `ContractCallPrice`).
pub const CONTRACT_CALL_PRICE: i64 = 2_205_000;
/// The price of a single read from disk since `HF_Huyao`, in 1e-11 GAS.
pub const READ_FROM_DISK_PRICE: i64 = 6_799_366;
/// The price per public key of `System.Contract.CreateMultisigAccount` since
/// `HF_Huyao`, in 1e-11 GAS.
pub const CREATE_MULTISIG_ACCOUNT_PRICE_PER_KEY: i64 = 10_581;
/// The base price of `System.Contract.CreateMultisigAccount` since `HF_Huyao`,
/// in 1e-11 GAS.
pub const CREATE_MULTISIG_ACCOUNT_BASE_PRICE: i64 = 62_153;
/// The price of `System.Contract.CallNative` since `HF_Huyao`, in 1e-11 GAS.
pub const CALL_NATIVE_PRICE: i64 = 1_130_000;
/// The price of `System.Contract.GetCallFlags` since `HF_Huyao`, in 1e-11 GAS.
pub const GET_CALL_FLAGS_PRICE: i64 = 5_933;
/// The price of `System.Contract.CreateStandardAccount` since `HF_Huyao`, in
/// 1e-11 GAS.
pub const CREATE_STANDARD_ACCOUNT_PRICE: i64 = 41_833;
/// The price of `System.Runtime.Platform` and `System.Runtime.GetAddressVersion`
/// since `HF_Huyao`, in 1e-11 GAS.
pub const GET_PLATFORM_PRICE: i64 = 8_867;
/// The price of `System.Runtime.GetNetwork` since `HF_Huyao`, in 1e-11 GAS.
pub const GET_NETWORK_PRICE: i64 = 9_833;
/// The price of `System.Runtime.GetTime` since `HF_Huyao`, in 1e-11 GAS.
pub const GET_TIME_PRICE: i64 = 9_033;
/// The price of `System.Runtime.GetScriptContainer` since `HF_Huyao`, in 1e-11
/// GAS.
pub const GET_SCRIPT_CONTAINER_PRICE: i64 = 36_533;
/// The base price of `System.Runtime.LoadScript` since `HF_Huyao`, in 1e-11 GAS.
pub const LOAD_SCRIPT_BASE_PRICE: i64 = 700_000;
/// The price per script byte of `System.Runtime.LoadScript` since `HF_Huyao`,
/// in 1e-11 GAS.
pub const LOAD_SCRIPT_PRICE_PER_BYTE: i64 = 187;
/// The base price of `System.Runtime.CheckWitness` since `HF_Huyao`, in 1e-11
/// GAS.
pub const CHECK_WITNESS_BASE_PRICE: i64 = 1_035_000;
/// The price per rule of `System.Runtime.CheckWitness` since `HF_Huyao`, in
/// 1e-11 GAS.
pub const CHECK_WITNESS_PRICE_PER_RULE: i64 = 297_286;
/// The price of `System.Runtime.GetInvocationCounter` since `HF_Huyao`, in
/// 1e-11 GAS.
pub const GET_INVOCATION_COUNTER_PRICE: i64 = 10_267;
/// The price of `System.Runtime.GetRandom` since `HF_Huyao`, in 1e-11 GAS.
pub const GET_RANDOM_PRICE: i64 = 15_833;
/// The price of `System.Runtime.Log` since `HF_Huyao`, in 1e-11 GAS.
pub const RUNTIME_LOG_PRICE: i64 = 31_767;
/// The base price of `System.Runtime.Notify` since `HF_Huyao`, in 1e-11 GAS.
pub const NOTIFY_BASE_PRICE: i64 = 2_245_000;
/// The price per state item of `System.Runtime.Notify` since `HF_Huyao`, in
/// 1e-11 GAS.
pub const NOTIFY_PRICE_PER_ITEM: i64 = 30_024;
/// The base price of `System.Runtime.GetNotifications` since `HF_Huyao`, in
/// 1e-11 GAS.
pub const GET_NOTIFICATIONS_BASE_PRICE: i64 = 205_000;
/// The price per notification of `System.Runtime.GetNotifications` since
/// `HF_Huyao`, in 1e-11 GAS.
pub const GET_NOTIFICATIONS_PRICE_PER_NOTIFICATION: i64 = 6_926;
/// The price of `System.Runtime.BurnGas` since `HF_Huyao`, in 1e-11 GAS.
pub const BURN_GAS_PRICE: i64 = 3_067;
/// The base price of `System.Runtime.CurrentSigners` since `HF_Huyao`, in
/// 1e-11 GAS.
pub const CURRENT_SIGNERS_BASE_PRICE: i64 = 18_429;
/// The price per signer reference of `System.Runtime.CurrentSigners` since
/// `HF_Huyao`, in 1e-11 GAS.
pub const CURRENT_SIGNERS_PRICE_PER_REF: i64 = 2_892;
/// The price of `System.Storage.GetContext` since `HF_Huyao`, in 1e-11 GAS.
pub const GET_STORAGE_CONTEXT_PRICE: i64 = 12_200;
/// The price of `System.Storage.GetReadOnlyContext` since `HF_Huyao`, in
/// 1e-11 GAS.
pub const GET_READ_ONLY_CONTEXT_PRICE: i64 = 11_933;
/// The price of `System.Storage.Find`, `System.Storage.Local.Find`,
/// `System.Storage.FindWithStart` and `System.Storage.Local.FindWithStart`
/// since `HF_Huyao`, in 1e-11 GAS.
pub const STORAGE_FIND_PRICE: i64 = 6_799_366;
/// The price of `System.Iterator.Next` since `HF_Huyao`, in 1e-11 GAS.
pub const ITERATOR_NEXT_PRICE: i64 = 3_367;
/// The base price of `System.Iterator.Value` since `HF_Huyao`, in 1e-11 GAS.
pub const ITERATOR_VALUE_BASE_PRICE: i64 = 19_173;
/// The price per reference of `System.Iterator.Value` since `HF_Huyao`, in
/// 1e-11 GAS.
pub const ITERATOR_VALUE_PRICE_PER_REF: i64 = 727;
/// The price per byte of `System.Iterator.Value` since `HF_Huyao`, in 1e-11 GAS.
pub const ITERATOR_VALUE_PRICE_PER_BYTE: i64 = 8;

// Dynamic fee weights.
const APPEND_W: [i64; 3] = [97, 192, 2715];
const ASSERT_W: [i64; 3] = [99, 9, 1847];
const CAT_W: [i64; 2] = [8, 2706];
const CLEAR_W: [i64; 3] = [93, -15, 1515];
const CLEAR_ITEMS_W: [i64; 2] = [103, 1455];
const CONVERT_ANY_W: [i64; 2] = [78, 1610];
const CONVERT_ARRAY_OR_STRUCT_W: [i64; 3] = [77, 134, 3218];
const CONVERT_BYTE_ARRAY_OR_BUFFER_W: [i64; 2] = [7, 3417];
const DROP_W: [i64; 2] = [99, 1486];
const END_FINALLY_W: [i64; 2] = [93, 1137];
const HAS_KEY_W: [i64; 2] = [99, 2575];
const INIT_SLOT_W: [i64; 3] = [67, 160, 1702];
const IS_NULL_W: [i64; 2] = [97, 1236];
const IS_TYPE_W: [i64; 2] = [97, 1154];
const KEYS_W: [i64; 3] = [96, 167, 2039];
const MEMCPY_W: [i64; 2] = [1, 3514];
const NEW_ARRAY_ANY_W: [i64; 2] = [126, 2718];
const NEW_ARRAY_BYTE_OR_INT_W: [i64; 2] = [134, 2718];
const NEW_BUFFER_W: [i64; 2] = [6, 2507];
const PACK_W: [i64; 2] = [173, 2649];
const PACK_MAP_W: [i64; 3] = [80, 5281, 3481];
const PICK_ITEM_W: [i64; 2] = [91, 2751];
const POP_ITEM_W: [i64; 2] = [91, 3078];
const REMOVE_ARRAY_OR_STRUCT_W: [i64; 3] = [98, 9, 1991];
const REMOVE_MAP_W: [i64; 3] = [96, 706, 6776];
const REVERSE_ITEMS_ARR_W: [i64; 3] = [98, 19, 2043];
const REVERSE_ITEMS_BUF_W: [i64; 2] = [9, 1690];
const REVERSE_W: [i64; 2] = [19, 1702];
const ROLL_W: [i64; 2] = [5, 1910];
const SET_ITEM_W: [i64; 3] = [96, 149, 2390];
const SIZE_W: [i64; 2] = [100, 2693];
const ST_W: [i64; 2] = [98, 1599];
const SUBSTR_W: [i64; 2] = [7, 2908];
const THROW_W: [i64; 2] = [84, 1742];
const UNPACK_W: [i64; 2] = [254, 2604];
const VALUES_W: [i64; 4] = [86, 312, 168, 4758];
const X_DROP_W: [i64; 3] = [98, 6, 1791];

/// A per-opcode dynamic price calculator (`Func<RunStats, long>`).
pub type PriceFunc = fn(&RunStats) -> i64;

/// Per-opcode dynamic price calculators (`DynamicPriceTable`).
///
/// A `None` entry means the opcode is priced from [`STATIC_COEFFICIENTS`].
#[derive(Clone)]
pub struct DynamicPriceTable {
    table: [Option<PriceFunc>; 256],
}

impl DynamicPriceTable {
    /// Creates an empty table.
    #[must_use]
    pub const fn new() -> Self {
        Self { table: [None; 256] }
    }

    /// Registers the calculator of an opcode.
    pub const fn set(&mut self, opcode: OpCode, calculator: PriceFunc) {
        self.table[opcode.byte() as usize] = Some(calculator);
    }

    /// Returns the calculator of an opcode, if one is registered.
    #[must_use]
    pub fn get(&self, opcode: u8) -> Option<PriceFunc> {
        self.table[opcode as usize]
    }
}

impl Default for DynamicPriceTable {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for DynamicPriceTable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DynamicPriceTable")
            .field(
                "registered",
                &self.table.iter().filter(|entry| entry.is_some()).count(),
            )
            .finish()
    }
}

const fn compose_dynamic_price_table() -> DynamicPriceTable {
    let mut table = DynamicPriceTable::new();
    table.set(OpCode::APPEND, append_gas);
    table.set(OpCode::ASSERT, assert_gas);
    table.set(OpCode::ASSERTMSG, assert_gas);
    table.set(OpCode::CAT, cat_gas);
    table.set(OpCode::CLEAR, clear_gas);
    table.set(OpCode::CLEARITEMS, clear_items_gas);
    table.set(OpCode::CONVERT, convert_gas);
    table.set(OpCode::DROP, drop_gas);
    table.set(OpCode::ENDFINALLY, end_finally_gas);
    table.set(OpCode::HASKEY, has_key_gas);
    table.set(OpCode::INITSLOT, init_slot_gas);
    table.set(OpCode::INITSSLOT, init_slot_gas);
    table.set(OpCode::ISNULL, is_null_gas);
    table.set(OpCode::ISTYPE, is_type_gas);
    table.set(OpCode::KEYS, keys_gas);
    table.set(OpCode::LEFT, substr_gas);
    table.set(OpCode::MEMCPY, memcpy_gas);
    table.set(OpCode::NEWARRAY, new_array_gas);
    table.set(OpCode::NEWARRAY_T, new_array_gas);
    table.set(OpCode::NEWBUFFER, new_buffer_gas);
    table.set(OpCode::NEWSTRUCT, new_array_gas);
    table.set(OpCode::NIP, drop_gas);
    table.set(OpCode::PACK, pack_gas);
    table.set(OpCode::PACKMAP, pack_map_gas);
    table.set(OpCode::PACKSTRUCT, pack_gas);
    table.set(OpCode::PICKITEM, pick_item_gas);
    table.set(OpCode::POPITEM, pop_item_gas);
    table.set(OpCode::REMOVE, remove_gas);
    table.set(OpCode::REVERSEITEMS, reverse_items_gas);
    table.set(OpCode::REVERSE3, reverse_gas);
    table.set(OpCode::REVERSE4, reverse_gas);
    table.set(OpCode::REVERSEN, reverse_gas);
    table.set(OpCode::RIGHT, substr_gas);
    table.set(OpCode::ROLL, roll_gas);
    table.set(OpCode::ROT, roll_gas);
    table.set(OpCode::SETITEM, set_item_gas);
    table.set(OpCode::SIZE, size_gas);
    table.set(OpCode::STSFLD0, st_gas);
    table.set(OpCode::STSFLD1, st_gas);
    table.set(OpCode::STSFLD2, st_gas);
    table.set(OpCode::STSFLD3, st_gas);
    table.set(OpCode::STSFLD4, st_gas);
    table.set(OpCode::STSFLD5, st_gas);
    table.set(OpCode::STSFLD6, st_gas);
    table.set(OpCode::STSFLD, st_gas);
    table.set(OpCode::STLOC0, st_gas);
    table.set(OpCode::STLOC1, st_gas);
    table.set(OpCode::STLOC2, st_gas);
    table.set(OpCode::STLOC3, st_gas);
    table.set(OpCode::STLOC4, st_gas);
    table.set(OpCode::STLOC5, st_gas);
    table.set(OpCode::STLOC6, st_gas);
    table.set(OpCode::STLOC, st_gas);
    table.set(OpCode::STARG0, st_gas);
    table.set(OpCode::STARG1, st_gas);
    table.set(OpCode::STARG2, st_gas);
    table.set(OpCode::STARG3, st_gas);
    table.set(OpCode::STARG4, st_gas);
    table.set(OpCode::STARG5, st_gas);
    table.set(OpCode::STARG6, st_gas);
    table.set(OpCode::STARG, st_gas);
    table.set(OpCode::SUBSTR, substr_gas);
    table.set(OpCode::THROW, throw_gas);
    table.set(OpCode::UNPACK, unpack_gas);
    table.set(OpCode::VALUES, values_gas);
    table.set(OpCode::XDROP, x_drop_gas);
    table
}

/// The default per-opcode dynamic price calculators
/// (`ApplicationEngine.DefaultDynamicPriceTable`).
pub static DEFAULT_DYNAMIC_PRICE_TABLE: DynamicPriceTable = compose_dynamic_price_table();

const fn compose_static_coefficients() -> [i64; 256] {
    let mut table = [0i64; 256];
    table[OpCode::PUSHINT8.byte() as usize] = 2798;
    table[OpCode::PUSHINT16.byte() as usize] = 2845;
    table[OpCode::PUSHINT32.byte() as usize] = 2851;
    table[OpCode::PUSHINT64.byte() as usize] = 2908;
    table[OpCode::PUSHINT128.byte() as usize] = 3002;
    table[OpCode::PUSHINT256.byte() as usize] = 3317;
    table[OpCode::PUSHT.byte() as usize] = 1082;
    table[OpCode::PUSHF.byte() as usize] = 1082;
    table[OpCode::PUSHA.byte() as usize] = 2439;
    table[OpCode::PUSHNULL.byte() as usize] = 1082;
    table[OpCode::PUSHDATA1.byte() as usize] = 1685;
    table[OpCode::PUSHDATA2.byte() as usize] = 1685;
    table[OpCode::PUSHDATA4.byte() as usize] = 1685;
    table[OpCode::PUSHM1.byte() as usize] = 2187;
    table[OpCode::PUSH0.byte() as usize] = 2187;
    table[OpCode::PUSH1.byte() as usize] = 2187;
    table[OpCode::PUSH2.byte() as usize] = 2187;
    table[OpCode::PUSH3.byte() as usize] = 2187;
    table[OpCode::PUSH4.byte() as usize] = 2187;
    table[OpCode::PUSH5.byte() as usize] = 2187;
    table[OpCode::PUSH6.byte() as usize] = 2187;
    table[OpCode::PUSH7.byte() as usize] = 2187;
    table[OpCode::PUSH8.byte() as usize] = 2187;
    table[OpCode::PUSH9.byte() as usize] = 2187;
    table[OpCode::PUSH10.byte() as usize] = 2187;
    table[OpCode::PUSH11.byte() as usize] = 2187;
    table[OpCode::PUSH12.byte() as usize] = 2187;
    table[OpCode::PUSH13.byte() as usize] = 2187;
    table[OpCode::PUSH14.byte() as usize] = 2187;
    table[OpCode::PUSH15.byte() as usize] = 2187;
    table[OpCode::PUSH16.byte() as usize] = 2187;
    table[OpCode::NOP.byte() as usize] = 1000;
    table[OpCode::JMP.byte() as usize] = 1058;
    table[OpCode::JMP_L.byte() as usize] = 1058;
    table[OpCode::JMPIF.byte() as usize] = 1211;
    table[OpCode::JMPIF_L.byte() as usize] = 1211;
    table[OpCode::JMPIFNOT.byte() as usize] = 1211;
    table[OpCode::JMPIFNOT_L.byte() as usize] = 1211;
    table[OpCode::JMPEQ.byte() as usize] = 1569;
    table[OpCode::JMPEQ_L.byte() as usize] = 1569;
    table[OpCode::JMPNE.byte() as usize] = 1569;
    table[OpCode::JMPNE_L.byte() as usize] = 1569;
    table[OpCode::JMPGT.byte() as usize] = 1569;
    table[OpCode::JMPGT_L.byte() as usize] = 1569;
    table[OpCode::JMPGE.byte() as usize] = 1569;
    table[OpCode::JMPGE_L.byte() as usize] = 1569;
    table[OpCode::JMPLT.byte() as usize] = 1569;
    table[OpCode::JMPLT_L.byte() as usize] = 1569;
    table[OpCode::JMPLE.byte() as usize] = 1569;
    table[OpCode::JMPLE_L.byte() as usize] = 1569;
    table[OpCode::CALL.byte() as usize] = 2451;
    table[OpCode::CALL_L.byte() as usize] = 2451;
    table[OpCode::CALLA.byte() as usize] = 2927;
    table[OpCode::CALLT.byte() as usize] = CONTRACT_CALL_PRICE;
    table[OpCode::ABORT.byte() as usize] = 0;
    table[OpCode::ABORTMSG.byte() as usize] = 0;
    table[OpCode::TRY.byte() as usize] = 1753;
    table[OpCode::TRY_L.byte() as usize] = 1753;
    table[OpCode::ENDTRY.byte() as usize] = 1127;
    table[OpCode::ENDTRY_L.byte() as usize] = 1127;
    table[OpCode::RET.byte() as usize] = 1085;
    table[OpCode::SYSCALL.byte() as usize] = 0;
    table[OpCode::DEPTH.byte() as usize] = 2053;
    table[OpCode::DUP.byte() as usize] = 1166;
    table[OpCode::OVER.byte() as usize] = 1183;
    table[OpCode::PICK.byte() as usize] = 1418;
    table[OpCode::TUCK.byte() as usize] = 1379;
    table[OpCode::SWAP.byte() as usize] = 1041;
    table[OpCode::LDSFLD0.byte() as usize] = 1103;
    table[OpCode::LDSFLD1.byte() as usize] = 1103;
    table[OpCode::LDSFLD2.byte() as usize] = 1103;
    table[OpCode::LDSFLD3.byte() as usize] = 1103;
    table[OpCode::LDSFLD4.byte() as usize] = 1103;
    table[OpCode::LDSFLD5.byte() as usize] = 1103;
    table[OpCode::LDSFLD6.byte() as usize] = 1103;
    table[OpCode::LDSFLD.byte() as usize] = 1103;
    table[OpCode::LDLOC0.byte() as usize] = 1103;
    table[OpCode::LDLOC1.byte() as usize] = 1103;
    table[OpCode::LDLOC2.byte() as usize] = 1103;
    table[OpCode::LDLOC3.byte() as usize] = 1103;
    table[OpCode::LDLOC4.byte() as usize] = 1103;
    table[OpCode::LDLOC5.byte() as usize] = 1103;
    table[OpCode::LDLOC6.byte() as usize] = 1103;
    table[OpCode::LDLOC.byte() as usize] = 1103;
    table[OpCode::LDARG0.byte() as usize] = 1103;
    table[OpCode::LDARG1.byte() as usize] = 1103;
    table[OpCode::LDARG2.byte() as usize] = 1103;
    table[OpCode::LDARG3.byte() as usize] = 1103;
    table[OpCode::LDARG4.byte() as usize] = 1103;
    table[OpCode::LDARG5.byte() as usize] = 1103;
    table[OpCode::LDARG6.byte() as usize] = 1103;
    table[OpCode::LDARG.byte() as usize] = 1103;
    table[OpCode::INVERT.byte() as usize] = 3122;
    table[OpCode::AND.byte() as usize] = 2967;
    table[OpCode::OR.byte() as usize] = 3044;
    table[OpCode::XOR.byte() as usize] = 3120;
    table[OpCode::EQUAL.byte() as usize] = 1610;
    table[OpCode::NOTEQUAL.byte() as usize] = 1610;
    table[OpCode::SIGN.byte() as usize] = 2520;
    table[OpCode::ABS.byte() as usize] = 2745;
    table[OpCode::NEGATE.byte() as usize] = 2822;
    table[OpCode::INC.byte() as usize] = 3099;
    table[OpCode::DEC.byte() as usize] = 3053;
    table[OpCode::ADD.byte() as usize] = 3153;
    table[OpCode::SUB.byte() as usize] = 3233;
    table[OpCode::MUL.byte() as usize] = 3541;
    table[OpCode::DIV.byte() as usize] = 6459;
    table[OpCode::MOD.byte() as usize] = 6522;
    table[OpCode::POW.byte() as usize] = 8277;
    table[OpCode::SQRT.byte() as usize] = 33802;
    table[OpCode::MODMUL.byte() as usize] = 8136;
    table[OpCode::MODPOW.byte() as usize] = 1648443;
    table[OpCode::SHL.byte() as usize] = 3711;
    table[OpCode::SHR.byte() as usize] = 4315;
    table[OpCode::NOT.byte() as usize] = 1262;
    table[OpCode::BOOLAND.byte() as usize] = 1444;
    table[OpCode::BOOLOR.byte() as usize] = 1476;
    table[OpCode::NZ.byte() as usize] = 1253;
    table[OpCode::NUMEQUAL.byte() as usize] = 1627;
    table[OpCode::NUMNOTEQUAL.byte() as usize] = 1627;
    table[OpCode::LT.byte() as usize] = 1662;
    table[OpCode::LE.byte() as usize] = 1662;
    table[OpCode::GT.byte() as usize] = 1662;
    table[OpCode::GE.byte() as usize] = 1662;
    table[OpCode::MIN.byte() as usize] = 1725;
    table[OpCode::MAX.byte() as usize] = 1725;
    table[OpCode::WITHIN.byte() as usize] = 2519;
    table[OpCode::NEWARRAY0.byte() as usize] = 1774;
    table[OpCode::NEWSTRUCT0.byte() as usize] = 1774;
    table[OpCode::NEWMAP.byte() as usize] = 2720;
    table
}

/// The flat per-opcode coefficients used by opcodes without a dynamic
/// calculator (`ApplicationEngine.StaticCoefficients`), indexed by opcode byte.
pub const STATIC_COEFFICIENTS: [i64; 256] = compose_static_coefficients();

impl ApplicationEngine {
    /// Returns the dynamic price table this engine prices opcodes with.
    #[must_use]
    pub fn dynamic_price_table(&self) -> &DynamicPriceTable {
        self.dynamic_price_table
    }

    /// Gets the price of an opcode since `HF_Huyao`, in femtoGAS.
    ///
    /// C# `ApplicationEngine.OpcodeV1(long baseFee, OpCode opcode, RunStats param)`:
    /// `baseFee` is the execution fee factor (already carrying the `FeeFactor`
    /// decimals) and the returned product is a femtoGAS amount.
    #[must_use]
    pub fn opcode_v1(&self, base_fee: i64, opcode: u8, stats: &RunStats) -> i64 {
        let price = match self.dynamic_price_table.get(opcode) {
            Some(calculator) => calculator(stats),
            None => STATIC_COEFFICIENTS[opcode as usize],
        };
        base_fee.saturating_mul(price)
    }
}

fn append_gas(stats: &RunStats) -> i64 {
    APPEND_W[0] * i64::from(stats.refs_delta())
        + APPEND_W[1] * i64::from(stats.cloned_items())
        + APPEND_W[2]
}

fn assert_gas(stats: &RunStats) -> i64 {
    ASSERT_W[0] * i64::from(stats.refs_delta())
        + ASSERT_W[1] * i64::from(stats.length())
        + ASSERT_W[2]
}

fn cat_gas(stats: &RunStats) -> i64 {
    CAT_W[0] * i64::from(stats.length()) + CAT_W[1]
}

fn clear_gas(stats: &RunStats) -> i64 {
    CLEAR_W[0] * i64::from(stats.refs_delta()) + CLEAR_W[1] * i64::from(stats.length()) + CLEAR_W[2]
}

fn clear_items_gas(stats: &RunStats) -> i64 {
    CLEAR_ITEMS_W[0] * i64::from(stats.refs_delta()) + CLEAR_ITEMS_W[1]
}

fn convert_gas(stats: &RunStats) -> i64 {
    let refs_delta = i64::from(stats.refs_delta());
    let length = i64::from(stats.length());
    match stats.item_type() {
        StackItemType::Any => CONVERT_ANY_W[0] * refs_delta + CONVERT_ANY_W[1],
        StackItemType::Array => {
            CONVERT_ARRAY_OR_STRUCT_W[0] * refs_delta
                + CONVERT_ARRAY_OR_STRUCT_W[1] * length
                + CONVERT_ARRAY_OR_STRUCT_W[2]
        }
        StackItemType::ByteString => {
            CONVERT_BYTE_ARRAY_OR_BUFFER_W[0] * length + CONVERT_BYTE_ARRAY_OR_BUFFER_W[1]
        }
        // C# throws for any other type. The CONVERT handler records one of the
        // three above, so this arm is unreachable; fall back to the `Any`
        // formula rather than aborting a node on a pricing path.
        other => {
            debug_assert!(
                false,
                "unsupported type {other:?} for CONVERT dynamic pricing"
            );
            CONVERT_ANY_W[0] * refs_delta + CONVERT_ANY_W[1]
        }
    }
}

fn drop_gas(stats: &RunStats) -> i64 {
    DROP_W[0] * i64::from(stats.refs_delta()) + DROP_W[1]
}

fn end_finally_gas(stats: &RunStats) -> i64 {
    END_FINALLY_W[0] * i64::from(stats.refs_delta()) + END_FINALLY_W[1]
}

fn has_key_gas(stats: &RunStats) -> i64 {
    HAS_KEY_W[0] * i64::from(stats.refs_delta()) + HAS_KEY_W[1]
}

fn init_slot_gas(stats: &RunStats) -> i64 {
    INIT_SLOT_W[0] * i64::from(stats.refs_delta())
        + INIT_SLOT_W[1] * i64::from(stats.length())
        + INIT_SLOT_W[2]
}

fn is_null_gas(stats: &RunStats) -> i64 {
    IS_NULL_W[0] * i64::from(stats.refs_delta()) + IS_NULL_W[1]
}

fn is_type_gas(stats: &RunStats) -> i64 {
    IS_TYPE_W[0] * i64::from(stats.refs_delta()) + IS_TYPE_W[1]
}

fn keys_gas(stats: &RunStats) -> i64 {
    KEYS_W[0] * i64::from(stats.refs_delta()) + KEYS_W[1] * i64::from(stats.length()) + KEYS_W[2]
}

fn memcpy_gas(stats: &RunStats) -> i64 {
    MEMCPY_W[0] * i64::from(stats.length()) + MEMCPY_W[1]
}

fn new_array_gas(stats: &RunStats) -> i64 {
    let length = i64::from(stats.length());
    if matches!(
        stats.item_type(),
        StackItemType::ByteString | StackItemType::Integer
    ) {
        NEW_ARRAY_BYTE_OR_INT_W[0] * length + NEW_ARRAY_BYTE_OR_INT_W[1]
    } else {
        NEW_ARRAY_ANY_W[0] * length + NEW_ARRAY_ANY_W[1]
    }
}

fn new_buffer_gas(stats: &RunStats) -> i64 {
    NEW_BUFFER_W[0] * i64::from(stats.length()) + NEW_BUFFER_W[1]
}

fn pack_gas(stats: &RunStats) -> i64 {
    PACK_W[0] * i64::from(stats.length()) + PACK_W[1]
}

fn pack_map_gas(stats: &RunStats) -> i64 {
    PACK_MAP_W[0] * i64::from(stats.refs_delta())
        + PACK_MAP_W[1] * i64::from(stats.length())
        + PACK_MAP_W[2]
}

fn pick_item_gas(stats: &RunStats) -> i64 {
    PICK_ITEM_W[0] * i64::from(stats.refs_delta()) + PICK_ITEM_W[1]
}

fn pop_item_gas(stats: &RunStats) -> i64 {
    POP_ITEM_W[0] * i64::from(stats.refs_delta()) + POP_ITEM_W[1]
}

fn remove_gas(stats: &RunStats) -> i64 {
    let refs_delta = i64::from(stats.refs_delta());
    let length = i64::from(stats.length());
    if stats.item_type() == StackItemType::Map {
        REMOVE_MAP_W[0] * refs_delta + REMOVE_MAP_W[1] * length + REMOVE_MAP_W[2]
    } else {
        REMOVE_ARRAY_OR_STRUCT_W[0] * refs_delta
            + REMOVE_ARRAY_OR_STRUCT_W[1] * length
            + REMOVE_ARRAY_OR_STRUCT_W[2]
    }
}

fn reverse_items_gas(stats: &RunStats) -> i64 {
    let length = i64::from(stats.length());
    if stats.item_type() == StackItemType::Buffer {
        REVERSE_ITEMS_BUF_W[0] * length + REVERSE_ITEMS_BUF_W[1]
    } else {
        REVERSE_ITEMS_ARR_W[0] * i64::from(stats.refs_delta())
            + REVERSE_ITEMS_ARR_W[1] * length
            + REVERSE_ITEMS_ARR_W[2]
    }
}

fn reverse_gas(stats: &RunStats) -> i64 {
    REVERSE_W[0] * i64::from(stats.length()) + REVERSE_W[1]
}

fn roll_gas(stats: &RunStats) -> i64 {
    ROLL_W[0] * i64::from(stats.length()) + ROLL_W[1]
}

fn set_item_gas(stats: &RunStats) -> i64 {
    SET_ITEM_W[0] * i64::from(stats.refs_delta())
        + SET_ITEM_W[1] * i64::from(stats.cloned_items())
        + SET_ITEM_W[2]
}

fn size_gas(stats: &RunStats) -> i64 {
    SIZE_W[0] * i64::from(stats.refs_delta()) + SIZE_W[1]
}

fn st_gas(stats: &RunStats) -> i64 {
    ST_W[0] * i64::from(stats.refs_delta()) + ST_W[1]
}

fn substr_gas(stats: &RunStats) -> i64 {
    SUBSTR_W[0] * i64::from(stats.length()) + SUBSTR_W[1]
}

fn throw_gas(stats: &RunStats) -> i64 {
    THROW_W[0] * i64::from(stats.refs_delta()) + THROW_W[1]
}

fn unpack_gas(stats: &RunStats) -> i64 {
    UNPACK_W[0] * i64::from(stats.length()) + UNPACK_W[1]
}

fn values_gas(stats: &RunStats) -> i64 {
    VALUES_W[0] * i64::from(stats.refs_delta())
        + VALUES_W[1] * i64::from(stats.length())
        + VALUES_W[2] * i64::from(stats.cloned_items())
        + VALUES_W[3]
}

fn x_drop_gas(stats: &RunStats) -> i64 {
    X_DROP_W[0] * i64::from(stats.refs_delta())
        + X_DROP_W[1] * i64::from(stats.length())
        + X_DROP_W[2]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_opcode_has_a_calculator() {
        // 60 opcodes are registered in C# v3.11.2 ComposeDynamicPriceTable.
        // STARG0..6, STLOC0..6 and STSFLD0..6 count individually.
        let registered = (0u8..=255)
            .filter(|opcode| DEFAULT_DYNAMIC_PRICE_TABLE.get(*opcode).is_some())
            .count();
        assert_eq!(registered, 66, "dynamic price table size diverges from C#");
    }

    #[test]
    fn static_coefficients_match_csharp_values() {
        assert_eq!(STATIC_COEFFICIENTS[OpCode::ADD.byte() as usize], 3153);
        assert_eq!(
            STATIC_COEFFICIENTS[OpCode::MODPOW.byte() as usize],
            1_648_443
        );
        assert_eq!(STATIC_COEFFICIENTS[OpCode::SQRT.byte() as usize], 33_802);
        assert_eq!(
            STATIC_COEFFICIENTS[OpCode::CALLT.byte() as usize],
            CONTRACT_CALL_PRICE
        );
        assert_eq!(STATIC_COEFFICIENTS[OpCode::PUSH1.byte() as usize], 2187);
        assert_eq!(STATIC_COEFFICIENTS[OpCode::NEWMAP.byte() as usize], 2720);
        // Opcodes without a coefficient are priced at zero.
        assert_eq!(STATIC_COEFFICIENTS[0xFE], 0);
    }

    #[test]
    fn calculators_follow_the_weight_formulas() {
        let mut stats = RunStats::new();
        stats.collect_ref_delta(3);
        stats.collect_op_length(7);
        stats.collect_cloned_items(1);

        // SetItem: 96 * refs + 149 * cloned + 2390
        assert_eq!(set_item_gas(&stats), 96 * 3 + 149 + 2390);
        // Append: 97 * refs + 192 * cloned + 2715
        assert_eq!(append_gas(&stats), 97 * 3 + 192 + 2715);
        // Cat: 8 * length + 2706
        assert_eq!(cat_gas(&stats), 8 * 7 + 2706);
        // Clear: 93 * refs + -15 * length + 1515
        assert_eq!(clear_gas(&stats), 93 * 3 - 15 * 7 + 1515);
        // Values: 86 * refs + 312 * length + 168 * cloned + 4758
        assert_eq!(values_gas(&stats), 86 * 3 + 312 * 7 + 168 + 4758);
    }

    #[test]
    fn type_sensitive_calculators_branch_on_the_collected_type() {
        let mut stats = RunStats::new();
        stats.collect_ref_delta(0);
        stats.collect_op_length(10);

        stats.collect_type(StackItemType::ByteString);
        assert_eq!(new_array_gas(&stats), 134 * 10 + 2718);
        assert_eq!(convert_gas(&stats), 7 * 10 + 3417);

        stats.collect_type(StackItemType::Array);
        assert_eq!(new_array_gas(&stats), 126 * 10 + 2718);
        assert_eq!(convert_gas(&stats), 77 * 0 + 134 * 10 + 3218);

        stats.collect_type(StackItemType::Map);
        assert_eq!(remove_gas(&stats), 96 * 0 + 706 * 10 + 6776);

        stats.collect_type(StackItemType::Buffer);
        assert_eq!(reverse_items_gas(&stats), 9 * 10 + 1690);
    }
}
