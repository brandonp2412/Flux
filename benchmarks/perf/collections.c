#include <inttypes.h>
#include <stdbool.h>
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

static inline int64_t checkedMul(int64_t left, int64_t right) {
    int64_t result;
    if (__builtin_mul_overflow(left, right, &result)) {
        abort();
    }
    return result;
}

int main(void) {
    int64_t total = 0;
    for (int64_t iteration = 0; iteration < 50000000; ++iteration) {
        const int64_t values[] = {iteration, 2, 3, 4, 5, 6, 7, 8};
        int64_t partial = 0;
        for (size_t index = 0; index < 8; ++index) {
            int64_t mapped = checkedMul(values[index], 2);
            if (mapped > 5) {
                partial = checkedAdd(partial, mapped);
            }
        }
        total = checkedAdd(total, partial);
    }
    printf("%" PRId64 "\n", total);
    return 0;
}
