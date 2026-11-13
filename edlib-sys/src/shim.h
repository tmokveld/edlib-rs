#ifndef EDLIB_SYS_SHIM_H
#define EDLIB_SYS_SHIM_H

#include "edlib.h"

#ifdef __cplusplus
#define EDLIB_SYS_NOEXCEPT noexcept
extern "C" {
#else
#define EDLIB_SYS_NOEXCEPT
#endif

/**
 * Same as edlibAlign, but a C++ exception (such as std::bad_alloc) terminates
 * the process instead of unwinding into the caller.
 */
EdlibAlignResult edlibAlignNoexcept(const char* query, int queryLength,
                                    const char* target, int targetLength,
                                    const EdlibAlignConfig config) EDLIB_SYS_NOEXCEPT;

#ifdef __cplusplus
}
#endif

#endif // EDLIB_SYS_SHIM_H
