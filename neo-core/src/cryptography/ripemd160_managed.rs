use std::convert::TryInto;

pub struct RIPEMD160Managed {
    buffer: [u8; 64],
    count: u64,
    state_md160: [u32; 5],
    block_dwords: [u32; 16],
}

impl RIPEMD160Managed {
    pub fn new() -> Self {
        let mut ripemd = RIPEMD160Managed {
            buffer: [0; 64],
            count: 0,
            state_md160: [0; 5],
            block_dwords: [0; 16],
        };
        ripemd.initialize();
        ripemd
    }

    pub fn initialize(&mut self) {
        self.count = 0;
        self.state_md160[0] = 0x67452301;
        self.state_md160[1] = 0xefcdab89;
        self.state_md160[2] = 0x98badcfe;
        self.state_md160[3] = 0x10325476;
        self.state_md160[4] = 0xc3d2e1f0;
        self.block_dwords = [0; 16];
        self.buffer = [0; 64];
    }

    pub fn hash_core(&mut self, input: &[u8]) {
        let mut buffer_len = (self.count & 0x3f) as usize;
        self.count += input.len() as u64;

        if buffer_len > 0 && buffer_len + input.len() >= 64 {
            self.buffer[buffer_len..64].copy_from_slice(&input[..64 - buffer_len]);
            self.md_transform(&self.buffer);
            buffer_len = 0;
        }

        let mut input_index = 64 - buffer_len;
        while input_index + 63 < input.len() {
            self.md_transform(&input[input_index..input_index + 64]);
            input_index += 64;
        }

        if input_index < input.len() {
            self.buffer[buffer_len..buffer_len + input.len() - input_index]
                .copy_from_slice(&input[input_index..]);
        }
    }

    pub fn hash_final(&mut self) -> [u8; 20] {
        let mut pad_len = 64 - (self.count as usize & 0x3f);
        if pad_len <= 8 {
            pad_len += 64;
        }

        let mut pad = vec![0u8; pad_len];
        pad[0] = 0x80;

        let bit_count = self.count * 8;
        pad[pad_len - 8..].copy_from_slice(&bit_count.to_le_bytes());

        self.hash_core(&pad);

        let mut hash = [0u8; 20];
        for (i, &state) in self.state_md160.iter().enumerate() {
            hash[i * 4..(i + 1) * 4].copy_from_slice(&state.to_le_bytes());
        }

        hash
    }

