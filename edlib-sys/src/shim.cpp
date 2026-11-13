#include "shim.h"

// noexcept turns any escaping exception into std::terminate, so Rust never
// sees a foreign unwind through a "C" ABI call.
extern "C" EdlibAlignResult edlibAlignNoexcept(const char* query, int queryLength,
                                               const char* target, int targetLength,
                                               const EdlibAlignConfig config) noexcept {
    return edlibAlign(query, queryLength, target, targetLength, config);
}
