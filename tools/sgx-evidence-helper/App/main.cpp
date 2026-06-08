#include "Enclave_u.h"

#include <sgx_dcap_ql_wrapper.h>
#include <sgx_quote_3.h>
#include <sgx_urts.h>

#include <openssl/rand.h>
#include <openssl/sha.h>

#include <algorithm>
#include <array>
#include <cerrno>
#include <cstdint>
#include <cstdlib>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <sstream>
#include <stdexcept>
#include <string>
#include <vector>

namespace {
constexpr size_t kSealingKeyLen = 32;
constexpr size_t kReportDataLen = 64;
constexpr char kBindingLabel[] = "neo-tee-sgx-sealing-key-v1";

std::string to_hex(const uint8_t* data, size_t len) {
    std::ostringstream os;
    os << std::hex << std::setfill('0');
    for (size_t i = 0; i < len; ++i) {
        os << std::setw(2) << static_cast<unsigned>(data[i]);
    }
    return os.str();
}

std::array<uint8_t, kSealingKeyLen> random_sealing_key() {
    std::array<uint8_t, kSealingKeyLen> key = {};
    if (RAND_bytes(key.data(), static_cast<int>(key.size())) != 1) {
        throw std::runtime_error("RAND_bytes failed while creating sealing key");
    }
    return key;
}

std::array<uint8_t, kReportDataLen> make_report_data(
    const std::array<uint8_t, kSealingKeyLen>& sealing_key
) {
    std::array<uint8_t, kReportDataLen> report_data = {};
    std::array<uint8_t, SHA256_DIGEST_LENGTH> digest = {};

    SHA256_CTX ctx;
    SHA256_Init(&ctx);
    SHA256_Update(&ctx, kBindingLabel, sizeof(kBindingLabel) - 1);
    SHA256_Update(&ctx, sealing_key.data(), sealing_key.size());
    SHA256_Final(digest.data(), &ctx);

    std::copy(digest.begin(), digest.end(), report_data.begin());
    return report_data;
}

void write_binary_file(const std::filesystem::path& path, const uint8_t* data, size_t len) {
    std::ofstream out(path, std::ios::binary | std::ios::trunc);
    if (!out) {
        throw std::runtime_error("failed to open output file: " + path.string());
    }
    out.write(reinterpret_cast<const char*>(data), static_cast<std::streamsize>(len));
    out.close();
    if (!out.good()) {
        throw std::runtime_error("failed to write output file: " + path.string());
    }
}

std::string sgx_status_hex(sgx_status_t s) {
    std::ostringstream os;
    os << "0x" << std::hex << std::setfill('0') << std::setw(4) << static_cast<uint32_t>(s);
    return os.str();
}

std::string quote3_status_hex(quote3_error_t s) {
    std::ostringstream os;
    os << "0x" << std::hex << std::setfill('0') << std::setw(4) << static_cast<uint32_t>(s);
    return os.str();
}

} // namespace

