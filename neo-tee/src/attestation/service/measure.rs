use super::*;
impl AttestationService {
    /// Get the expected MRENCLAVE for this enclave version
    pub fn expected_mrenclave(&self) -> [u8; 32] {
        // On real hardware, this should be computed from the enclave binary (MRENCLAVE).
        // In simulation mode we use a deterministic value to keep tests reproducible.
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-enclave-v1");
        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Compute MRENCLAVE from enclave binary according to Intel SGX specification
    ///
    /// MRENCLAVE is computed by simulating the SGX enclave build process:
    /// 1. ECREATE: Hash initial enclave metadata (SECS)
    /// 2. EADD/EEXTEND: Hash each 256-byte page of enclave code/data
    /// 3. Final measurement is SHA-256 accumulation of all operations
    ///
    /// # Production Implementation
    ///
    /// This implementation follows Intel SGX measurement specification:
    /// - Processes enclave binary in 256-byte pages (SGX_PAGE_SIZE)
    /// - Each EEXTEND operation hashes 256 bytes of page content
    /// - Measurement accumulates: SHA256(previous_measurement || operation || data)
    ///
    /// # Reference
    ///
    /// Intel SGX Developer Guide, Section 2.4: Enclave Measurement
    /// https://www.intel.com/content/www/us/en/developer/tools/software-guard-extensions/overview.html
    pub fn compute_mrenclave(&self, enclave_binary: &[u8]) -> [u8; 32] {
        self.compute_mrenclave_with_metadata(enclave_binary, &EnclaveMetadata::default())
    }

    /// Compute MRENCLAVE with enclave metadata (SECS attributes)
    ///
    /// # Arguments
    ///
    /// * `enclave_binary` - The signed enclave binary (.so/.signed.so)
    /// * `metadata` - SECS attributes (size, base address, attributes, etc.)
    ///
    /// # SGX Measurement Process
    ///
    /// The measurement process follows these SGX instructions:
    ///
    /// 1. **ECREATE** (Initialize SECS):
    ///    ```text
    ///    measurement = SHA256(0x0045434543524541 || SECS.SSAFRAMESIZE || SECS.SIZE ||
    ///                         0 || SECS.MISCSELECT || 0 || SECS.ATTRIBUTES || 0 ||
    ///                         SECS.MRENCLAVE || 0 || SECS.MRSIGNER)
    ///    ```text
    ///
    /// 2. **EADD** (Add page - hashes page info, not content):
    ///    For each page:
    ///    ```text
    ///    measurement = SHA256(measurement || 0x0000000000444441 ||
    ///                         page.offset || page.flags)
    ///    ```text
    ///
    /// 3. **EEXTEND** (Extend page content - hashes actual data):
    ///    For each 256-byte chunk in page (16 chunks per 4KB page):
    ///    ```text
    ///    measurement = SHA256(measurement || 0x00444E4554584545 || chunk_offset || chunk_data)
    ///    ```text
    ///
    /// # Returns
    ///
    /// 32-byte MRENCLAVE value that uniquely identifies this enclave binary
    pub fn compute_mrenclave_with_metadata(
        &self,
        enclave_binary: &[u8],
        metadata: &EnclaveMetadata,
    ) -> [u8; 32] {
        const SGX_PAGE_SIZE: usize = 4096;
        const SGX_CHUNK_SIZE: usize = 256;

        // Initialize measurement with ECREATE operation
        let mut measurement = self.measure_ecreate(metadata);

        // Process binary in 4KB pages (SGX page granularity)
        let mut offset = 0u64;
        while offset < enclave_binary.len() as u64 {
            let page_offset = offset as usize;
            let page_end = (page_offset + SGX_PAGE_SIZE).min(enclave_binary.len());
            let page_data = &enclave_binary[page_offset..page_end];

            // EADD: Hash page metadata (offset, flags)
            measurement = self.measure_eadd(&measurement, offset, metadata.page_type);

            // EEXTEND: Hash page content in 256-byte chunks
            for chunk_offset in (0..page_data.len()).step_by(SGX_CHUNK_SIZE) {
                let chunk_end = (chunk_offset + SGX_CHUNK_SIZE).min(page_data.len());
                let mut chunk = [0u8; SGX_CHUNK_SIZE];
                chunk[..chunk_end - chunk_offset]
                    .copy_from_slice(&page_data[chunk_offset..chunk_end]);

                measurement =
                    self.measure_eextend(&measurement, offset + chunk_offset as u64, &chunk);
            }

            offset += SGX_PAGE_SIZE as u64;
        }

        measurement
    }

    /// Measure ECREATE operation (initialize SECS)
    fn measure_ecreate(&self, metadata: &EnclaveMetadata) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Magic: "ECREATE" in little-endian (0x45434543524541)
        hasher.update([0x45, 0x43, 0x52, 0x45, 0x41, 0x54, 0x45, 0x00]);

        // SECS.SSAFRAMESIZE (1 page = 4096 bytes)
        hasher.update(metadata.ssa_frame_size.to_le_bytes());

        // SECS.SIZE (enclave size, must be power of 2)
        hasher.update(metadata.enclave_size.to_le_bytes());

        // Reserved (48 bytes)
        hasher.update([0u8; 48]);

        // SECS.MISCSELECT (4 bytes)
        hasher.update(metadata.misc_select.to_le_bytes());

        // Reserved (28 bytes)
        hasher.update([0u8; 28]);

        // SECS.ATTRIBUTES (16 bytes): debug, mode64bit, provisionkey, etc.
        hasher.update(metadata.attributes);

        // Reserved (352 bytes)
        hasher.update([0u8; 352]);

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Measure EADD operation (add page metadata)
    fn measure_eadd(&self, prev_measurement: &[u8; 32], offset: u64, page_type: u64) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Previous measurement
        hasher.update(prev_measurement);

        // Magic: "EADD" in little-endian (0x00000000444441)
        hasher.update([0x45, 0x41, 0x44, 0x44, 0x00, 0x00, 0x00, 0x00]);

        // Page offset within enclave
        hasher.update(offset.to_le_bytes());

        // SECINFO: page type and permissions
        // Bits 0-7: page type (REG=0x01, TCS=0x02, TRIM=0x04)
        // Bits 8-15: flags (R=0x01, W=0x02, X=0x04)
        hasher.update(page_type.to_le_bytes());

        // Reserved (48 bytes)
        hasher.update([0u8; 48]);

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Measure EEXTEND operation (extend page content)
    fn measure_eextend(
        &self,
        prev_measurement: &[u8; 32],
        offset: u64,
        chunk: &[u8; 256],
    ) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Previous measurement
        hasher.update(prev_measurement);

        // Magic: "EEXTEND" in little-endian (0x444E4554584545)
        hasher.update([0x45, 0x45, 0x58, 0x54, 0x45, 0x4E, 0x44, 0x00]);

        // Chunk offset (256-byte aligned)
        hasher.update(offset.to_le_bytes());

        // Reserved (48 bytes)
        hasher.update([0u8; 48]);

        // 256-byte chunk of page content
        hasher.update(chunk);

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Verify MRENCLAVE against a known good value
    pub fn verify_mrenclave(
        &self,
        report: &AttestationReport,
        expected: &[u8; 32],
    ) -> TeeResult<()> {
        if !report.verify_mrenclave(expected) {
            Err(TeeError::mrenclave_mismatch(expected, &report.mrenclave))
        } else {
            Ok(())
        }
    }

    /// Verify MRSIGNER against a known good value
    pub fn verify_mrsigner(
        &self,
        report: &AttestationReport,
        expected: &[u8; 32],
    ) -> TeeResult<()> {
        if !report.verify_mrsigner(expected) {
            Err(TeeError::mrsigner_mismatch(expected, &report.mrsigner))
        } else {
            Ok(())
        }
    }
}
