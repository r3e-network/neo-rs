#include "Enclave_t.h"

#include <sgx_trts.h>
#include <sgx_utils.h>

#include <cstring>

extern "C" sgx_status_t ecall_create_qe_report(
    const sgx_target_info_t* qe_target_info,
    const uint8_t* report_data,
    sgx_report_t* report
) {
    if (qe_target_info == nullptr || report_data == nullptr || report == nullptr) {
        return SGX_ERROR_INVALID_PARAMETER;
    }

    sgx_report_data_t sgx_report_data = {};
    std::memcpy(sgx_report_data.d, report_data, sizeof(sgx_report_data.d));

    return sgx_create_report(qe_target_info, &sgx_report_data, report);
}
