SGX_SDK ?= /tmp/intel-sgxsdk/sgxsdk
SGX_MODE ?= HW
SGX_ARCH ?= x64
SGX_DEBUG ?= 0

include $(SGX_SDK)/buildenv.mk

ifeq ($(SGX_ARCH), x86)
$(error x86 is not supported for this helper)
endif

SGX_COMMON_FLAGS := -m64 -Wall -Wextra
ifeq ($(SGX_DEBUG), 1)
SGX_COMMON_FLAGS += -O0 -g
else
SGX_COMMON_FLAGS += -O2
endif

SGX_COMMON_CFLAGS := $(SGX_COMMON_FLAGS)
SGX_COMMON_CXXFLAGS := $(SGX_COMMON_FLAGS) -std=c++17

SGX_EDGER8R := $(SGX_SDK)/bin/x64/sgx_edger8r
SGX_ENCLAVE_SIGNER := $(SGX_SDK)/bin/x64/sgx_sign

APP_NAME := sgx_evidence_helper
APP_SRCS := App/main.cpp App/Enclave_u.c
APP_OBJS := $(APP_SRCS:.cpp=.o)
APP_OBJS := $(APP_OBJS:.c=.o)
APP_INC := -IApp -IEnclave -I$(SGX_SDK)/include
APP_CFLAGS := $(APP_INC) -fPIC
APP_CXXFLAGS := $(APP_CFLAGS)
APP_LDFLAGS := -L$(SGX_SDK)/lib64 -lsgx_urts -lsgx_dcap_ql -lcrypto -ldl -lpthread

ENCLAVE_NAME := enclave.so
SIGNED_ENCLAVE_NAME := enclave.signed.so
ENCLAVE_SRCS := Enclave/Enclave.cpp Enclave/Enclave_t.c
ENCLAVE_OBJS := $(ENCLAVE_SRCS:.cpp=.o)
ENCLAVE_OBJS := $(ENCLAVE_OBJS:.c=.o)
ENCLAVE_INC := -IEnclave -I$(SGX_SDK)/include -I$(SGX_SDK)/include/tlibc -I$(SGX_SDK)/include/libcxx
ENCLAVE_CFLAGS := $(ENCLAVE_INC) -nostdinc -fvisibility=hidden -fpie -ffunction-sections -fdata-sections
ENCLAVE_CXXFLAGS := $(ENCLAVE_CFLAGS) -nostdinc++
ENCLAVE_LDFLAGS := \
	-Wl,--no-undefined -nostdlib -nodefaultlibs -nostartfiles \
	-L$(SGX_TRUSTED_LIBRARY_PATH) \
	-Wl,--whole-archive -lsgx_trts -Wl,--no-whole-archive \
	-Wl,--start-group -lsgx_tstdc -lsgx_tcxx -lsgx_tcrypto -lsgx_tservice -Wl,--end-group \
	-Wl,-Bstatic -Wl,-Bsymbolic -Wl,--no-undefined \
	-Wl,-pie,-eenclave_entry -Wl,--export-dynamic \
	-Wl,--defsym,__ImageBase=0 -Wl,--gc-sections \
	-Wl,--version-script=Enclave/Enclave.lds

ENCLAVE_CONFIG := Enclave/Enclave.config.xml
ENCLAVE_TEST_KEY := Enclave/Enclave_private_test.pem

.PHONY: all clean

all: $(APP_NAME) $(SIGNED_ENCLAVE_NAME)

App/Enclave_u.h App/Enclave_u.c: Enclave/Enclave.edl
	$(SGX_EDGER8R) --untrusted Enclave/Enclave.edl --search-path Enclave --search-path $(SGX_SDK)/include --untrusted-dir App

Enclave/Enclave_t.h Enclave/Enclave_t.c: Enclave/Enclave.edl
	$(SGX_EDGER8R) --trusted Enclave/Enclave.edl --search-path Enclave --search-path $(SGX_SDK)/include --trusted-dir Enclave

App/%.o: App/%.c App/Enclave_u.h
	$(CC) $(SGX_COMMON_CFLAGS) $(APP_CFLAGS) -c $< -o $@

App/%.o: App/%.cpp App/Enclave_u.h
	$(CXX) $(SGX_COMMON_CXXFLAGS) $(APP_CXXFLAGS) -c $< -o $@

$(APP_NAME): $(APP_OBJS)
	$(CXX) $^ -o $@ $(APP_LDFLAGS)

Enclave/%.o: Enclave/%.c Enclave/Enclave_t.h
	$(CC) $(SGX_COMMON_CFLAGS) $(ENCLAVE_CFLAGS) -c $< -o $@

Enclave/%.o: Enclave/%.cpp Enclave/Enclave_t.h
	$(CXX) $(SGX_COMMON_CXXFLAGS) $(ENCLAVE_CXXFLAGS) -c $< -o $@

$(ENCLAVE_NAME): $(ENCLAVE_OBJS)
	$(CXX) $^ -o $@ $(ENCLAVE_LDFLAGS)

$(SIGNED_ENCLAVE_NAME): $(ENCLAVE_NAME)
ifeq ($(wildcard $(ENCLAVE_TEST_KEY)),)
	openssl genrsa -out $(ENCLAVE_TEST_KEY) -3 3072
endif
	$(SGX_ENCLAVE_SIGNER) sign -key $(ENCLAVE_TEST_KEY) -enclave $(ENCLAVE_NAME) -out $@ -config $(ENCLAVE_CONFIG)

clean:
	rm -f $(APP_NAME) $(ENCLAVE_NAME) $(SIGNED_ENCLAVE_NAME) \
		App/*.o App/Enclave_u.c App/Enclave_u.h \
		Enclave/*.o Enclave/Enclave_t.c Enclave/Enclave_t.h \
		$(ENCLAVE_TEST_KEY)