    fn md_transform(&mut self, block: &[u8]) {
        let mut aa = self.state_md160[0];
        let mut bb = self.state_md160[1];
        let mut cc = self.state_md160[2];
        let mut dd = self.state_md160[3];
        let mut ee = self.state_md160[4];

        let mut aaa = aa;
        let mut bbb = bb;
        let mut ccc = cc;
        let mut ddd = dd;
        let mut eee = ee;

        for i in 0..16 {
            self.block_dwords[i] = u32::from_le_bytes(block[i * 4..(i + 1) * 4].try_into().unwrap());
        }

        // Left Round 1
        aa = self.round1(aa, bb, cc, dd, ee, self.block_dwords[0], 11);
        ee = self.round1(ee, aa, bb, cc, dd, self.block_dwords[1], 14);
        dd = self.round1(dd, ee, aa, bb, cc, self.block_dwords[2], 15);
        cc = self.round1(cc, dd, ee, aa, bb, self.block_dwords[3], 12);
        bb = self.round1(bb, cc, dd, ee, aa, self.block_dwords[4], 5);
        aa = self.round1(aa, bb, cc, dd, ee, self.block_dwords[5], 8);
        ee = self.round1(ee, aa, bb, cc, dd, self.block_dwords[6], 7);
        dd = self.round1(dd, ee, aa, bb, cc, self.block_dwords[7], 9);
        cc = self.round1(cc, dd, ee, aa, bb, self.block_dwords[8], 11);
        bb = self.round1(bb, cc, dd, ee, aa, self.block_dwords[9], 13);
        aa = self.round1(aa, bb, cc, dd, ee, self.block_dwords[10], 14);
        ee = self.round1(ee, aa, bb, cc, dd, self.block_dwords[11], 15);
        dd = self.round1(dd, ee, aa, bb, cc, self.block_dwords[12], 6);
        cc = self.round1(cc, dd, ee, aa, bb, self.block_dwords[13], 7);
        bb = self.round1(bb, cc, dd, ee, aa, self.block_dwords[14], 9);
        aa = self.round1(aa, bb, cc, dd, ee, self.block_dwords[15], 8);

        // Left Round 2
        ee = self.round2(ee, aa, bb, cc, dd, self.block_dwords[7], 7);
        dd = self.round2(dd, ee, aa, bb, cc, self.block_dwords[4], 6);
        cc = self.round2(cc, dd, ee, aa, bb, self.block_dwords[13], 8);
        bb = self.round2(bb, cc, dd, ee, aa, self.block_dwords[1], 13);
        aa = self.round2(aa, bb, cc, dd, ee, self.block_dwords[10], 11);
        ee = self.round2(ee, aa, bb, cc, dd, self.block_dwords[6], 9);
        dd = self.round2(dd, ee, aa, bb, cc, self.block_dwords[15], 7);
        cc = self.round2(cc, dd, ee, aa, bb, self.block_dwords[3], 15);
        bb = self.round2(bb, cc, dd, ee, aa, self.block_dwords[12], 7);
        aa = self.round2(aa, bb, cc, dd, ee, self.block_dwords[0], 12);
        ee = self.round2(ee, aa, bb, cc, dd, self.block_dwords[9], 15);
        dd = self.round2(dd, ee, aa, bb, cc, self.block_dwords[5], 9);
        cc = self.round2(cc, dd, ee, aa, bb, self.block_dwords[2], 11);
        bb = self.round2(bb, cc, dd, ee, aa, self.block_dwords[14], 7);
        aa = self.round2(aa, bb, cc, dd, ee, self.block_dwords[11], 13);
        ee = self.round2(ee, aa, bb, cc, dd, self.block_dwords[8], 12);

        // Left Round 3
        dd = self.round3(dd, ee, aa, bb, cc, self.block_dwords[3], 11);
        cc = self.round3(cc, dd, ee, aa, bb, self.block_dwords[10], 13);
        bb = self.round3(bb, cc, dd, ee, aa, self.block_dwords[14], 6);
        aa = self.round3(aa, bb, cc, dd, ee, self.block_dwords[4], 7);
        ee = self.round3(ee, aa, bb, cc, dd, self.block_dwords[9], 14);
        dd = self.round3(dd, ee, aa, bb, cc, self.block_dwords[15], 9);
        cc = self.round3(cc, dd, ee, aa, bb, self.block_dwords[8], 13);
        bb = self.round3(bb, cc, dd, ee, aa, self.block_dwords[1], 15);
        aa = self.round3(aa, bb, cc, dd, ee, self.block_dwords[2], 14);
        ee = self.round3(ee, aa, bb, cc, dd, self.block_dwords[7], 8);
        dd = self.round3(dd, ee, aa, bb, cc, self.block_dwords[0], 13);
        cc = self.round3(cc, dd, ee, aa, bb, self.block_dwords[6], 6);
        bb = self.round3(bb, cc, dd, ee, aa, self.block_dwords[13], 5);
        aa = self.round3(aa, bb, cc, dd, ee, self.block_dwords[11], 12);
        ee = self.round3(ee, aa, bb, cc, dd, self.block_dwords[5], 7);
        dd = self.round3(dd, ee, aa, bb, cc, self.block_dwords[12], 5);

        // Left Round 4
        cc = self.round4(cc, dd, ee, aa, bb, self.block_dwords[1], 11);
        bb = self.round4(bb, cc, dd, ee, aa, self.block_dwords[9], 12);
        aa = self.round4(aa, bb, cc, dd, ee, self.block_dwords[11], 14);
        ee = self.round4(ee, aa, bb, cc, dd, self.block_dwords[10], 15);
        dd = self.round4(dd, ee, aa, bb, cc, self.block_dwords[0], 14);
        cc = self.round4(cc, dd, ee, aa, bb, self.block_dwords[8], 15);
        bb = self.round4(bb, cc, dd, ee, aa, self.block_dwords[12], 9);
        aa = self.round4(aa, bb, cc, dd, ee, self.block_dwords[4], 8);
        ee = self.round4(ee, aa, bb, cc, dd, self.block_dwords[13], 9);
        dd = self.round4(dd, ee, aa, bb, cc, self.block_dwords[3], 14);
        cc = self.round4(cc, dd, ee, aa, bb, self.block_dwords[7], 5);
        bb = self.round4(bb, cc, dd, ee, aa, self.block_dwords[15], 6);
        aa = self.round4(aa, bb, cc, dd, ee, self.block_dwords[14], 8);
        ee = self.round4(ee, aa, bb, cc, dd, self.block_dwords[5], 6);
        dd = self.round4(dd, ee, aa, bb, cc, self.block_dwords[6], 5);
        cc = self.round4(cc, dd, ee, aa, bb, self.block_dwords[2], 12);

        // Left Round 5
        bb = self.round5(bb, cc, dd, ee, aa, self.block_dwords[4], 9);
        aa = self.round5(aa, bb, cc, dd, ee, self.block_dwords[0], 15);
        ee = self.round5(ee, aa, bb, cc, dd, self.block_dwords[5], 5);
        dd = self.round5(dd, ee, aa, bb, cc, self.block_dwords[9], 11);
        cc = self.round5(cc, dd, ee, aa, bb, self.block_dwords[7], 6);
        bb = self.round5(bb, cc, dd, ee, aa, self.block_dwords[12], 8);
        aa = self.round5(aa, bb, cc, dd, ee, self.block_dwords[2], 13);
        ee = self.round5(ee, aa, bb, cc, dd, self.block_dwords[10], 12);
        dd = self.round5(dd, ee, aa, bb, cc, self.block_dwords[14], 5);
        cc = self.round5(cc, dd, ee, aa, bb, self.block_dwords[1], 12);
        bb = self.round5(bb, cc, dd, ee, aa, self.block_dwords[3], 13);
        aa = self.round5(aa, bb, cc, dd, ee, self.block_dwords[8], 14);
        ee = self.round5(ee, aa, bb, cc, dd, self.block_dwords[11], 11);
        dd = self.round5(dd, ee, aa, bb, cc, self.block_dwords[6], 8);
        cc = self.round5(cc, dd, ee, aa, bb, self.block_dwords[15], 5);
        bb = self.round5(bb, cc, dd, ee, aa, self.block_dwords[13], 6);

        // Parallel Right Round 1
        aaa = self.par_round1(aaa, bbb, ccc, ddd, eee, self.block_dwords[5], 8);
        eee = self.par_round1(eee, aaa, bbb, ccc, ddd, self.block_dwords[14], 9);
        ddd = self.par_round1(ddd, eee, aaa, bbb, ccc, self.block_dwords[7], 9);
        ccc = self.par_round1(ccc, ddd, eee, aaa, bbb, self.block_dwords[0], 11);
        bbb = self.par_round1(bbb, ccc, ddd, eee, aaa, self.block_dwords[9], 13);
        aaa = self.par_round1(aaa, bbb, ccc, ddd, eee, self.block_dwords[2], 15);
        eee = self.par_round1(eee, aaa, bbb, ccc, ddd, self.block_dwords[11], 15);
        ddd = self.par_round1(ddd, eee, aaa, bbb, ccc, self.block_dwords[4], 5);
        ccc = self.par_round1(ccc, ddd, eee, aaa, bbb, self.block_dwords[13], 7);
        bbb = self.par_round1(bbb, ccc, ddd, eee, aaa, self.block_dwords[6], 7);
        aaa = self.par_round1(aaa, bbb, ccc, ddd, eee, self.block_dwords[15], 8);
        eee = self.par_round1(eee, aaa, bbb, ccc, ddd, self.block_dwords[8], 11);
        ddd = self.par_round1(ddd, eee, aaa, bbb, ccc, self.block_dwords[1], 14);
        ccc = self.par_round1(ccc, ddd, eee, aaa, bbb, self.block_dwords[10], 14);
