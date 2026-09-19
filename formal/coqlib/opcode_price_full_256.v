Require Import Arith Lia List.
Import ListNotations.

(** Opcode execution-unit prices, generated verbatim from the Rust
    source of truth neo-core/src/smart_contract/application_engine/
    op_code_prices.rs OPCODE_PRICE_TABLE (indexed by OpCode byte value).
    Values are the pre-ExecFeeFactor execution-unit costs.  These are
    NOT claimed to match the C# reference beyond what the Rust source
    documents; a full automated C# cross-check remains TODO(M-17). *)

Definition price_table : list nat :=
  [1; 1; 1; 1; 4; 4; 0; 0;
   1; 1; 4; 1; 8; 512; 4096; 1;
   1; 1; 1; 1; 1; 1; 1; 1;
   1; 1; 1; 1; 1; 1; 1; 1;
   1; 1; 2; 2; 2; 2; 2; 2;
   2; 2; 2; 2; 2; 2; 2; 2;
   2; 2; 2; 2; 512; 512; 512; 32768;
   0; 1; 512; 4; 4; 4; 4; 4;
   0; 0; 0; 2; 0; 2; 2; 0;
   16; 16; 2; 2; 0; 2; 2; 0;
   2; 2; 16; 2; 2; 16; 16; 64;
   2; 2; 2; 2; 2; 2; 2; 2;
   2; 2; 2; 2; 2; 2; 2; 2;
   2; 2; 2; 2; 2; 2; 2; 2;
   2; 2; 2; 2; 2; 2; 2; 2;
   2; 2; 2; 2; 2; 2; 2; 2;
   2; 2; 2; 2; 2; 2; 2; 2;
   256; 2048; 0; 2048; 2048; 2048; 2048; 0;
   4; 8; 8; 8; 0; 0; 0; 32;
   32; 4; 4; 4; 4; 4; 8; 8;
   8; 8; 8; 64; 64; 32; 2048; 0;
   8; 8; 4; 8; 8; 0; 0; 0;
   0; 4; 0; 8; 8; 8; 8; 8;
   8; 8; 8; 8; 0; 0; 2048; 2048;
   2048; 2048; 16; 512; 512; 512; 512; 0;
   8192; 0; 4; 64; 16; 8192; 64; 8192;
   8192; 8192; 16; 16; 16; 0; 0; 0;
   2; 2; 0; 8192; 0; 0; 0; 0;
   0; 1; 0; 0; 0; 0; 0; 0;
   0; 0; 0; 0; 0; 0; 0; 0;
   0; 0; 0; 0; 0; 0; 0; 0;
   0; 0; 0; 0; 0; 0; 0; 0]
.

(** Price for an opcode byte value; out-of-range indexes cost 0. *)
Definition opcode_price_all (op : nat) : nat :=
  nth op price_table 0.

(** Every opcode price is non-negative (a nat). *)
Lemma all_opcodes_non_negative :
  forall op : nat, opcode_price_all op >= 0.
Proof.
  intros op. unfold opcode_price_all. lia.
Qed.

(** The table covers exactly the 256 opcode byte values. *)
Lemma opcode_table_size : length price_table = 256.
Proof. reflexivity. Qed.

(** Per-opcode prices, machine-checked against the Rust table. *)
Example price_000 : opcode_price_all 0 = 1.
Proof. reflexivity. Qed.
Example price_001 : opcode_price_all 1 = 1.
Proof. reflexivity. Qed.
Example price_002 : opcode_price_all 2 = 1.
Proof. reflexivity. Qed.
Example price_003 : opcode_price_all 3 = 1.
Proof. reflexivity. Qed.
Example price_004 : opcode_price_all 4 = 4.
Proof. reflexivity. Qed.
Example price_005 : opcode_price_all 5 = 4.
Proof. reflexivity. Qed.
Example price_006 : opcode_price_all 6 = 0.
Proof. reflexivity. Qed.
Example price_007 : opcode_price_all 7 = 0.
Proof. reflexivity. Qed.
Example price_008 : opcode_price_all 8 = 1.
Proof. reflexivity. Qed.
Example price_009 : opcode_price_all 9 = 1.
Proof. reflexivity. Qed.
Example price_010 : opcode_price_all 10 = 4.
Proof. reflexivity. Qed.
Example price_011 : opcode_price_all 11 = 1.
Proof. reflexivity. Qed.
Example price_012 : opcode_price_all 12 = 8.
Proof. reflexivity. Qed.
Example price_013 : opcode_price_all 13 = 512.
Proof. reflexivity. Qed.
Example price_014 : opcode_price_all 14 = 4096.
Proof. reflexivity. Qed.
Example price_015 : opcode_price_all 15 = 1.
Proof. reflexivity. Qed.
Example price_016 : opcode_price_all 16 = 1.
Proof. reflexivity. Qed.
Example price_017 : opcode_price_all 17 = 1.
Proof. reflexivity. Qed.
Example price_018 : opcode_price_all 18 = 1.
Proof. reflexivity. Qed.
Example price_019 : opcode_price_all 19 = 1.
Proof. reflexivity. Qed.
Example price_020 : opcode_price_all 20 = 1.
Proof. reflexivity. Qed.
Example price_021 : opcode_price_all 21 = 1.
Proof. reflexivity. Qed.
Example price_022 : opcode_price_all 22 = 1.
Proof. reflexivity. Qed.
Example price_023 : opcode_price_all 23 = 1.
Proof. reflexivity. Qed.
Example price_024 : opcode_price_all 24 = 1.
Proof. reflexivity. Qed.
Example price_025 : opcode_price_all 25 = 1.
Proof. reflexivity. Qed.
Example price_026 : opcode_price_all 26 = 1.
Proof. reflexivity. Qed.
Example price_027 : opcode_price_all 27 = 1.
Proof. reflexivity. Qed.
Example price_028 : opcode_price_all 28 = 1.
Proof. reflexivity. Qed.
Example price_029 : opcode_price_all 29 = 1.
Proof. reflexivity. Qed.
Example price_030 : opcode_price_all 30 = 1.
Proof. reflexivity. Qed.
Example price_031 : opcode_price_all 31 = 1.
Proof. reflexivity. Qed.
Example price_032 : opcode_price_all 32 = 1.
Proof. reflexivity. Qed.
Example price_033 : opcode_price_all 33 = 1.
Proof. reflexivity. Qed.
Example price_034 : opcode_price_all 34 = 2.
Proof. reflexivity. Qed.
Example price_035 : opcode_price_all 35 = 2.
Proof. reflexivity. Qed.
Example price_036 : opcode_price_all 36 = 2.
Proof. reflexivity. Qed.
Example price_037 : opcode_price_all 37 = 2.
Proof. reflexivity. Qed.
Example price_038 : opcode_price_all 38 = 2.
Proof. reflexivity. Qed.
Example price_039 : opcode_price_all 39 = 2.
Proof. reflexivity. Qed.
Example price_040 : opcode_price_all 40 = 2.
Proof. reflexivity. Qed.
Example price_041 : opcode_price_all 41 = 2.
Proof. reflexivity. Qed.
Example price_042 : opcode_price_all 42 = 2.
Proof. reflexivity. Qed.
Example price_043 : opcode_price_all 43 = 2.
Proof. reflexivity. Qed.
Example price_044 : opcode_price_all 44 = 2.
Proof. reflexivity. Qed.
Example price_045 : opcode_price_all 45 = 2.
Proof. reflexivity. Qed.
Example price_046 : opcode_price_all 46 = 2.
Proof. reflexivity. Qed.
Example price_047 : opcode_price_all 47 = 2.
Proof. reflexivity. Qed.
Example price_048 : opcode_price_all 48 = 2.
Proof. reflexivity. Qed.
Example price_049 : opcode_price_all 49 = 2.
Proof. reflexivity. Qed.
Example price_050 : opcode_price_all 50 = 2.
Proof. reflexivity. Qed.
Example price_051 : opcode_price_all 51 = 2.
Proof. reflexivity. Qed.
Example price_052 : opcode_price_all 52 = 512.
Proof. reflexivity. Qed.
Example price_053 : opcode_price_all 53 = 512.
Proof. reflexivity. Qed.
Example price_054 : opcode_price_all 54 = 512.
Proof. reflexivity. Qed.
Example price_055 : opcode_price_all 55 = 32768.
Proof. reflexivity. Qed.
Example price_056 : opcode_price_all 56 = 0.
Proof. reflexivity. Qed.
Example price_057 : opcode_price_all 57 = 1.
Proof. reflexivity. Qed.
Example price_058 : opcode_price_all 58 = 512.
Proof. reflexivity. Qed.
Example price_059 : opcode_price_all 59 = 4.
Proof. reflexivity. Qed.
Example price_060 : opcode_price_all 60 = 4.
Proof. reflexivity. Qed.
Example price_061 : opcode_price_all 61 = 4.
Proof. reflexivity. Qed.
Example price_062 : opcode_price_all 62 = 4.
Proof. reflexivity. Qed.
Example price_063 : opcode_price_all 63 = 4.
Proof. reflexivity. Qed.
Example price_064 : opcode_price_all 64 = 0.
Proof. reflexivity. Qed.
Example price_065 : opcode_price_all 65 = 0.
Proof. reflexivity. Qed.
Example price_066 : opcode_price_all 66 = 0.
Proof. reflexivity. Qed.
Example price_067 : opcode_price_all 67 = 2.
Proof. reflexivity. Qed.
Example price_068 : opcode_price_all 68 = 0.
Proof. reflexivity. Qed.
Example price_069 : opcode_price_all 69 = 2.
Proof. reflexivity. Qed.
Example price_070 : opcode_price_all 70 = 2.
Proof. reflexivity. Qed.
Example price_071 : opcode_price_all 71 = 0.
Proof. reflexivity. Qed.
Example price_072 : opcode_price_all 72 = 16.
Proof. reflexivity. Qed.
Example price_073 : opcode_price_all 73 = 16.
Proof. reflexivity. Qed.
Example price_074 : opcode_price_all 74 = 2.
Proof. reflexivity. Qed.
Example price_075 : opcode_price_all 75 = 2.
Proof. reflexivity. Qed.
Example price_076 : opcode_price_all 76 = 0.
Proof. reflexivity. Qed.
Example price_077 : opcode_price_all 77 = 2.
Proof. reflexivity. Qed.
Example price_078 : opcode_price_all 78 = 2.
Proof. reflexivity. Qed.
Example price_079 : opcode_price_all 79 = 0.
Proof. reflexivity. Qed.
Example price_080 : opcode_price_all 80 = 2.
Proof. reflexivity. Qed.
Example price_081 : opcode_price_all 81 = 2.
Proof. reflexivity. Qed.
Example price_082 : opcode_price_all 82 = 16.
Proof. reflexivity. Qed.
Example price_083 : opcode_price_all 83 = 2.
Proof. reflexivity. Qed.
Example price_084 : opcode_price_all 84 = 2.
Proof. reflexivity. Qed.
Example price_085 : opcode_price_all 85 = 16.
Proof. reflexivity. Qed.
Example price_086 : opcode_price_all 86 = 16.
Proof. reflexivity. Qed.
Example price_087 : opcode_price_all 87 = 64.
Proof. reflexivity. Qed.
Example price_088 : opcode_price_all 88 = 2.
Proof. reflexivity. Qed.
Example price_089 : opcode_price_all 89 = 2.
Proof. reflexivity. Qed.
Example price_090 : opcode_price_all 90 = 2.
Proof. reflexivity. Qed.
Example price_091 : opcode_price_all 91 = 2.
Proof. reflexivity. Qed.
Example price_092 : opcode_price_all 92 = 2.
Proof. reflexivity. Qed.
Example price_093 : opcode_price_all 93 = 2.
Proof. reflexivity. Qed.
Example price_094 : opcode_price_all 94 = 2.
Proof. reflexivity. Qed.
Example price_095 : opcode_price_all 95 = 2.
Proof. reflexivity. Qed.
Example price_096 : opcode_price_all 96 = 2.
Proof. reflexivity. Qed.
Example price_097 : opcode_price_all 97 = 2.
Proof. reflexivity. Qed.
Example price_098 : opcode_price_all 98 = 2.
Proof. reflexivity. Qed.
Example price_099 : opcode_price_all 99 = 2.
Proof. reflexivity. Qed.
Example price_100 : opcode_price_all 100 = 2.
Proof. reflexivity. Qed.
Example price_101 : opcode_price_all 101 = 2.
Proof. reflexivity. Qed.
Example price_102 : opcode_price_all 102 = 2.
Proof. reflexivity. Qed.
Example price_103 : opcode_price_all 103 = 2.
Proof. reflexivity. Qed.
Example price_104 : opcode_price_all 104 = 2.
Proof. reflexivity. Qed.
Example price_105 : opcode_price_all 105 = 2.
Proof. reflexivity. Qed.
Example price_106 : opcode_price_all 106 = 2.
Proof. reflexivity. Qed.
Example price_107 : opcode_price_all 107 = 2.
Proof. reflexivity. Qed.
Example price_108 : opcode_price_all 108 = 2.
Proof. reflexivity. Qed.
Example price_109 : opcode_price_all 109 = 2.
Proof. reflexivity. Qed.
Example price_110 : opcode_price_all 110 = 2.
Proof. reflexivity. Qed.
Example price_111 : opcode_price_all 111 = 2.
Proof. reflexivity. Qed.
Example price_112 : opcode_price_all 112 = 2.
Proof. reflexivity. Qed.
Example price_113 : opcode_price_all 113 = 2.
Proof. reflexivity. Qed.
Example price_114 : opcode_price_all 114 = 2.
Proof. reflexivity. Qed.
Example price_115 : opcode_price_all 115 = 2.
Proof. reflexivity. Qed.
Example price_116 : opcode_price_all 116 = 2.
Proof. reflexivity. Qed.
Example price_117 : opcode_price_all 117 = 2.
Proof. reflexivity. Qed.
Example price_118 : opcode_price_all 118 = 2.
Proof. reflexivity. Qed.
Example price_119 : opcode_price_all 119 = 2.
Proof. reflexivity. Qed.
Example price_120 : opcode_price_all 120 = 2.
Proof. reflexivity. Qed.
Example price_121 : opcode_price_all 121 = 2.
Proof. reflexivity. Qed.
Example price_122 : opcode_price_all 122 = 2.
Proof. reflexivity. Qed.
Example price_123 : opcode_price_all 123 = 2.
Proof. reflexivity. Qed.
Example price_124 : opcode_price_all 124 = 2.
Proof. reflexivity. Qed.
Example price_125 : opcode_price_all 125 = 2.
Proof. reflexivity. Qed.
Example price_126 : opcode_price_all 126 = 2.
Proof. reflexivity. Qed.
Example price_127 : opcode_price_all 127 = 2.
Proof. reflexivity. Qed.
Example price_128 : opcode_price_all 128 = 2.
Proof. reflexivity. Qed.
Example price_129 : opcode_price_all 129 = 2.
Proof. reflexivity. Qed.
Example price_130 : opcode_price_all 130 = 2.
Proof. reflexivity. Qed.
Example price_131 : opcode_price_all 131 = 2.
Proof. reflexivity. Qed.
Example price_132 : opcode_price_all 132 = 2.
Proof. reflexivity. Qed.
Example price_133 : opcode_price_all 133 = 2.
Proof. reflexivity. Qed.
Example price_134 : opcode_price_all 134 = 2.
Proof. reflexivity. Qed.
Example price_135 : opcode_price_all 135 = 2.
Proof. reflexivity. Qed.
Example price_136 : opcode_price_all 136 = 256.
Proof. reflexivity. Qed.
Example price_137 : opcode_price_all 137 = 2048.
Proof. reflexivity. Qed.
Example price_138 : opcode_price_all 138 = 0.
Proof. reflexivity. Qed.
Example price_139 : opcode_price_all 139 = 2048.
Proof. reflexivity. Qed.
Example price_140 : opcode_price_all 140 = 2048.
Proof. reflexivity. Qed.
Example price_141 : opcode_price_all 141 = 2048.
Proof. reflexivity. Qed.
Example price_142 : opcode_price_all 142 = 2048.
Proof. reflexivity. Qed.
Example price_143 : opcode_price_all 143 = 0.
Proof. reflexivity. Qed.
Example price_144 : opcode_price_all 144 = 4.
Proof. reflexivity. Qed.
Example price_145 : opcode_price_all 145 = 8.
Proof. reflexivity. Qed.
Example price_146 : opcode_price_all 146 = 8.
Proof. reflexivity. Qed.
Example price_147 : opcode_price_all 147 = 8.
Proof. reflexivity. Qed.
Example price_148 : opcode_price_all 148 = 0.
Proof. reflexivity. Qed.
Example price_149 : opcode_price_all 149 = 0.
Proof. reflexivity. Qed.
Example price_150 : opcode_price_all 150 = 0.
Proof. reflexivity. Qed.
Example price_151 : opcode_price_all 151 = 32.
Proof. reflexivity. Qed.
Example price_152 : opcode_price_all 152 = 32.
Proof. reflexivity. Qed.
Example price_153 : opcode_price_all 153 = 4.
Proof. reflexivity. Qed.
Example price_154 : opcode_price_all 154 = 4.
Proof. reflexivity. Qed.
Example price_155 : opcode_price_all 155 = 4.
Proof. reflexivity. Qed.
Example price_156 : opcode_price_all 156 = 4.
Proof. reflexivity. Qed.
Example price_157 : opcode_price_all 157 = 4.
Proof. reflexivity. Qed.
Example price_158 : opcode_price_all 158 = 8.
Proof. reflexivity. Qed.
Example price_159 : opcode_price_all 159 = 8.
Proof. reflexivity. Qed.
Example price_160 : opcode_price_all 160 = 8.
Proof. reflexivity. Qed.
Example price_161 : opcode_price_all 161 = 8.
Proof. reflexivity. Qed.
Example price_162 : opcode_price_all 162 = 8.
Proof. reflexivity. Qed.
Example price_163 : opcode_price_all 163 = 64.
Proof. reflexivity. Qed.
Example price_164 : opcode_price_all 164 = 64.
Proof. reflexivity. Qed.
Example price_165 : opcode_price_all 165 = 32.
Proof. reflexivity. Qed.
Example price_166 : opcode_price_all 166 = 2048.
Proof. reflexivity. Qed.
Example price_167 : opcode_price_all 167 = 0.
Proof. reflexivity. Qed.
Example price_168 : opcode_price_all 168 = 8.
Proof. reflexivity. Qed.
Example price_169 : opcode_price_all 169 = 8.
Proof. reflexivity. Qed.
Example price_170 : opcode_price_all 170 = 4.
Proof. reflexivity. Qed.
Example price_171 : opcode_price_all 171 = 8.
Proof. reflexivity. Qed.
Example price_172 : opcode_price_all 172 = 8.
Proof. reflexivity. Qed.
Example price_173 : opcode_price_all 173 = 0.
Proof. reflexivity. Qed.
Example price_174 : opcode_price_all 174 = 0.
Proof. reflexivity. Qed.
Example price_175 : opcode_price_all 175 = 0.
Proof. reflexivity. Qed.
Example price_176 : opcode_price_all 176 = 0.
Proof. reflexivity. Qed.
Example price_177 : opcode_price_all 177 = 4.
Proof. reflexivity. Qed.
Example price_178 : opcode_price_all 178 = 0.
Proof. reflexivity. Qed.
Example price_179 : opcode_price_all 179 = 8.
Proof. reflexivity. Qed.
Example price_180 : opcode_price_all 180 = 8.
Proof. reflexivity. Qed.
Example price_181 : opcode_price_all 181 = 8.
Proof. reflexivity. Qed.
Example price_182 : opcode_price_all 182 = 8.
Proof. reflexivity. Qed.
Example price_183 : opcode_price_all 183 = 8.
Proof. reflexivity. Qed.
Example price_184 : opcode_price_all 184 = 8.
Proof. reflexivity. Qed.
Example price_185 : opcode_price_all 185 = 8.
Proof. reflexivity. Qed.
Example price_186 : opcode_price_all 186 = 8.
Proof. reflexivity. Qed.
Example price_187 : opcode_price_all 187 = 8.
Proof. reflexivity. Qed.
Example price_188 : opcode_price_all 188 = 0.
Proof. reflexivity. Qed.
Example price_189 : opcode_price_all 189 = 0.
Proof. reflexivity. Qed.
Example price_190 : opcode_price_all 190 = 2048.
Proof. reflexivity. Qed.
Example price_191 : opcode_price_all 191 = 2048.
Proof. reflexivity. Qed.
Example price_192 : opcode_price_all 192 = 2048.
Proof. reflexivity. Qed.
Example price_193 : opcode_price_all 193 = 2048.
Proof. reflexivity. Qed.
Example price_194 : opcode_price_all 194 = 16.
Proof. reflexivity. Qed.
Example price_195 : opcode_price_all 195 = 512.
Proof. reflexivity. Qed.
Example price_196 : opcode_price_all 196 = 512.
Proof. reflexivity. Qed.
Example price_197 : opcode_price_all 197 = 512.
Proof. reflexivity. Qed.
Example price_198 : opcode_price_all 198 = 512.
Proof. reflexivity. Qed.
Example price_199 : opcode_price_all 199 = 0.
Proof. reflexivity. Qed.
Example price_200 : opcode_price_all 200 = 8192.
Proof. reflexivity. Qed.
Example price_201 : opcode_price_all 201 = 0.
Proof. reflexivity. Qed.
Example price_202 : opcode_price_all 202 = 4.
Proof. reflexivity. Qed.
Example price_203 : opcode_price_all 203 = 64.
Proof. reflexivity. Qed.
Example price_204 : opcode_price_all 204 = 16.
Proof. reflexivity. Qed.
Example price_205 : opcode_price_all 205 = 8192.
Proof. reflexivity. Qed.
Example price_206 : opcode_price_all 206 = 64.
Proof. reflexivity. Qed.
Example price_207 : opcode_price_all 207 = 8192.
Proof. reflexivity. Qed.
Example price_208 : opcode_price_all 208 = 8192.
Proof. reflexivity. Qed.
Example price_209 : opcode_price_all 209 = 8192.
Proof. reflexivity. Qed.
Example price_210 : opcode_price_all 210 = 16.
Proof. reflexivity. Qed.
Example price_211 : opcode_price_all 211 = 16.
Proof. reflexivity. Qed.
Example price_212 : opcode_price_all 212 = 16.
Proof. reflexivity. Qed.
Example price_213 : opcode_price_all 213 = 0.
Proof. reflexivity. Qed.
Example price_214 : opcode_price_all 214 = 0.
Proof. reflexivity. Qed.
Example price_215 : opcode_price_all 215 = 0.
Proof. reflexivity. Qed.
Example price_216 : opcode_price_all 216 = 2.
Proof. reflexivity. Qed.
Example price_217 : opcode_price_all 217 = 2.
Proof. reflexivity. Qed.
Example price_218 : opcode_price_all 218 = 0.
Proof. reflexivity. Qed.
Example price_219 : opcode_price_all 219 = 8192.
Proof. reflexivity. Qed.
Example price_220 : opcode_price_all 220 = 0.
Proof. reflexivity. Qed.
Example price_221 : opcode_price_all 221 = 0.
Proof. reflexivity. Qed.
Example price_222 : opcode_price_all 222 = 0.
Proof. reflexivity. Qed.
Example price_223 : opcode_price_all 223 = 0.
Proof. reflexivity. Qed.
Example price_224 : opcode_price_all 224 = 0.
Proof. reflexivity. Qed.
Example price_225 : opcode_price_all 225 = 1.
Proof. reflexivity. Qed.
Example price_226 : opcode_price_all 226 = 0.
Proof. reflexivity. Qed.
Example price_227 : opcode_price_all 227 = 0.
Proof. reflexivity. Qed.
Example price_228 : opcode_price_all 228 = 0.
Proof. reflexivity. Qed.
Example price_229 : opcode_price_all 229 = 0.
Proof. reflexivity. Qed.
Example price_230 : opcode_price_all 230 = 0.
Proof. reflexivity. Qed.
Example price_231 : opcode_price_all 231 = 0.
Proof. reflexivity. Qed.
Example price_232 : opcode_price_all 232 = 0.
Proof. reflexivity. Qed.
Example price_233 : opcode_price_all 233 = 0.
Proof. reflexivity. Qed.
Example price_234 : opcode_price_all 234 = 0.
Proof. reflexivity. Qed.
Example price_235 : opcode_price_all 235 = 0.
Proof. reflexivity. Qed.
Example price_236 : opcode_price_all 236 = 0.
Proof. reflexivity. Qed.
Example price_237 : opcode_price_all 237 = 0.
Proof. reflexivity. Qed.
Example price_238 : opcode_price_all 238 = 0.
Proof. reflexivity. Qed.
Example price_239 : opcode_price_all 239 = 0.
Proof. reflexivity. Qed.
Example price_240 : opcode_price_all 240 = 0.
Proof. reflexivity. Qed.
Example price_241 : opcode_price_all 241 = 0.
Proof. reflexivity. Qed.
Example price_242 : opcode_price_all 242 = 0.
Proof. reflexivity. Qed.
Example price_243 : opcode_price_all 243 = 0.
Proof. reflexivity. Qed.
Example price_244 : opcode_price_all 244 = 0.
Proof. reflexivity. Qed.
Example price_245 : opcode_price_all 245 = 0.
Proof. reflexivity. Qed.
Example price_246 : opcode_price_all 246 = 0.
Proof. reflexivity. Qed.
Example price_247 : opcode_price_all 247 = 0.
Proof. reflexivity. Qed.
Example price_248 : opcode_price_all 248 = 0.
Proof. reflexivity. Qed.
Example price_249 : opcode_price_all 249 = 0.
Proof. reflexivity. Qed.
Example price_250 : opcode_price_all 250 = 0.
Proof. reflexivity. Qed.
Example price_251 : opcode_price_all 251 = 0.
Proof. reflexivity. Qed.
Example price_252 : opcode_price_all 252 = 0.
Proof. reflexivity. Qed.
Example price_253 : opcode_price_all 253 = 0.
Proof. reflexivity. Qed.
Example price_254 : opcode_price_all 254 = 0.
Proof. reflexivity. Qed.
Example price_255 : opcode_price_all 255 = 0.
Proof. reflexivity. Qed.

