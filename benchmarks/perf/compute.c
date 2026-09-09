#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>

static inline int64_t checkedAdd(int64_t left, int64_t right) {
    int64_t result;
    if (__builtin_add_overflow(left, right, &result)) {
        abort();
    }
    return result;
}

int main(void) {
    int64_t total = 0;
    for (int64_t value = 0; value < 100000000; ++value) {
        total = checkedAdd(total, value);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