int main(int argc, char** argv) {
    try {
        const std::filesystem::path out_dir = (argc >= 2) ? argv[1] : "./tee_data";
        const std::filesystem::path helper_dir = std::filesystem::absolute(argv[0]).parent_path();
        const std::filesystem::path enclave_path =
            (argc >= 3) ? argv[2] : (helper_dir / "enclave.signed.so");

        std::filesystem::create_directories(out_dir);
        std::filesystem::permissions(
            out_dir,
            std::filesystem::perms::owner_all,
            std::filesystem::perm_options::replace
        );

        const auto sealing_key = random_sealing_key();
        const auto report_data = make_report_data(sealing_key);

        sgx_enclave_id_t enclave_id = 0;
        sgx_launch_token_t launch_token = {};
        int launch_token_updated = 0;

        const uint32_t enclave_debug_flag = []() {
            const char* env = std::getenv("NEO_SGX_HELPER_DEBUG_ENCLAVE");
            if (env != nullptr && std::string(env) == "1") {
                return static_cast<uint32_t>(SGX_DEBUG_FLAG);
            }
            return uint32_t {0};
        }();

        sgx_status_t status = sgx_create_enclave(
            enclave_path.c_str(),
            enclave_debug_flag,
            &launch_token,
            &launch_token_updated,
            &enclave_id,
            nullptr
        );
        if (status != SGX_SUCCESS) {
            throw std::runtime_error("sgx_create_enclave failed: " + sgx_status_hex(status));
        }

        sgx_target_info_t qe_target_info = {};
        quote3_error_t qe_ret = sgx_qe_get_target_info(&qe_target_info);
        if (qe_ret != SGX_QL_SUCCESS) {
            sgx_destroy_enclave(enclave_id);
            throw std::runtime_error("sgx_qe_get_target_info failed: " + quote3_status_hex(qe_ret));
        }

        sgx_report_t report = {};
        sgx_status_t ecall_return = SGX_SUCCESS;
        status = ecall_create_qe_report(
            enclave_id,
            &ecall_return,
            &qe_target_info,
            report_data.data(),
            &report
        );
        if (status != SGX_SUCCESS || ecall_return != SGX_SUCCESS) {
            sgx_destroy_enclave(enclave_id);
            throw std::runtime_error(
                "ecall_create_qe_report failed. status=" + sgx_status_hex(status) +
                ", ecall_return=" + sgx_status_hex(ecall_return)
            );
        }

        uint32_t quote_size = 0;
        qe_ret = sgx_qe_get_quote_size(&quote_size);
        if (qe_ret != SGX_QL_SUCCESS || quote_size == 0) {
            sgx_destroy_enclave(enclave_id);
            throw std::runtime_error("sgx_qe_get_quote_size failed: " + quote3_status_hex(qe_ret));
        }

        std::vector<uint8_t> quote(quote_size);
        qe_ret = sgx_qe_get_quote(&report, quote_size, quote.data());
        sgx_destroy_enclave(enclave_id);
        if (qe_ret != SGX_QL_SUCCESS) {
            throw std::runtime_error("sgx_qe_get_quote failed: " + quote3_status_hex(qe_ret));
        }

        const auto quote_path = out_dir / "sgx.quote";
        const auto key_path = out_dir / "sgx.sealing_key";
        write_binary_file(quote_path, quote.data(), quote.size());
        write_binary_file(key_path, sealing_key.data(), sealing_key.size());
        std::filesystem::permissions(
            quote_path,
            std::filesystem::perms::owner_read | std::filesystem::perms::owner_write,
            std::filesystem::perm_options::replace
        );
        std::filesystem::permissions(
            key_path,
            std::filesystem::perms::owner_read | std::filesystem::perms::owner_write,
            std::filesystem::perm_options::replace
        );

        if (quote.size() >= sizeof(sgx_quote3_t)) {
            const auto* quote_v3 = reinterpret_cast<const sgx_quote3_t*>(quote.data());
            std::cout << "Quote metadata:\n";
            std::cout << "  version: " << quote_v3->header.version << "\n";
            std::cout << "  att_key_type: " << quote_v3->header.att_key_type << "\n";
            std::cout << "  isv_svn: " << quote_v3->report_body.isv_svn << "\n";
            std::cout << "  mrenclave: "
                      << to_hex(quote_v3->report_body.mr_enclave.m, sizeof(quote_v3->report_body.mr_enclave.m))
                      << "\n";
            std::cout << "  mrsigner: "
                      << to_hex(quote_v3->report_body.mr_signer.m, sizeof(quote_v3->report_body.mr_signer.m))
                      << "\n";
            std::cout << "  report_data[0..32]: "
                      << to_hex(quote_v3->report_body.report_data.d, 32) << "\n";
        }

        std::cout << "Generated SGX evidence:\n";
        std::cout << "  quote: " << quote_path << " (" << quote.size() << " bytes)\n";
        std::cout << "  sealing key: " << key_path << " (" << sealing_key.size() << " bytes)\n";
        std::cout << "  binding digest: " << to_hex(report_data.data(), 32) << "\n";
        return 0;
    } catch (const std::exception& ex) {
        std::cerr << "sgx_evidence_helper error: " << ex.what() << "\n";
        return 1;
    }
}
