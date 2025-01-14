# Declare the opaque types
cdef extern from "configmaton.h":
    ctypedef struct OwnedConfigmaton:
        pass

    ctypedef struct FfiConfigmaton:
        pass

    struct Bytestring:
        size_t len
        const unsigned char* data

    OwnedConfigmaton* new_configmaton_base(const unsigned char* buf, size_t len)
    void drop_configmaton_base(OwnedConfigmaton* base)
    FfiConfigmaton* base_get_configmaton(OwnedConfigmaton* base)
    FfiConfigmaton* configmaton_make_child(FfiConfigmaton* configmaton)
    void configmaton_set(FfiConfigmaton* configmaton, const unsigned char* key, size_t key_len,
             const unsigned char* value, size_t value_len)
    Bytestring configmaton_get(
            const FfiConfigmaton* configmaton, const unsigned char* key, size_t key_len)
    Bytestring configmaton_pop_command(FfiConfigmaton* configmaton)
